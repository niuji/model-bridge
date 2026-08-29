//! OpenRouter 余额 adapter。

use serde_json::{json, Value};

use super::{check_params, endpoint_param, REQUEST_TIMEOUT};

const OPENROUTER_CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";

/// OpenRouter：GET /api/v1/credits，Bearer。上游返回包在 data 里（USD credits）。
/// 载荷：{"total_credits": number, "total_usage": number, "currency": "USD"}
pub(super) async fn openrouter_credits(
    client: &reqwest::Client,
    api_key: &str,
    params: &serde_json::Map<String, Value>,
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
    use serde_json::{json, Map, Value};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::UsageDef;

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    fn usage_def(adapter: &str, params: &Map<String, Value>) -> UsageDef {
        UsageDef { adapter: adapter.into(), params: params.clone(), result: None, display: None }
    }

    fn params_with_endpoint(url: &str) -> Map<String, Value> {
        let mut p = Map::new();
        p.insert("endpoint".into(), json!(url));
        p
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
        let data = crate::admin::balance_svc::fetch_balance(
            &client(), &usage_def("openrouter", &params_with_endpoint(&format!("{}/api/v1/credits", server.uri()))), "or-test",
        ).await.unwrap();
        assert_eq!(data, json!({"total_credits": 100.5, "total_usage": 25.75, "currency": "USD"}));
    }
}
