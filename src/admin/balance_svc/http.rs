//! 声明式 http adapter：GET 只读探测，url/headers/结果路径全由 params 声明。

use serde_json::Value;

use super::{check_params, REQUEST_TIMEOUT};

/// http adapter 可接受的 param key：url（必填）+ headers（可选，值里 {api_key} 占位）。
const HTTP_PARAMS: &[&str] = &["url", "headers"];

/// 声明式 http adapter：GET 只读，url/headers 由 params 声明；上游 2xx 后按 result 点路径
/// 切出余额相关 JSON（缺省 = 整份响应）原样返回，落库即该值。
pub(super) async fn http_balance(
    client: &reqwest::Client,
    api_key: &str,
    params: &serde_json::Map<String, Value>,
    result: Option<&str>,
) -> anyhow::Result<Value> {
    check_params(params, HTTP_PARAMS)?;
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("'url' is required"))?;
    if !crate::admin::provider_svc::is_safe_base_url(url) {
        anyhow::bail!("url must be http(s): {}", url);
    }
    let mut req = client.get(url);
    if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
        for (name, value) in headers {
            let value = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("header '{}' must be a string", name))?;
            req = req.header(name.as_str(), value.replace("{api_key}", api_key));
        }
    }
    let resp = req.timeout(REQUEST_TIMEOUT).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }
    let body: Value = resp.json().await?;
    match result {
        None => Ok(body),
        Some(path) => extract_by_path(&body, path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("result path '{}' not found in response", path)),
    }
}

/// 按点路径在 JSON 上导航（对象 `.` + 数字段索引数组，如 `balance_infos.0`），返回命中值引用。
pub(super) fn extract_by_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = match cur {
            // 数组节点按数字段索引（与前端 resolvePath 的 acc[k] 同口径）；越界/非数字 → None
            Value::Array(arr) => arr.get(seg.parse::<usize>().ok()?)?,
            _ => cur.get(seg)?,
        };
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map, Value};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::admin::balance_svc::fetch_balance;
    use crate::config::UsageDef;

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[tokio::test]
    async fn http_adapter_interpolates_key_and_extracts_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .and(wiremock::matchers::header("Authorization", "Bearer trip-key"))
            .and(wiremock::matchers::header("X-Custom", "hello"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0,
                "data": { "balance": "10.50", "currency": "CNY" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mut params = Map::new();
        params.insert("url".into(), json!(format!("{}/balance", server.uri())));
        let mut headers = Map::new();
        headers.insert("Authorization".into(), json!("Bearer {api_key}"));
        headers.insert("X-Custom".into(), json!("hello"));
        params.insert("headers".into(), Value::Object(headers));
        let usage = UsageDef {
            adapter: "http".into(),
            params,
            result: Some("data".into()),
            display: Some("¥{balance}".into()),
        };
        let data = fetch_balance(&client(), &usage, "trip-key").await.unwrap();
        // 切出 data 子对象且原样保留结构，不扁平化
        assert_eq!(data, json!({"balance": "10.50", "currency": "CNY"}));
    }

    #[tokio::test]
    async fn http_adapter_without_result_returns_whole_body() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"balance": 1.0})))
            .mount(&server)
            .await;
        let mut params = Map::new();
        params.insert("url".into(), json!(format!("{}/balance", server.uri())));
        let usage = UsageDef { adapter: "http".into(), params, result: None, display: None };
        let data = fetch_balance(&client(), &usage, "k").await.unwrap();
        assert_eq!(data, json!({"balance": 1.0}));
    }

    #[tokio::test]
    async fn http_adapter_extracts_array_element_by_numeric_path_segment() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "balance_infos": [
                    { "currency": "CNY", "total_balance": "10.34" },
                    { "currency": "USD", "total_balance": "99.00" }
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mut params = Map::new();
        params.insert("url".into(), json!(format!("{}/balance", server.uri())));
        let usage = UsageDef {
            adapter: "http".into(),
            params,
            result: Some("balance_infos.0".into()),
            display: Some("¥{total_balance}".into()),
        };
        let data = fetch_balance(&client(), &usage, "k").await.unwrap();
        assert_eq!(data, json!({ "currency": "CNY", "total_balance": "10.34" }));
    }

    #[tokio::test]
    async fn http_adapter_missing_result_path_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .mount(&server)
            .await;
        let mut params = Map::new();
        params.insert("url".into(), json!(format!("{}/balance", server.uri())));
        let usage = UsageDef { adapter: "http".into(), params, result: Some("nope.deep".into()), display: None };
        let err = fetch_balance(&client(), &usage, "k").await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn http_adapter_requires_url() {
        let usage = UsageDef { adapter: "http".into(), params: Map::new(), result: None, display: None };
        let err = fetch_balance(&client(), &usage, "k").await.unwrap_err();
        assert!(err.to_string().contains("'url' is required"));
    }

    #[tokio::test]
    async fn http_adapter_rejects_non_http_url() {
        let mut params = Map::new();
        params.insert("url".into(), json!("file:///etc/passwd"));
        let usage = UsageDef { adapter: "http".into(), params, result: None, display: None };
        let err = fetch_balance(&client(), &usage, "k").await.unwrap_err();
        assert!(err.to_string().contains("http(s)"));
    }

    #[tokio::test]
    async fn http_adapter_rejects_unknown_param() {
        let mut params = Map::new();
        params.insert("url".into(), json!("https://x.example.com"));
        params.insert("method".into(), json!("POST"));
        let usage = UsageDef { adapter: "http".into(), params, result: None, display: None };
        let err = fetch_balance(&client(), &usage, "k").await.unwrap_err();
        assert!(err.to_string().contains("unknown usage param"));
    }

    #[tokio::test]
    async fn http_adapter_non_2xx_is_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let mut params = Map::new();
        params.insert("url".into(), json!(format!("{}/balance", server.uri())));
        let usage = UsageDef { adapter: "http".into(), params, result: None, display: None };
        let err = fetch_balance(&client(), &usage, "k").await.unwrap_err();
        assert!(err.to_string().contains("HTTP 500"));
    }
}
