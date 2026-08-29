//! Provider 余额查询：内置 adapter 注册表。各家契约差异（鉴权、endpoint、响应字段）
//! 全部封在各 adapter 模块内；输出的 JSON 载荷形状由 adapter 自定义，是 adapter 与
//! 前端渲染之间的契约，后端不做统一归一化。

mod bigmodel;
mod deepseek;
mod http;
mod openrouter;
mod volcengine;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sqlx::SqlitePool;

use crate::config::{ProviderDef, UsageDef};
use crate::db::models::BalanceRow;
use crate::state::AppState;

/// 上游请求超时，与 fetch-models 一致。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// adapter 的 param key 白名单校验：配置在 JSON 文件里没有 UI 校验，拼错的 key 必须
/// fail-fast，否则「自定义 endpoint」会静默退回默认值。
fn check_params(params: &serde_json::Map<String, Value>, allowed: &[&str]) -> anyhow::Result<()> {
    for key in params.keys() {
        if !allowed.contains(&key.as_str()) {
            anyhow::bail!("unknown usage param '{}'", key);
        }
    }
    Ok(())
}

/// 读取可选的 endpoint 覆盖参数；未设则用 adapter 默认 URL。非 http(s) 拒绝（SSRF，
/// 与 refresh_routes 的 is_safe_base_url 同口径）。
fn endpoint_param(params: &serde_json::Map<String, Value>, default: &str) -> anyhow::Result<String> {
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
        "deepseek" => deepseek::deepseek_balance(client, api_key, &usage.params).await,
        "openrouter" => openrouter::openrouter_credits(client, api_key, &usage.params).await,
        "bigmodel" => bigmodel::bigmodel_usage(client, api_key, &usage.params).await,
        "http" => http::http_balance(client, api_key, &usage.params, usage.result.as_deref()).await,
        "volcengine" => volcengine::volcengine_usage(client, api_key, &usage.params).await,
        _ => anyhow::bail!("unknown usage adapter: {}", usage.adapter),
    }
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
    use wiremock::matchers::method;
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
            .and(wiremock::matchers::path("/user/balance"))
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
            config_error: None,
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
            .and(wiremock::matchers::path("/user/balance"))
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
            .and(wiremock::matchers::path("/user/balance"))
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
            ProviderDef { id: "plain".into(), name: "plain".into(), icon: None, channels: vec![], usage: None, config_error: None },
        ]).await;
        set_provider_config(&state, "on", true, "sk").await;
        set_provider_config(&state, "off", false, "sk").await;
        set_provider_config(&state, "plain", true, "sk").await;

        probe_balances(&state).await.unwrap();

        assert!(read_balance_row(&state.db, "on").await.unwrap().is_some());
        assert!(read_balance_row(&state.db, "off").await.unwrap().is_none());
        assert!(read_balance_row(&state.db, "plain").await.unwrap().is_none());
    }
}
