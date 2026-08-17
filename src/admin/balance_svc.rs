//! Provider 余额查询：内置 adapter 注册表。各家契约差异（鉴权、endpoint、响应字段）
//! 全部封在各 adapter 函数内；输出的 JSON 载荷形状由 adapter 自定义，是 adapter 与
//! 前端渲染之间的契约，后端不做统一归一化。

use std::time::Duration;

use serde_json::{json, Map, Value};

/// 上游请求超时，与 fetch-models 一致。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// adapter 的 param key 白名单校验：配置在 JSON 文件里没有 UI 校验，拼错的 key 必须
/// fail-fast，否则「自定义 endpoint」会静默退回默认值。
fn check_params(params: &Map<String, Value>, allowed: &[&str]) -> anyhow::Result<()> {
    for key in params.keys() {
        if !allowed.contains(&key.as_str()) {
            anyhow::bail!("unknown usage param '{}'", key);
        }
    }
    Ok(())
}

/// 读取可选的 endpoint 覆盖参数；未设则用 adapter 默认 URL。非 http(s) 拒绝（SSRF，
/// 与 refresh_routes 的 is_safe_base_url 同口径）。
fn endpoint_param(params: &Map<String, Value>, default: &str) -> anyhow::Result<String> {
    match params.get("endpoint") {
        Some(v) => {
            let url = v
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("'endpoint' must be a string"))?;
            if !crate::admin::provider_svc::is_safe_base_url(url) {
                anyhow::bail!("endpoint must be http(s): {}", url);
            }
            Ok(url.to_string())
        }
        None => Ok(default.to_string()),
    }
}

/// 按 adapter 名分发余额查询，返回该 adapter 定义的 JSON 载荷。
pub async fn fetch_balance(
    client: &reqwest::Client,
    adapter: &str,
    api_key: &str,
    params: &Map<String, Value>,
) -> anyhow::Result<Value> {
    match adapter {
        "deepseek" => deepseek_balance(client, api_key, params).await,
        "openrouter" => openrouter_credits(client, api_key, params).await,
        _ => anyhow::bail!("unknown usage adapter: {}", adapter),
    }
}

const DEEPSEEK_BALANCE_URL: &str = "https://api.deepseek.com/user/balance";

/// DeepSeek：GET /user/balance，Bearer。上游返回 balance 为字符串（CNY）。
/// 载荷：{"balance": number, "is_available": bool, "currency": "CNY"}
async fn deepseek_balance(
    client: &reqwest::Client,
    api_key: &str,
    params: &Map<String, Value>,
) -> anyhow::Result<Value> {
    check_params(params, &["endpoint"])?;
    let url = endpoint_param(params, DEEPSEEK_BALANCE_URL)?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let body: Value = resp.json().await?;
    let balance: f64 = body["balance"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing 'balance' in response"))?
        .parse()
        .map_err(|_| anyhow::anyhow!("'balance' is not a number"))?;
    let is_available = body["is_available"].as_bool().unwrap_or(true);
    Ok(json!({ "balance": balance, "is_available": is_available, "currency": "CNY" }))
}

const OPENROUTER_CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";

/// OpenRouter：GET /api/v1/credits，Bearer。上游返回包在 data 里（USD credits）。
/// 载荷：{"total_credits": number, "total_usage": number, "currency": "USD"}
async fn openrouter_credits(
    client: &reqwest::Client,
    api_key: &str,
    params: &Map<String, Value>,
) -> anyhow::Result<Value> {
    check_params(params, &["endpoint"])?;
    let url = endpoint_param(params, OPENROUTER_CREDITS_URL)?;
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let body: Value = resp.json().await?;
    let data = &body["data"];
    let total_credits = data["total_credits"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("missing 'data.total_credits' in response"))?;
    let total_usage = data["total_usage"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("missing 'data.total_usage' in response"))?;
    Ok(json!({ "total_credits": total_credits, "total_usage": total_usage, "currency": "USD" }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map, Value};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    fn params_with_endpoint(url: &str) -> Map<String, Value> {
        let mut p = Map::new();
        p.insert("endpoint".into(), json!(url));
        p
    }

    #[tokio::test]
    async fn deepseek_parses_balance_payload_with_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "is_available": true,
                "balance": "12.34"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let data = fetch_balance(
            &client(), "deepseek", "sk-test",
            &params_with_endpoint(&format!("{}/user/balance", server.uri())),
        ).await.unwrap();
        assert_eq!(data, json!({"balance": 12.34, "is_available": true, "currency": "CNY"}));
    }

    #[tokio::test]
    async fn openrouter_parses_credits_payload() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/credits"))
            .and(header("Authorization", "Bearer or-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "total_credits": 100.5, "total_usage": 25.75 }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let data = fetch_balance(
            &client(), "openrouter", "or-test",
            &params_with_endpoint(&format!("{}/api/v1/credits", server.uri())),
        ).await.unwrap();
        assert_eq!(data, json!({"total_credits": 100.5, "total_usage": 25.75, "currency": "USD"}));
    }

    #[tokio::test]
    async fn unknown_adapter_rejected() {
        let err = fetch_balance(&client(), "nope", "k", &Map::new()).await.unwrap_err();
        assert!(err.to_string().contains("unknown usage adapter"));
    }

    #[tokio::test]
    async fn unknown_param_key_rejected() {
        let mut p = Map::new();
        p.insert("endpiont".into(), json!("https://x.example.com")); // 拼写错误
        let err = fetch_balance(&client(), "deepseek", "k", &p).await.unwrap_err();
        assert!(err.to_string().contains("unknown usage param"));
    }

    #[tokio::test]
    async fn non_http_endpoint_rejected() {
        let err = fetch_balance(&client(), "deepseek", "k", &params_with_endpoint("file:///etc/passwd"))
            .await.unwrap_err();
        assert!(err.to_string().contains("http(s)"));
    }

    #[tokio::test]
    async fn upstream_non_2xx_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let err = fetch_balance(
            &client(), "deepseek", "k",
            &params_with_endpoint(&format!("{}/user/balance", server.uri())),
        ).await.unwrap_err();
        assert!(err.to_string().contains("HTTP 500"));
    }
}
