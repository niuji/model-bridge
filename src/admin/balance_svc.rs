//! Provider 余额查询：内置 adapter 注册表。各家契约差异（鉴权、endpoint、响应字段）
//! 全部封在各 adapter 函数内；输出的 JSON 载荷形状由 adapter 自定义，是 adapter 与
//! 前端渲染之间的契约，后端不做统一归一化。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Map, Value};
use sqlx::SqlitePool;

use crate::config::{ProviderDef, UsageDef};
use crate::db::models::BalanceRow;
use crate::state::AppState;

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
    usage: &UsageDef,
    api_key: &str,
) -> anyhow::Result<Value> {
    match usage.adapter.as_str() {
        "deepseek" => deepseek_balance(client, api_key, &usage.params).await,
        "openrouter" => openrouter_credits(client, api_key, &usage.params).await,
        "http" => http_balance(client, api_key, &usage.params, usage.result.as_deref()).await,
        _ => anyhow::bail!("unknown usage adapter: {}", usage.adapter),
    }
}

const DEEPSEEK_BALANCE_URL: &str = "https://api.deepseek.com/user/balance";

/// DeepSeek：GET /user/balance，Bearer。上游返回 balance_infos 数组，金额为字符串（CNY）。
/// 载荷：{"is_available": bool, "currency": "CNY", "total_balance": number, "granted_balance": number, "topped_up_balance": number}
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

/// http adapter 可接受的 param key：url（必填）+ headers（可选，值里 {api_key} 占位）。
const HTTP_PARAMS: &[&str] = &["url", "headers"];

/// 声明式 http adapter：GET 只读，url/headers 由 params 声明；上游 2xx 后按 result 点路径
/// 切出余额相关 JSON（缺省 = 整份响应）原样返回，落库即该值。
async fn http_balance(
    client: &reqwest::Client,
    api_key: &str,
    params: &Map<String, Value>,
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
fn extract_by_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
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

fn now_rfc3339() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 成功快照落库（UPSERT）：覆写 data，清空 error_msg。
pub async fn upsert_balance_ok(
    pool: &SqlitePool,
    provider_id: &str,
    adapter: &str,
    data: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO provider_balance (provider_id, adapter, status, data, error_msg, fetched_at)
         VALUES (?, ?, 'ok', ?, NULL, ?)
         ON CONFLICT(provider_id) DO UPDATE SET
           adapter = excluded.adapter, status = 'ok', data = excluded.data,
           error_msg = NULL, fetched_at = excluded.fetched_at",
    )
    .bind(provider_id)
    .bind(adapter)
    .bind(data.to_string())
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// 失败落库：只覆写 status/error_msg/fetched_at，**不动 data**（保留上次成功值）。
pub async fn upsert_balance_error(
    pool: &SqlitePool,
    provider_id: &str,
    adapter: &str,
    error: &str,
) -> anyhow::Result<()> {
    let msg: String = error.chars().take(500).collect();
    sqlx::query(
        "INSERT INTO provider_balance (provider_id, adapter, status, data, error_msg, fetched_at)
         VALUES (?, ?, 'error', NULL, ?, ?)
         ON CONFLICT(provider_id) DO UPDATE SET
           adapter = excluded.adapter, status = 'error',
           error_msg = excluded.error_msg, fetched_at = excluded.fetched_at",
    )
    .bind(provider_id)
    .bind(adapter)
    .bind(msg)
    .bind(now_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn read_balance_row(pool: &SqlitePool, provider_id: &str) -> anyhow::Result<Option<BalanceRow>> {
    let row = sqlx::query_as::<_, BalanceRow>(
        "SELECT provider_id, adapter, status, data, error_msg, fetched_at FROM provider_balance WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// 探测单个 provider 并落库，返回最新快照行。上游/契约失败落 error 行后仍返回该行
/// （供 refresh 端点直接回显）；仅 DB 错误向上抛。
pub async fn probe_one(state: &Arc<AppState>, def: &ProviderDef, api_key: &str) -> anyhow::Result<BalanceRow> {
    let Some(usage) = def.usage.as_ref() else {
        anyhow::bail!("provider '{}' has no usage adapter configured", def.id);
    };
    let result = if api_key.is_empty() {
        Err(anyhow::anyhow!("api_key 未配置"))
    } else {
        fetch_balance(&state.client, usage, api_key).await
    };
    match result {
        Ok(data) => upsert_balance_ok(&state.db, &def.id, &usage.adapter, &data).await?,
        Err(e) => {
            tracing::warn!("balance probe failed for '{}': {}", def.id, e);
            upsert_balance_error(&state.db, &def.id, &usage.adapter, &e.to_string()).await?;
        }
    }
    read_balance_row(&state.db, &def.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("balance row missing after upsert"))
}

/// 一轮余额探测：只探配置了 usage 且 is_enabled 的 provider（停用的跳过、已有行保留）；
/// 单个失败不中断整轮。
pub async fn probe_balances(state: &Arc<AppState>) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct CfgRow {
        provider_id: String,
        is_enabled: bool,
        api_key: String,
    }
    let cfgs: Vec<CfgRow> =
        sqlx::query_as::<_, CfgRow>("SELECT provider_id, is_enabled, api_key FROM provider_config")
            .fetch_all(&state.db)
            .await?;
    let mut enabled: HashMap<String, bool> = HashMap::new();
    let mut keys: HashMap<String, String> = HashMap::new();
    for c in cfgs {
        enabled.insert(c.provider_id.clone(), c.is_enabled);
        keys.insert(c.provider_id, c.api_key);
    }
    for def in &state.provider_defs {
        if def.usage.is_none() {
            continue;
        }
        if !enabled.get(&def.id).copied().unwrap_or(false) {
            continue;
        }
        let api_key = keys.get(&def.id).cloned().unwrap_or_default();
        if let Err(e) = probe_one(state, def, &api_key).await {
            tracing::warn!("balance probe persist failed for '{}': {}", def.id, e);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map, Value};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::{ProviderDef, UsageDef};
    use crate::db::schema::run_migrations;
    use crate::state::AppState;
    use sqlx::SqlitePool;

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
        let data = fetch_balance(
            &client(), &usage_def("deepseek", &params_with_endpoint(&format!("{}/user/balance", server.uri()))), "sk-test",
        ).await.unwrap();
        assert_eq!(data, json!({"is_available": true, "currency": "CNY", "total_balance": 12.34, "granted_balance": 0.0, "topped_up_balance": 12.34}));
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
            &client(), &usage_def("openrouter", &params_with_endpoint(&format!("{}/api/v1/credits", server.uri()))), "or-test",
        ).await.unwrap();
        assert_eq!(data, json!({"total_credits": 100.5, "total_usage": 25.75, "currency": "USD"}));
    }

    #[tokio::test]
    async fn unknown_adapter_rejected() {
        let err = fetch_balance(&client(), &usage_def("nope", &Map::new()), "k").await.unwrap_err();
        assert!(err.to_string().contains("unknown usage adapter"));
    }

    #[tokio::test]
    async fn unknown_param_key_rejected() {
        let mut p = Map::new();
        p.insert("endpiont".into(), json!("https://x.example.com")); // 拼写错误
        let err = fetch_balance(&client(), &usage_def("deepseek", &p), "k").await.unwrap_err();
        assert!(err.to_string().contains("unknown usage param"));
    }

    #[tokio::test]
    async fn non_http_endpoint_rejected() {
        let err = fetch_balance(&client(), &usage_def("deepseek", &params_with_endpoint("file:///etc/passwd")), "k")
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
            &client(), &usage_def("deepseek", &params_with_endpoint(&format!("{}/user/balance", server.uri()))), "k",
        ).await.unwrap_err();
        assert!(err.to_string().contains("HTTP 500"));
    }

    /// 构造带内存 SQLite + provider_defs 的最小 AppState（同 proxy_route_tests 做法）。
    async fn build_state(defs: Vec<ProviderDef>) -> Arc<AppState> {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        Arc::new(AppState {
            openai_chat_routes: Arc::new(RwLock::new(HashMap::new())),
            openai_responses_routes: Arc::new(RwLock::new(HashMap::new())),
            anthropic_routes: Arc::new(RwLock::new(HashMap::new())),
            provider_defs: defs,
            db: pool,
            client: client(),
            api_key_cache: Arc::new(RwLock::new(HashMap::new())),
            encryption_key: None,
            proxy_base_url: "http://test".into(),
        })
    }

    fn def_with_usage(id: &str, adapter: &str, endpoint: Option<&str>) -> ProviderDef {
        let mut params = Map::new();
        if let Some(url) = endpoint {
            params.insert("endpoint".into(), json!(url));
        }
        ProviderDef {
            id: id.into(),
            name: id.into(),
            icon: None,
            channels: vec![],
            usage: Some(UsageDef {
                adapter: adapter.into(),
                params,
                result: None,
                display: None,
            }),
        }
    }

    async fn set_provider_config(state: &AppState, id: &str, enabled: bool, api_key: &str) {
        sqlx::query("INSERT INTO provider_config (provider_id, api_key, is_enabled) VALUES (?, ?, ?)")
            .bind(id).bind(api_key).bind(enabled)
            .execute(&state.db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn error_probe_preserves_previous_data() {
        let state = build_state(vec![]).await;
        upsert_balance_ok(&state.db, "p", "deepseek", &json!({"balance": 1.0})).await.unwrap();
        upsert_balance_error(&state.db, "p", "deepseek", "HTTP 500").await.unwrap();
        let row = read_balance_row(&state.db, "p").await.unwrap().unwrap();
        assert_eq!(row.status, "error");
        assert_eq!(row.error_msg.as_deref(), Some("HTTP 500"));
        // 关键语义：失败保留上次成功的 data
        assert_eq!(row.data.as_deref(), Some(r#"{"balance":1.0}"#));
    }

    #[tokio::test]
    async fn probe_one_success_upserts_ok_row() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "is_available": true,
                "balance_infos": [{
                    "currency": "CNY",
                    "total_balance": "9.50",
                    "granted_balance": "0.00",
                    "topped_up_balance": "9.50"
                }]
            })))
            .mount(&server)
            .await;
        let def = def_with_usage("deepseek", "deepseek", Some(&format!("{}/user/balance", server.uri())));
        let state = build_state(vec![def.clone()]).await;
        let row = probe_one(&state, &def, "sk-test").await.unwrap();
        assert_eq!(row.status, "ok");
        let data: Value = serde_json::from_str(row.data.as_ref().unwrap()).unwrap();
        assert_eq!(data["total_balance"], 9.5);
    }

    #[tokio::test]
    async fn probe_one_without_key_writes_error_row() {
        let def = def_with_usage("deepseek", "deepseek", None);
        let state = build_state(vec![def.clone()]).await;
        let row = probe_one(&state, &def, "").await.unwrap();
        assert_eq!(row.status, "error");
        assert_eq!(row.error_msg.as_deref(), Some("api_key 未配置"));
        assert!(row.data.is_none());
    }

    #[tokio::test]
    async fn probe_balances_skips_disabled_and_unconfigured() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/balance"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "is_available": true,
                "balance_infos": [{
                    "currency": "CNY",
                    "total_balance": "1.00",
                    "granted_balance": "0.00",
                    "topped_up_balance": "1.00"
                }]
            })))
            .expect(1) // 只有启用的那个会真正请求上游
            .mount(&server)
            .await;
        let ep = format!("{}/user/balance", server.uri());
        let state = build_state(vec![
            def_with_usage("on", "deepseek", Some(&ep)),
            def_with_usage("off", "deepseek", Some(&ep)),
            ProviderDef { id: "plain".into(), name: "plain".into(), icon: None, channels: vec![], usage: None },
        ]).await;
        set_provider_config(&state, "on", true, "sk").await;
        set_provider_config(&state, "off", false, "sk").await;
        set_provider_config(&state, "plain", true, "sk").await;

        probe_balances(&state).await.unwrap();

        assert!(read_balance_row(&state.db, "on").await.unwrap().is_some());
        assert!(read_balance_row(&state.db, "off").await.unwrap().is_none());
        assert!(read_balance_row(&state.db, "plain").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn http_adapter_interpolates_key_and_extracts_result() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/balance"))
            .and(header("Authorization", "Bearer trip-key"))
            .and(header("X-Custom", "hello"))
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
