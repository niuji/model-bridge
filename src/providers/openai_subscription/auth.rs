use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(super) const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(super) const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
pub(super) const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
// Compatibility version follows the researched upstream adapter; live account acceptance is not guaranteed.
pub(super) const MODELS_URL: &str =
    "https://chatgpt.com/backend-api/codex/models?client_version=0.155.1";

pub(super) fn authorization_url(state: &str, verifier: &str) -> String {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse("https://auth.openai.com/oauth/authorize").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", REDIRECT_URI),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "model-bridge"),
    ]);
    url.to_string()
}

pub(super) fn parse_callback(raw: &str, state: &str) -> Result<String> {
    if raw.len() > 16384 {
        bail!("OAuth callback is too large");
    }
    let url = reqwest::Url::parse(raw).map_err(|_| anyhow!("Invalid callback URL"))?;
    if url.scheme() != "http"
        || url.host_str() != Some("localhost")
        || url.port() != Some(1455)
        || url.path() != "/auth/callback"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("Invalid callback URL");
    }
    let pairs: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = pairs.iter().filter(|(key, _)| key == "state").collect();
    if states.len() != 1 || states[0].1 != state {
        bail!("Invalid OAuth state");
    }
    if pairs.iter().any(|(key, _)| key == "error") {
        bail!("OAuth authorization was rejected");
    }
    let codes: Vec<_> = pairs.iter().filter(|(key, _)| key == "code").collect();
    if codes.len() != 1 || codes[0].1.is_empty() {
        bail!("Missing or duplicate authorization code");
    }
    Ok(codes[0].1.to_string())
}

pub(super) async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("Upstream response interrupted"))?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            bail!("Upstream response exceeds limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("Invalid upstream JSON"))
}

pub(super) fn token_account(token: &str) -> Result<(String, Option<String>)> {
    // Claims are used only as metadata from the TLS-protected token endpoint, never as JWT authentication.
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("Account identity missing"))?;
    let raw = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| anyhow!("Invalid account identity"))?;
    let value: Value =
        serde_json::from_slice(&raw).map_err(|_| anyhow!("Invalid account identity"))?;
    let account = value
        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Account identity missing"))?;
    if account.len() > 512 || account.contains(['\r', '\n']) {
        bail!("Invalid account identity");
    }
    let label = value
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((account.to_owned(), label))
}
