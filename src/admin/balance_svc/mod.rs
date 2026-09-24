//! Provider 余额查询：内置 adapter 注册表。各家契约差异（鉴权、endpoint、响应字段）
//! 全部封在各 adapter 模块内；输出的 JSON 载荷形状由 adapter 自定义，是 adapter 与
//! 前端渲染之间的契约，后端不做统一归一化。

mod anthropic;
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
        "anthropic_cost" => anthropic::monthly_cost(client, api_key, &usage.params).await,
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
pub async fn upsert_balance_ok<'e>(
    pool: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
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

/// 失败只更新错误状态，保留上次成功的数据和更新时间。首次失败仍插入空快照供 UI 显示错误。
pub async fn upsert_balance_error<'e>(
    pool: impl sqlx::Executor<'e, Database = sqlx::Sqlite>,
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
           error_msg = excluded.error_msg",
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
/// （供 refresh 端点直接回显）；DB 错误及查询期间费用配置变更向上抛。
pub async fn probe_one(state: &Arc<AppState>, def: &ProviderDef, api_key: &str) -> anyhow::Result<BalanceRow> {
    anyhow::ensure!(def.access_type == crate::config::AccessType::ApiKey && def.config_error.is_none(),
        "balance is unavailable for this provider configuration");
    let Some(usage) = def.usage.as_ref() else {
        anyhow::bail!("provider '{}' has no usage adapter configured", def.id);
    };
    let mut usage = usage.clone();
    let cost_key;
    let api_key = if usage.adapter == "anthropic_cost" {
        let config = crate::admin::provider_svc::get_provider_config(&state.db, &def.id).await?;
        let workspace_id = config
            .as_ref()
            .map(|c| c.workspace_id.as_str())
            .unwrap_or_default();
        usage
            .params
            .insert("workspace_id".into(), Value::String(workspace_id.into()));
        cost_key = config.map(|c| c.cost_api_key).unwrap_or_default();
        cost_key.as_str()
    } else {
        api_key
    };
    let result = if api_key.is_empty() && usage.adapter == "anthropic_cost" {
        Err(anyhow::anyhow!("未配置费用查询 API Key"))
    } else if api_key.is_empty() {
        Err(anyhow::anyhow!("api_key 未配置"))
    } else {
        fetch_balance(&state.client, &usage, api_key).await
    };
    let mut tx = state.db.begin().await?;
    if usage.adapter == "anthropic_cost" {
        // 配置保存与快照写入在同一 SQLite 事务内互斥，旧请求不能恢复已被清除的费用。
        let current: (String, String) = sqlx::query_as(
            "SELECT workspace_id, cost_api_key FROM provider_api_key_credentials WHERE provider_id = ?",
        )
        .bind(&def.id)
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or_default();
        if current.0 != usage.params["workspace_id"].as_str().unwrap_or_default()
            || current.1 != api_key
        {
            anyhow::bail!("费用配置已变更，请重新查询");
        }
    }
    match result {
        Ok(data) => upsert_balance_ok(&mut *tx, &def.id, &usage.adapter, &data).await?,
        Err(e) => {
            tracing::warn!("balance probe failed for '{}': {}", def.id, e);
            upsert_balance_error(&mut *tx, &def.id, &usage.adapter, &e.to_string()).await?;
        }
    }
    tx.commit().await?;
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
        sqlx::query_as::<_, CfgRow>("SELECT c.provider_id,c.is_enabled,COALESCE(k.api_key,'') AS api_key FROM provider_config c LEFT JOIN provider_api_key_credentials k USING(provider_id)")
            .fetch_all(&state.db)
            .await?;
    let mut enabled: HashMap<String, bool> = HashMap::new();
    let mut keys: HashMap<String, String> = HashMap::new();
    for c in cfgs {
        enabled.insert(c.provider_id.clone(), c.is_enabled);
        keys.insert(c.provider_id, c.api_key);
    }
    for def in &state.provider_defs {
        if def.usage.is_none() || def.access_type != crate::config::AccessType::ApiKey || def.config_error.is_some() {
            continue;
        }
        if !enabled.get(&def.id).copied().unwrap_or(false) {
            continue;
        }
        if def
            .usage
            .as_ref()
            .is_some_and(|u| u.adapter == "anthropic_cost")
        {
            let config =
                crate::admin::provider_svc::get_provider_config(&state.db, &def.id).await?;
            if config.is_none_or(|c| c.cost_api_key.is_empty()) {
                continue;
            }
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
    async fn anthropic_cost_converts_cents_to_dollars() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::header("x-api-key", "admin-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"results": [{"amount": "123.45", "currency": "USD", "workspace_id": null}]}],
                "has_more": false, "next_page": null
            }))).mount(&server).await;
        let data = fetch_balance(
            &client(),
            &usage_def("anthropic_cost", &params_with_endpoint(&server.uri())),
            "admin-key",
        )
        .await
        .unwrap();
        assert_eq!(data["cost_usd"], json!(1.2345));
        assert_eq!(data["kind"], "cost");
        assert_eq!(data["scope"], "organization");
    }

    #[tokio::test]
    async fn anthropic_cost_paginates_and_selects_workspace() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::query_param(
                "group_by[]",
                "workspace_id",
            ))
            .and(wiremock::matchers::query_param("limit", "31"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"results": [
                    {"amount": "100", "currency": "USD", "workspace_id": "wrkspc_a"},
                    {"amount": "900", "currency": "USD", "workspace_id": "wrkspc_other"}
                ]}], "has_more": true, "next_page": "page2"
            })))
            .mount(&server)
            .await;
        Mock::given(wiremock::matchers::query_param("page", "page2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"results": [{"amount": "250.5", "currency": "USD", "workspace_id": "wrkspc_a"}]}],
                "has_more": false, "next_page": null
            }))).with_priority(1).mount(&server).await;
        let mut params = params_with_endpoint(&server.uri());
        params.insert("workspace_id".into(), json!("wrkspc_a"));
        let data = fetch_balance(&client(), &usage_def("anthropic_cost", &params), "admin")
            .await
            .unwrap();
        assert_eq!(data["cost_usd"], json!(3.505));
        assert_eq!(data["scope"], "workspace");
        assert_eq!(data["workspace_id"], "wrkspc_a");
        assert!(data["starting_at"]
            .as_str()
            .unwrap()
            .ends_with("-01T00:00:00Z"));
        let reqs = server.received_requests().await.unwrap();
        assert_eq!(reqs.len(), 2);
        for req in reqs {
            assert_eq!(
                req.url
                    .query_pairs()
                    .find(|(k, _)| k == "starting_at")
                    .unwrap()
                    .1,
                data["starting_at"].as_str().unwrap()
            );
        }
    }

    #[tokio::test]
    async fn anthropic_cost_uses_separate_key_and_retains_snapshot_on_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(wiremock::matchers::header("x-api-key", "admin-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [], "has_more": false, "next_page": null
            })))
            .mount(&server)
            .await;
        let def = def_with_usage("anthropic", "anthropic_cost", Some(&server.uri()));
        let state = build_state(vec![def.clone()]).await;
        set_provider_config(&state, "anthropic", true, "inference-key").await;
        // 未配置管理凭证时自动刷新跳过，手动刷新报清晰错误，绝不拿推理 Key 试探。
        probe_balances(&state).await.unwrap();
        assert!(read_balance_row(&state.db, "anthropic")
            .await
            .unwrap()
            .is_none());
        let row = probe_one(&state, &def, "inference-key").await.unwrap();
        assert_eq!(row.status, "error");
        assert!(server.received_requests().await.unwrap().is_empty());
        sqlx::query("UPDATE provider_api_key_credentials SET cost_api_key = 'admin-key'")
            .execute(&state.db)
            .await
            .unwrap();
        probe_balances(&state).await.unwrap();
        let row = read_balance_row(&state.db, "anthropic")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.status, "ok");
        assert_eq!(
            serde_json::from_str::<Value>(row.data.as_ref().unwrap()).unwrap()["cost_usd"],
            0.0
        );
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let failed = probe_one(&state, &def, "inference-key").await.unwrap();
        assert_eq!(failed.status, "error");
        assert_eq!(failed.data, row.data);
        assert_eq!(failed.fetched_at, row.fetched_at);
    }

    #[tokio::test]
    async fn anthropic_cost_rejects_invalid_amount_and_pagination() {
        for body in [
            json!({"data": [{"results": [{"amount": "NaN", "currency": "USD"}]}], "has_more": false}),
            json!({"data": [{"results": [{"amount": "100", "currency": "CNY"}]}], "has_more": false}),
            json!({"data": [], "has_more": true, "next_page": null}),
            json!({"data": [], "has_more": true, "next_page": "same"}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
            assert!(fetch_balance(
                &client(),
                &usage_def("anthropic_cost", &params_with_endpoint(&server.uri())),
                "admin"
            )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn anthropic_cost_discards_inflight_result_after_settings_change() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(200))
                    .set_body_json(json!({"data": [], "has_more": false, "next_page": null})),
            )
            .mount(&server)
            .await;
        let def = def_with_usage("anthropic", "anthropic_cost", Some(&server.uri()));
        let state = build_state(vec![def.clone()]).await;
        set_provider_config(&state, "anthropic", true, "inference").await;
        sqlx::query("UPDATE provider_api_key_credentials SET cost_api_key = 'admin'")
            .execute(&state.db)
            .await
            .unwrap();
        let probing_state = state.clone();
        let probe = tokio::spawn(async move { probe_one(&probing_state, &def, "inference").await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while server.received_requests().await.unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        crate::admin::provider_svc::update_provider(
            &state.db,
            "anthropic",
            "inference",
            true,
            &[],
            &[],
            &crate::db::models::ProviderSettings {
                workspace_id: Some("wrkspc_new".into()),
                cost_api_key: None,
            },
        )
        .await
        .unwrap();
        assert!(probe.await.unwrap().is_err());
        assert!(read_balance_row(&state.db, "anthropic")
            .await
            .unwrap()
            .is_none());
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
            subscription: std::sync::Arc::new(crate::providers::openai_subscription::SubscriptionService::new(pool.clone(), reqwest::Client::new(), None)),
            admin_base_url: "http://localhost:10020".into(),
            updates: std::sync::Arc::new(crate::update::Manager::default()),
            usage_tasks: tokio_util::task::TaskTracker::new(),
            openai_chat_routes: Arc::new(RwLock::new(HashMap::new())),
            openai_responses_routes: Arc::new(RwLock::new(HashMap::new())),
            anthropic_routes: Arc::new(RwLock::new(HashMap::new())),
            provider_defs: defs,
            db: pool,
            client: client(),
            api_key_cache: Arc::new(RwLock::new(HashMap::new())),
            encryption_key: None,
            request_log_enabled: tokio::sync::RwLock::new(false),
            request_log_dir: std::env::temp_dir().join(format!("mb-request-log-{}", uuid::Uuid::new_v4())),
            proxy_base_url: "http://test".into(),
        })
    }

    fn def_with_usage(id: &str, adapter: &str, endpoint: Option<&str>) -> ProviderDef {
        let mut params = Map::new();
        if let Some(url) = endpoint {
            params.insert("endpoint".into(), json!(url));
        }
        ProviderDef { access_type: crate::config::AccessType::ApiKey, adapter: None,
            id: id.into(),
            name: id.into(),
            icon: None,
            console_url: None,
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
        sqlx::query("INSERT INTO provider_config (provider_id, is_enabled) VALUES (?, ?)")
            .bind(id).bind(enabled)
            .execute(&state.db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO provider_api_key_credentials (provider_id, api_key) VALUES (?, ?)")
            .bind(id).bind(api_key).execute(&state.db).await.unwrap();
    }

    #[tokio::test]
    async fn balance_skips_subscription_residual_credentials_and_invalid_config() {
        let mut subscription = def_with_usage("sub", "deepseek", None);
        subscription.access_type = crate::config::AccessType::Subscription;
        let mut invalid = def_with_usage("bad", "deepseek", None);
        invalid.config_error = Some("invalid".into());
        let state = build_state(vec![subscription.clone(), invalid]).await;
        set_provider_config(&state, "sub", true, "").await;
        set_provider_config(&state, "bad", true, "").await;
        probe_balances(&state).await.unwrap();
        assert!(read_balance_row(&state.db, "sub").await.unwrap().is_none());
        assert!(read_balance_row(&state.db, "bad").await.unwrap().is_none());
        assert!(probe_one(&state, &subscription, "").await.is_err());
    }

    #[tokio::test]
    async fn error_probe_preserves_previous_data_and_timestamp() {
        let state = build_state(vec![]).await;
        upsert_balance_ok(&state.db, "p", "deepseek", &json!({"balance": 1.0})).await.unwrap();
        sqlx::query("UPDATE provider_balance SET fetched_at = '2026-01-01T00:00:00Z' WHERE provider_id = 'p'")
            .execute(&state.db).await.unwrap();
        upsert_balance_error(&state.db, "p", "deepseek", "HTTP 500").await.unwrap();
        let row = read_balance_row(&state.db, "p").await.unwrap().unwrap();
        assert_eq!(row.status, "error");
        assert_eq!(row.error_msg.as_deref(), Some("HTTP 500"));
        assert_eq!(row.fetched_at, "2026-01-01T00:00:00Z");
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
            ProviderDef { access_type: crate::config::AccessType::ApiKey, adapter: None, id: "plain".into(), name: "plain".into(), icon: None, console_url: None, channels: vec![], usage: None, config_error: None },
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
