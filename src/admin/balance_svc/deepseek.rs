//! DeepSeek 余额 adapter。

use serde_json::{json, Value};

use super::{check_params, endpoint_param, REQUEST_TIMEOUT};

const DEEPSEEK_BALANCE_URL: &str = "https://api.deepseek.com/user/balance";

/// DeepSeek：GET /user/balance，Bearer。上游返回 balance_infos 数组，金额为字符串（CNY）。
/// 载荷：{"is_available": bool, "currency": "CNY", "total_balance": number, "granted_balance": number, "topped_up_balance": number}
pub(super) async fn deepseek_balance(
    client: &reqwest::Client,
    api_key: &str,
    params: &serde_json::Map<String, Value>,
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
    // 上游返回 balance_infos 数组（金额为字符串）；该接口无官方文档，契约按实测响应。
    let info = body["balance_infos"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| anyhow::anyhow!("missing 'balance_infos' in response"))?;
    fn parse_amount(info: &Value, field: &str) -> anyhow::Result<f64> {
        info[field]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing '{}' in balance_infos", field))?
            .parse()
            .map_err(|_| anyhow::anyhow!("'{}' is not a number", field))
    }
    let total_balance = parse_amount(info, "total_balance")?;
    let granted_balance = parse_amount(info, "granted_balance")?;
    let topped_up_balance = parse_amount(info, "topped_up_balance")?;
    let currency = info["currency"].as_str().unwrap_or("CNY").to_string();
    let is_available = body["is_available"].as_bool().unwrap_or(true);
    Ok(json!({
        "is_available": is_available,
        "currency": currency,
        "total_balance": total_balance,
        "granted_balance": granted_balance,
        "topped_up_balance": topped_up_balance
    }))
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
    async fn deepseek_parses_balance_payload_with_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .and(header("Authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "is_available": true,
                "balance_infos": [{
                    "currency": "CNY",
                    "total_balance": "12.34",
                    "granted_balance": "0.00",
                    "topped_up_balance": "12.34"
                }]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let data = crate::admin::balance_svc::fetch_balance(
            &client(), &usage_def("deepseek", &params_with_endpoint(&format!("{}/user/balance", server.uri()))), "sk-test",
        ).await.unwrap();
        assert_eq!(data, json!({"is_available": true, "currency": "CNY", "total_balance": 12.34, "granted_balance": 0.0, "topped_up_balance": 12.34}));
    }
}
