mod auth;
mod models;
use anyhow::{anyhow, bail, Result};
use auth::*;
use models::parse_models;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Serialize)]
pub struct SubscriptionSummary {
    pub status: String,
    pub account_id: Option<String>,
    pub account_label: Option<String>,
}
// Deliberately neither Debug nor Serialize: access tokens must never enter diagnostics or Admin JSON.
pub struct SubscriptionCredentials {
    pub access_token: String,
    pub account_id: String,
    pub credential_version: String,
}
#[derive(Serialize)]
pub struct LoginStart {
    pub session_id: String,
    pub authorization_url: String,
    pub expires_at: i64,
}
#[derive(Serialize)]
pub struct LoginStatus {
    pub status: String,
}
struct LoginSession {
    provider_id: String,
    id: String,
    state: String,
    verifier: String,
    expires_at: i64,
    status: String,
    cancel: CancellationToken,
}
impl LoginSession {
    fn active(&self) -> bool {
        self.status == "pending" || self.status == "exchanging"
    }
    fn finish(&mut self, status: &str) {
        self.status = status.to_owned();
        self.state.clear();
        self.verifier.clear();
        self.cancel.cancel();
    }
}
struct Stored {
    account_id: String,
    access: String,
    refresh: String,
    expires_at: i64,
    version: String,
    status: String,
}

pub struct SubscriptionService {
    db: SqlitePool,
    client: reqwest::Client,
    key: Option<[u8; 32]>,
    sessions: Mutex<Option<LoginSession>>,
    refresh_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    token_url: String,
    models_url: String,
}
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn login_result_page(succeeded: bool) -> (axum::http::StatusCode, axum::response::Html<String>) {
    let (status, title, message, color) = if succeeded {
        (
            axum::http::StatusCode::OK,
            "登录成功",
            "OpenAI 订阅账号已登录。可以关闭此页面，返回 Model Bridge 继续使用。",
            "#15803d",
        )
    } else {
        (
            axum::http::StatusCode::BAD_REQUEST,
            "登录失败",
            "未能完成 OpenAI 订阅登录。可以关闭此页面，返回 Model Bridge 重新发起登录。",
            "#b91c1c",
        )
    };
    (
        status,
        axum::response::Html(format!(
            r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<title>{title} · Model Bridge</title>
<style>
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: #f5f6f8; color: #1f2937; font-family: system-ui, sans-serif; }}
main {{ box-sizing: border-box; width: min(440px, calc(100% - 32px)); padding: 40px 32px; border-radius: 16px; background: white; text-align: center; box-shadow: 0 8px 32px #0000000a; }}
.brand {{ color: #6b7280; font-size: 14px; }}
h1 {{ color: {color}; font-size: 28px; }}
p {{ line-height: 1.8; }}
</style>
</head>
<body><main><div class="brand">Model Bridge · OpenAI 订阅</div><h1>{title}</h1><p>{message}</p></main></body>
</html>"#
        )),
    )
}

impl SubscriptionService {
    pub fn new(db: SqlitePool, client: reqwest::Client, key: Option<[u8; 32]>) -> Self {
        Self {
            db,
            client,
            key,
            sessions: Mutex::new(None),
            refresh_locks: Mutex::new(HashMap::new()),
            token_url: TOKEN_URL.into(),
            models_url: MODELS_URL.into(),
        }
    }
    pub async fn start_login(
        self: &Arc<Self>,
        provider_id: &str,
        admin_url: &str,
    ) -> Result<LoginStart> {
        if self.key.is_none() {
            bail!("Configure database.encryption_key before subscription login");
        }
        let admin = reqwest::Url::parse(admin_url).map_err(|_| anyhow!("Invalid Admin address"))?;
        if !matches!(admin.scheme(), "http" | "https")
            || !admin.username().is_empty()
            || admin.password().is_some()
        {
            bail!("Invalid Admin address");
        }
        let mut guard = self.sessions.lock().await;
        if guard
            .as_ref()
            .is_some_and(|s| s.active() && s.expires_at > now())
        {
            bail!("A subscription login is already in progress");
        }
        if let Some(s) = guard.as_mut() {
            s.finish("expired");
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:1455")
            .await
            .map_err(|_| {
                anyhow!("OAuth callback port 1455 is unavailable; close the other login and retry")
            })?;
        let id = random();
        let state = random();
        let verifier = random();
        let expires_at = now() + 600;
        let authorization_url = authorization_url(&state, &verifier);
        let cancel = CancellationToken::new();
        *guard = Some(LoginSession {
            provider_id: provider_id.into(),
            id: id.clone(),
            state,
            verifier,
            expires_at,
            status: "pending".into(),
            cancel: cancel.clone(),
        });
        drop(guard);
        let service = self.clone();
        let provider = provider_id.to_owned();
        let session = id.clone();
        let app = axum::Router::new().route(
            "/auth/callback",
            axum::routing::get(move |uri: axum::http::Uri| {
                let service = service.clone();
                let provider = provider.clone();
                let session = session.clone();
                async move {
                    let raw = format!("http://localhost:1455{}", uri);
                    let succeeded = matches!(
                        service.submit_callback(&provider, &session, &raw).await,
                        Ok(status) if status.status == "succeeded"
                    );
                    login_result_page(succeeded)
                }
            }),
        );
        let shutdown = cancel.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await;
        });
        let service = Arc::downgrade(self);
        let session = id.clone();
        tokio::spawn(async move {
            tokio::select! { _=tokio::time::sleep(Duration::from_secs(600))=>{}, _=cancel.cancelled()=>{} }
            if let Some(service) = service.upgrade() {
                let mut guard = service.sessions.lock().await;
                if let Some(s) = guard.as_mut().filter(|s| s.id == session && s.active()) {
                    s.finish("expired");
                }
            }
        });
        Ok(LoginStart {
            session_id: id,
            authorization_url,
            expires_at,
        })
    }
    pub async fn login_status(&self, provider_id: &str, session_id: &str) -> Result<LoginStatus> {
        let mut guard = self.sessions.lock().await;
        let s = guard
            .as_mut()
            .filter(|s| s.provider_id == provider_id && s.id == session_id)
            .ok_or_else(|| anyhow!("Login session not found"))?;
        if s.active() && s.expires_at <= now() {
            s.finish("expired");
        }
        Ok(LoginStatus {
            status: s.status.clone(),
        })
    }
    pub async fn cancel_login(&self, provider_id: &str, session_id: &str) -> Result<()> {
        let mut guard = self.sessions.lock().await;
        let s = guard
            .as_mut()
            .filter(|s| s.provider_id == provider_id && s.id == session_id)
            .ok_or_else(|| anyhow!("Login session not found"))?;
        if s.active() {
            s.finish("cancelled");
        }
        Ok(())
    }
    pub async fn logout(&self, provider_id: &str) -> Result<()> {
        // Serialize deletion with callback commit so an in-flight exchange cannot recreate a logged-out account.
        let mut guard = self.sessions.lock().await;
        if let Some(s) = guard
            .as_mut()
            .filter(|s| s.provider_id == provider_id && s.active())
        {
            s.finish("cancelled");
        }
        sqlx::query("DELETE FROM provider_subscription_accounts WHERE provider_id=?")
            .bind(provider_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }
    pub async fn submit_callback(
        &self,
        provider_id: &str,
        session_id: &str,
        url: &str,
    ) -> Result<LoginStatus> {
        let (code, verifier) = {
            let mut guard = self.sessions.lock().await;
            let s = guard
                .as_mut()
                .filter(|s| s.provider_id == provider_id && s.id == session_id)
                .ok_or_else(|| anyhow!("Login session not found"))?;
            if s.expires_at <= now() {
                s.finish("expired");
                bail!("Login session expired");
            }
            if s.status != "pending" {
                bail!("Login callback already claimed or cancelled");
            }
            let code = match parse_callback(url, &s.state) {
                Ok(code) => code,
                Err(error) => {
                    if error.to_string() == "OAuth authorization was rejected" {
                        s.finish("failed");
                    }
                    return Err(error);
                }
            };
            s.status = "exchanging".into();
            s.state.clear();
            (code, std::mem::take(&mut s.verifier))
        };
        let result = self
            .exchange(&[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", &verifier),
                ("client_id", CLIENT_ID),
            ])
            .await;
        let mut guard = self.sessions.lock().await;
        let s = guard
            .as_mut()
            .filter(|s| s.provider_id == provider_id && s.id == session_id)
            .ok_or_else(|| anyhow!("Login session cancelled"))?;
        if s.status != "exchanging" || s.expires_at <= now() {
            if s.active() {
                s.finish("expired");
            }
            bail!("Login session cancelled or expired");
        }
        let result = match result {
            Ok(data) => self.save_login(provider_id, data).await,
            Err(e) => Err(e),
        };
        s.finish(if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        });
        result?;
        Ok(LoginStatus {
            status: s.status.clone(),
        })
    }
    async fn exchange(&self, form: &[(&str, &str)]) -> Result<Value> {
        let response = self
            .client
            .post(&self.token_url)
            .timeout(Duration::from_secs(30))
            .form(form)
            .send()
            .await
            .map_err(|_| anyhow!("Subscription token request failed; retry later"))?;
        let status = response.status();
        let data = bounded_json(response).await?;
        if !status.is_success() {
            let code = data
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| data.pointer("/error/code").and_then(Value::as_str));
            if matches!(status.as_u16(), 400 | 401)
                && code.is_some_and(|s| {
                    matches!(
                        s,
                        "invalid_grant"
                            | "invalid_token"
                            | "refresh_token_expired"
                            | "refresh_token_reused"
                            | "refresh_token_invalidated"
                    )
                })
            {
                bail!("subscription_reauth_required");
            }
            bail!(
                "Subscription token request rejected (HTTP {})",
                status.as_u16()
            );
        }
        Ok(data)
    }
    async fn save_login(&self, provider_id: &str, data: Value) -> Result<()> {
        let access = required_token(&data, "access_token")?;
        let refresh = required_token(&data, "refresh_token")?;
        let (account, label) = token_account(
            data.get("id_token")
                .and_then(Value::as_str)
                .unwrap_or(access),
        )
        .or_else(|_| token_account(access))?;
        let access = crate::crypto::seal_required(self.key.as_ref(), access)?;
        let refresh = crate::crypto::seal_required(self.key.as_ref(), refresh)?;
        let expires = expiry(&data)?;
        let mut tx = self.db.begin().await?;
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT account_id FROM provider_subscription_accounts WHERE provider_id=?",
        )
        .bind(provider_id)
        .fetch_optional(&mut *tx)
        .await?;
        if previous.as_ref() != Some(&account) {
            for table in ["provider_models", "upstream_models", "upstream_models_seen"] {
                sqlx::query(&format!("DELETE FROM {table} WHERE provider_id=?"))
                    .bind(provider_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        sqlx::query("INSERT INTO provider_subscription_accounts (provider_id,account_id,account_label,access_token_encrypted,refresh_token_encrypted,expires_at,credential_version,auth_status,updated_at) VALUES (?,?,?,?,?,?,?,'authorized',?) ON CONFLICT(provider_id) DO UPDATE SET account_id=excluded.account_id,account_label=excluded.account_label,access_token_encrypted=excluded.access_token_encrypted,refresh_token_encrypted=excluded.refresh_token_encrypted,expires_at=excluded.expires_at,credential_version=excluded.credential_version,auth_status='authorized',updated_at=excluded.updated_at")
            .bind(provider_id).bind(account).bind(label).bind(access).bind(refresh).bind(expires).bind(random()).bind(now()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn summary(&self, provider_id: &str) -> Result<SubscriptionSummary> {
        let row=sqlx::query("SELECT account_id,account_label,auth_status FROM provider_subscription_accounts WHERE provider_id=?").bind(provider_id).fetch_optional(&self.db).await?;
        Ok(match row {
            Some(row) => SubscriptionSummary {
                status: row.get("auth_status"),
                account_id: Some(row.get("account_id")),
                account_label: row.get("account_label"),
            },
            None => SubscriptionSummary {
                status: "unconfigured".into(),
                account_id: None,
                account_label: None,
            },
        })
    }
    async fn stored(&self, provider_id: &str) -> Result<Stored> {
        let row = sqlx::query("SELECT * FROM provider_subscription_accounts WHERE provider_id=?")
            .bind(provider_id)
            .fetch_optional(&self.db)
            .await?
            .ok_or_else(|| anyhow!("Subscription is not logged in"))?;
        Ok(Stored {
            account_id: row.get("account_id"),
            access: row.get("access_token_encrypted"),
            refresh: row.get("refresh_token_encrypted"),
            expires_at: row.get("expires_at"),
            version: row.get("credential_version"),
            status: row.get("auth_status"),
        })
    }
    fn decode(&self, row: Stored) -> Result<SubscriptionCredentials> {
        if row.status != "authorized" {
            bail!("Subscription requires login");
        }
        Ok(SubscriptionCredentials {
            access_token: crate::crypto::reveal_required(self.key.as_ref(), &row.access)?,
            account_id: row.account_id,
            credential_version: row.version,
        })
    }
    pub async fn credentials(&self, provider_id: &str) -> Result<SubscriptionCredentials> {
        let row = self.stored(provider_id).await?;
        if row.expires_at > now() + 60 {
            return self.decode(row);
        }
        self.refresh(provider_id, None).await
    }
    pub async fn force_refresh(
        &self,
        provider_id: &str,
        expected_version: &str,
    ) -> Result<SubscriptionCredentials> {
        self.refresh(provider_id, Some(expected_version)).await
    }
    async fn refresh(
        &self,
        provider_id: &str,
        expected: Option<&str>,
    ) -> Result<SubscriptionCredentials> {
        let lock = self
            .refresh_locks
            .lock()
            .await
            .entry(provider_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;
        let row = self.stored(provider_id).await?;
        if row.status != "authorized" {
            bail!("Subscription requires login");
        }
        if expected.is_some_and(|v| v != row.version)
            || (expected.is_none() && row.expires_at > now() + 60)
        {
            return self.decode(row);
        }
        let refresh = crate::crypto::reveal_required(self.key.as_ref(), &row.refresh)?;
        let data = match self
            .exchange(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", CLIENT_ID),
            ])
            .await
        {
            Ok(data) => data,
            Err(err) => {
                if err.to_string() == "subscription_reauth_required" {
                    let updated = sqlx::query("UPDATE provider_subscription_accounts SET auth_status='reauth_required',updated_at=? WHERE provider_id=? AND credential_version=?").bind(now()).bind(provider_id).bind(&row.version).execute(&self.db).await?;
                    if updated.rows_affected() == 0 {
                        return self.decode(self.stored(provider_id).await?);
                    }
                }
                return Err(err);
            }
        };
        let access = required_token(&data, "access_token")?;
        for token in [Some(access), data.get("id_token").and_then(Value::as_str)]
            .into_iter()
            .flatten()
        {
            if token_account(token).is_ok_and(|(account, _)| account != row.account_id) {
                bail!("Refreshed subscription account identity changed; log in again");
            }
        }
        let new_refresh = data
            .get("refresh_token")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(&refresh);
        let access = crate::crypto::seal_required(self.key.as_ref(), access)?;
        let refresh = crate::crypto::seal_required(self.key.as_ref(), new_refresh)?;
        sqlx::query("UPDATE provider_subscription_accounts SET access_token_encrypted=?,refresh_token_encrypted=?,expires_at=?,credential_version=?,updated_at=? WHERE provider_id=? AND credential_version=?")
            .bind(access).bind(refresh).bind(expiry(&data)?).bind(random()).bind(now()).bind(provider_id).bind(row.version).execute(&self.db).await?;
        self.decode(self.stored(provider_id).await?)
    }
    pub async fn models(&self, provider_id: &str) -> Result<Vec<(String, String)>> {
        let mut credentials = self.credentials(provider_id).await?;
        for attempt in 0..2 {
            let response = self
                .client
                .get(&self.models_url)
                .timeout(Duration::from_secs(30))
                .bearer_auth(&credentials.access_token)
                .header("chatgpt-account-id", &credentials.account_id)
                .header("originator", "model-bridge")
                .header("User-Agent", "model-bridge")
                .send()
                .await
                .map_err(|_| anyhow!("Subscription model request failed"))?;
            if response.status() == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                credentials = self
                    .force_refresh(provider_id, &credentials.credential_version)
                    .await?;
                continue;
            }
            if !response.status().is_success() {
                bail!(
                    "Subscription model request rejected (HTTP {})",
                    response.status().as_u16()
                );
            }
            let models = parse_models(bounded_json(response).await?)?;
            let current = self.stored(provider_id).await?;
            if current.status != "authorized"
                || current.version != credentials.credential_version
                || current.account_id != credentials.account_id
            {
                bail!("Subscription account changed during model discovery; retry");
            }
            return Ok(models);
        }
        bail!("Subscription authentication rejected")
    }
}
fn required_token<'a>(data: &'a Value, key: &str) -> Result<&'a str> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Incomplete token response"))
}
fn expiry(data: &Value) -> Result<i64> {
    let seconds = data
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|s| *s > 0 && *s <= 366 * 86400)
        .ok_or_else(|| anyhow!("Invalid token lifetime"))?;
    Ok(now() + seconds)
}

#[cfg(test)]
mod tests {
    #[test]
    fn callback_rejects_wrong_origin_and_duplicate_state() {
        assert!(
            super::parse_callback("https://evil.example/auth/callback?code=x&state=s", "s")
                .is_err()
        );
        assert!(super::parse_callback(
            "http://localhost:1455/auth/callback?code=x&state=s&state=s",
            "s"
        )
        .is_err());
        assert!(super::parse_callback(
            "http://localhost:1455/auth/callback?code=x&state=wrong",
            "s"
        )
        .is_err());
        assert_eq!(
            super::parse_callback("http://localhost:1455/auth/callback?code=x&state=s", "s")
                .unwrap(),
            "x"
        );
    }
    #[test]
    fn catalog_uses_subscription_visibility_not_platform_api_support() {
        let data = serde_json::json!({"models":[{"slug":"visible","display_name":"Visible","visibility":"list","supported_in_api":true},{"slug":"hidden","visibility":"hide"},{"slug":"subscription-only","supported_in_api":false},{"slug":"picker-hidden","show_in_picker":false}]});
        assert_eq!(
            super::parse_models(data).unwrap(),
            vec![
                (
                    "subscription-only".to_string(),
                    "subscription-only".to_string()
                ),
                ("visible".to_string(), "Visible".to_string())
            ]
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

    async fn service(server: &MockServer) -> Arc<SubscriptionService> {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE provider_subscription_accounts (provider_id TEXT PRIMARY KEY,account_id TEXT NOT NULL,account_label TEXT,access_token_encrypted TEXT NOT NULL,refresh_token_encrypted TEXT NOT NULL,expires_at INTEGER NOT NULL,credential_version TEXT NOT NULL,auth_status TEXT NOT NULL,updated_at INTEGER NOT NULL)").execute(&db).await.unwrap();
        for table in ["provider_models", "upstream_models", "upstream_models_seen"] {
            sqlx::query(&format!(
                "CREATE TABLE {table} (provider_id TEXT,model_id TEXT)"
            ))
            .execute(&db)
            .await
            .unwrap();
        }
        let mut service = SubscriptionService::new(db, reqwest::Client::new(), Some([9; 32]));
        service.token_url = server.uri();
        Arc::new(service)
    }
    async fn account(service: &SubscriptionService, version: &str, expires: i64) {
        let access = crate::crypto::seal_required(Some(&[9; 32]), "old-access").unwrap();
        let refresh = crate::crypto::seal_required(Some(&[9; 32]), "old-refresh").unwrap();
        sqlx::query("INSERT OR REPLACE INTO provider_subscription_accounts VALUES ('p','account','label',?,?,?,?, 'authorized',0)")
            .bind(access).bind(refresh).bind(expires).bind(version).execute(&service.db).await.unwrap();
    }
    fn refreshed() -> Value {
        serde_json::json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600})
    }

    #[tokio::test]
    async fn concurrent_expiry_and_401_refresh_only_rotate_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(refreshed()))
            .expect(1)
            .mount(&server)
            .await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        let (a, b, c) = tokio::join!(
            service.credentials("p"),
            service.credentials("p"),
            service.force_refresh("p", "original")
        );
        assert_eq!(a.unwrap().access_token, "new-access");
        assert_eq!(b.unwrap().access_token, "new-access");
        assert_eq!(c.unwrap().access_token, "new-access");
        assert_eq!(
            crate::crypto::reveal_required(
                Some(&[9; 32]),
                &service.stored("p").await.unwrap().refresh
            )
            .unwrap(),
            "new-refresh"
        );
        server.verify().await;
    }
    #[tokio::test]
    async fn logout_during_refresh_cannot_resurrect_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(refreshed())
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        let task = {
            let service = service.clone();
            tokio::spawn(async move { service.credentials("p").await })
        };
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        service.logout("p").await.unwrap();
        assert!(task.await.unwrap().is_err());
        assert_eq!(service.summary("p").await.unwrap().status, "unconfigured");
    }
    #[tokio::test]
    async fn relogin_during_refresh_discards_old_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(refreshed())
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        let task = {
            let service = service.clone();
            tokio::spawn(async move { service.credentials("p").await })
        };
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        service.logout("p").await.unwrap();
        account(&service, "replacement", now() + 3600).await;
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.credential_version, "replacement");
        assert_eq!(result.access_token, "old-access");
    }
    #[tokio::test]
    async fn temporary_refresh_error_keeps_authorized_state() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_json(serde_json::json!({"error":"temporarily_unavailable"})),
            )
            .mount(&server)
            .await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        assert!(service.credentials("p").await.is_err());
        assert_eq!(service.summary("p").await.unwrap().status, "authorized");
    }
    #[tokio::test]
    async fn invalid_grant_requires_new_login() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"error":"invalid_grant","error_description":"secret-never-expose"}))).mount(&server).await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        let err = service.credentials("p").await.err().unwrap().to_string();
        assert!(!err.contains("secret"));
        assert_eq!(
            service.summary("p").await.unwrap().status,
            "reauth_required"
        );
    }
    #[tokio::test]
    async fn wrong_local_encryption_key_does_not_mark_reauth() {
        let server = MockServer::start().await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        sqlx::query("UPDATE provider_subscription_accounts SET refresh_token_encrypted='invalid'")
            .execute(&service.db)
            .await
            .unwrap();
        assert!(service.credentials("p").await.is_err());
        assert_eq!(service.summary("p").await.unwrap().status, "authorized");
        assert!(server.received_requests().await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn account_change_clears_models_atomically_but_relogin_preserves_them() {
        use base64::Engine;
        let server = MockServer::start().await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        for table in ["provider_models", "upstream_models", "upstream_models_seen"] {
            sqlx::query(&format!("INSERT INTO {table} VALUES ('p','selected')"))
                .execute(&service.db)
                .await
                .unwrap();
        }
        let token = |id: &str| {
            format!(
                "header.{}.signature",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                    serde_json::json!({"https://api.openai.com/auth":{"chatgpt_account_id":id}})
                        .to_string()
                )
            )
        };
        let mut data = refreshed();
        data["id_token"] = token("account").into();
        service.save_login("p", data).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM provider_models")
            .fetch_one(&service.db)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let mut data = refreshed();
        data["id_token"] = token("different-account").into();
        service.save_login("p", data).await.unwrap();
        for table in ["provider_models", "upstream_models", "upstream_models_seen"] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&service.db)
                .await
                .unwrap();
            assert_eq!(count, 0);
        }
    }
    async fn pending(service: &SubscriptionService) {
        *service.sessions.lock().await = Some(LoginSession {
            provider_id: "p".into(),
            id: "session".into(),
            state: "state".into(),
            verifier: "verifier".into(),
            expires_at: now() + 600,
            status: "pending".into(),
            cancel: CancellationToken::new(),
        });
    }
    #[tokio::test]
    async fn denied_authorization_closes_session() {
        let server = MockServer::start().await;
        let service = service(&server).await;
        pending(&service).await;
        assert!(service
            .submit_callback(
                "p",
                "session",
                "http://localhost:1455/auth/callback?state=state&error=access_denied"
            )
            .await
            .is_err());
        assert_eq!(
            service.login_status("p", "session").await.unwrap().status,
            "failed"
        );
    }
    #[tokio::test]
    async fn callback_is_claimed_once_and_cancel_prevents_commit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(refreshed())
                    .set_delay(Duration::from_millis(150)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let service = service(&server).await;
        pending(&service).await;
        let task = {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .submit_callback(
                        "p",
                        "session",
                        "http://localhost:1455/auth/callback?state=state&code=secret-code",
                    )
                    .await
            })
        };
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        assert!(service
            .submit_callback(
                "p",
                "session",
                "http://localhost:1455/auth/callback?state=state&code=secret-code"
            )
            .await
            .is_err());
        service.cancel_login("p", "session").await.unwrap();
        assert!(task.await.unwrap().is_err());
        assert_eq!(
            service.login_status("p", "session").await.unwrap().status,
            "cancelled"
        );
        assert_eq!(service.summary("p").await.unwrap().status, "unconfigured");
        server.verify().await;
    }
    #[tokio::test]
    async fn expired_session_and_wrong_state_never_exchange_tokens() {
        let server = MockServer::start().await;
        let service = service(&server).await;
        pending(&service).await;
        assert!(service
            .submit_callback(
                "p",
                "session",
                "http://localhost:1455/auth/callback?state=wrong&code=code"
            )
            .await
            .is_err());
        assert_eq!(
            service.login_status("p", "session").await.unwrap().status,
            "pending"
        );
        service.sessions.lock().await.as_mut().unwrap().expires_at = now() - 1;
        assert!(service
            .submit_callback(
                "p",
                "session",
                "http://localhost:1455/auth/callback?state=state&code=code"
            )
            .await
            .is_err());
        assert_eq!(
            service.login_status("p", "session").await.unwrap().status,
            "expired"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }
    #[tokio::test]
    async fn old_invalid_grant_does_not_reject_replacement_account() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error":"invalid_grant"}))
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        let task = {
            let service = service.clone();
            tokio::spawn(async move { service.credentials("p").await })
        };
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        account(&service, "replacement", now() + 3600).await;
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.credential_version, "replacement");
        assert_eq!(service.summary("p").await.unwrap().status, "authorized");
    }
    #[tokio::test]
    async fn temporary_listener_handles_pkce_callback_and_global_login_conflict() {
        use base64::Engine;
        let server = MockServer::start().await;
        let mut data = refreshed();
        data["id_token"]=format!("header.{}.signature",base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::json!({"https://api.openai.com/auth":{"chatgpt_account_id":"listener-account"}}).to_string())).into();
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(data))
            .expect(1)
            .mount(&server)
            .await;
        let service = service(&server).await;
        let login = service
            .start_login("p", "http://localhost:10020/providers")
            .await
            .unwrap();
        assert!(service
            .start_login("other", "http://localhost:10020/providers")
            .await
            .is_err());
        let auth = reqwest::Url::parse(&login.authorization_url).unwrap();
        let pairs: HashMap<_, _> = auth.query_pairs().into_owned().collect();
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert!(!pairs.contains_key("code_verifier"));
        assert_eq!(pairs["originator"], "model-bridge");
        let failed =
            reqwest::get("http://127.0.0.1:1455/auth/callback?state=wrong&code=secret-code")
                .await
                .unwrap();
        assert_eq!(failed.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(failed.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert!(!failed.headers().contains_key("location"));
        let body = failed.text().await.unwrap();
        assert!(body.contains("登录失败"));
        assert!(body.contains("可以关闭此页面"));
        assert!(!body.contains("secret-code"));
        let url = format!(
            "http://127.0.0.1:1455/auth/callback?state={}&code=test-code",
            pairs["state"]
        );
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert!(!response.headers().contains_key("location"));
        let body = response.text().await.unwrap();
        assert!(body.contains("登录成功"));
        assert!(body.contains("可以关闭此页面"));
        assert!(!body.contains("test-code"));
        assert_eq!(
            service.summary("p").await.unwrap().account_id.as_deref(),
            Some("listener-account")
        );
        assert_eq!(
            service
                .login_status("p", &login.session_id)
                .await
                .unwrap()
                .status,
            "succeeded"
        );
        assert!(service
            .submit_callback(
                "p",
                &login.session_id,
                &format!(
                    "http://localhost:1455/auth/callback?state={}&code=test-code",
                    pairs["state"]
                )
            )
            .await
            .is_err());
        server.verify().await;
    }
    #[tokio::test]
    async fn nested_refresh_revocation_requires_login() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({"error":{"code":"refresh_token_reused","message":"do-not-expose"}}))).mount(&server).await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        assert!(service.credentials("p").await.is_err());
        assert_eq!(
            service.summary("p").await.unwrap().status,
            "reauth_required"
        );
    }
    #[tokio::test]
    async fn model_request_discards_account_replacement() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"models":[{"slug":"old-account-model"}]}))
                    .set_delay(Duration::from_millis(150)),
            )
            .mount(&server)
            .await;
        let mut service = Arc::try_unwrap(service(&server).await).ok().unwrap();
        service.models_url = server.uri();
        let service = Arc::new(service);
        account(&service, "original", now() + 3600).await;
        let task = {
            let service = service.clone();
            tokio::spawn(async move { service.models("p").await })
        };
        while server.received_requests().await.unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        account(&service, "replacement", now() + 3600).await;
        assert!(task.await.unwrap().is_err());
    }
    #[tokio::test]
    async fn refresh_cannot_switch_account_identity() {
        use base64::Engine;
        let server = MockServer::start().await;
        let mut data = refreshed();
        data["access_token"]=format!("header.{}.signature",base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::json!({"https://api.openai.com/auth":{"chatgpt_account_id":"wrong-account"}}).to_string())).into();
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(data))
            .mount(&server)
            .await;
        let service = service(&server).await;
        account(&service, "original", 0).await;
        assert!(service.credentials("p").await.is_err());
        assert_eq!(service.stored("p").await.unwrap().version, "original");
    }
}
