# Provider 余额查询 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 后台定时探测 provider 上游账户余额（DeepSeek / OpenRouter 适配器），快照落库，Providers 页卡片展示并提供「查询余额」实时刷新。

**Architecture:** `ProviderDef` 新增 `usage: {adapter, params}` 声明绑定哪个内置适配器；`src/admin/balance_svc.rs` 按 adapter 名 match 分发到各家实现（鉴权/endpoint/解析差异封在函数内），输出各家自定义形状的 JSON 载荷存 `provider_balance` 单行快照（失败保留旧值）；`GET /api/admin/providers` 附带 `usage`+`balance`，`POST /providers/{id}/balance/refresh` 实时重探。

**Tech Stack:** Rust (axum/sqlx/reqwest/serde_json, wiremock dev-dep)、Vue 3 + Naive UI（`web/`）、SQLite。

**Scope note:** Trip adapter 的接口契约尚未提供，本计划**不含** trip 实现；注册表模式保证后续追加 = 新增一个 match 分支 + 一个函数 + fixture 测试，届时另起小计划。

## Global Constraints

- 新增 `[bridge]` 配置字段必须 `#[serde(default = "fn")]`（函数默认值，非裸 default），否则旧 toml 解析失败、服务启动即退出。
- 探测失败只覆写 `status`/`error_msg`/`fetched_at`，**必须保留**上次成功的 `data`（有测试钉住）。
- adapter 收到不认识的 param key 一律报错（fail-fast）；`endpoint` 参数必须 http(s)（复用 `is_safe_base_url`）。
- `error_msg` 落库前截断 500 字符；上游请求 timeout 10s（与 fetch-models 一致）。
- 停用（`is_enabled=false`）的 provider 不探测、不展示余额、refresh 返回 400。
- 提交信息风格沿用现有：`feat:` / `fix:` / `docs:` 前缀 + 中文描述。

**参考文件**（动手前先读）：
- 设计稿：`docs/superpowers/specs/2026-08-17-provider-balance-design.md`
- 同构先例：`fetch_models_from_api`（`src/admin/provider_svc.rs:444`）、`probe_upstream_models`（同文件 :779）、drift 嵌入列表（`list_providers` :13）
- 测试先例：`src/router/proxy_route_tests.rs`（内存 SQLite + wiremock 的 AppState 构造）

---

### Task 1: 配置层 — UsageDef + providers.json 预置

**Files:**
- Modify: `src/config.rs`（`ProviderDef` 结构、tests 模块）
- Modify: `providers.json`（deepseek、openrouter 条目）

**Interfaces:**
- Produces: `crate::config::UsageDef { adapter: String, params: serde_json::Map<String, serde_json::Value> }`；`ProviderDef.usage: Option<UsageDef>`（带 `Serialize`，Task 6 的列表响应要用）

- [ ] **Step 1: 写失败测试**

在 `src/config.rs` 的 `mod tests` 末尾追加：

```rust
    #[test]
    fn usage_def_parses_adapter_and_params() {
        let def: ProviderDef = serde_json::from_str(
            r#"{"id":"x","name":"X","usage":{"adapter":"deepseek","params":{"endpoint":"https://gw.example.com/user/balance"}}}"#,
        ).unwrap();
        let usage = def.usage.unwrap();
        assert_eq!(usage.adapter, "deepseek");
        assert_eq!(usage.params["endpoint"], "https://gw.example.com/user/balance");
    }

    #[test]
    fn usage_absent_is_none_and_params_default_empty() {
        let def: ProviderDef = serde_json::from_str(r#"{"id":"x","name":"X"}"#).unwrap();
        assert!(def.usage.is_none());
        let def: ProviderDef =
            serde_json::from_str(r#"{"id":"x","name":"X","usage":{"adapter":"openrouter"}}"#).unwrap();
        assert!(def.usage.unwrap().params.is_empty());
    }

    #[test]
    fn builtin_providers_json_parses_with_usage() {
        // providers.json 编译期内嵌；deepseek/openrouter 已预置 usage 块
        let defs: Vec<ProviderDef> = serde_json::from_str(include_str!("../providers.json")).unwrap();
        let ds = defs.iter().find(|d| d.id == "deepseek").unwrap();
        assert_eq!(ds.usage.as_ref().unwrap().adapter, "deepseek");
        let or = defs.iter().find(|d| d.id == "openrouter").unwrap();
        assert_eq!(or.usage.as_ref().unwrap().adapter, "openrouter");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test config::tests --lib`
Expected: 编译失败（`ProviderDef` 无 `usage` 字段）

- [ ] **Step 3: 最小实现**

`src/config.rs` 顶部 import 改为：

```rust
use serde::{Deserialize, Serialize};
```

`ProviderDef` 定义改为（加一个字段）：

```rust
#[derive(Clone, Debug, Deserialize)]
pub struct ProviderDef {
    pub id: String,
    pub name: String,
    /// 图标（emoji 或图片 URL）
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub channels: Vec<ChannelDef>,
    /// 余额查询适配声明（可选）：adapter 为内置实现名（见 balance_svc），params 为该 adapter 的自定义参数。
    /// 注意 ~/.mb/providers.json 对同 id provider 是整体替换：覆盖时若不带 usage 会一并丢失余额查询。
    #[serde(default)]
    pub usage: Option<UsageDef>,
}

/// 余额查询适配声明。params 接受哪些 key 由各 adapter 自行定义并严格校验（未知 key 报错）。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UsageDef {
    pub adapter: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
}
```

`providers.json`：deepseek 条目加 `"usage": { "adapter": "deepseek" }`，openrouter 条目加 `"usage": { "adapter": "openrouter" }`（与 `channels` 同级）：

```json
  {
    "id": "deepseek",
    "name": "DeepSeek",
    "icon": "deepseek.svg",
    "channels": [
      { "type": "openai_chat", "base_url": "https://api.deepseek.com/v1", "models_endpoint": "https://api.deepseek.com/v1/models" },
      { "type": "openai_responses", "base_url": "https://api.deepseek.com", "models_endpoint": "https://api.deepseek.com/v1/models" },
      { "type": "anthropic", "base_url": "https://api.deepseek.com/anthropic/v1", "models_endpoint": "https://api.deepseek.com/v1/models" }
    ],
    "usage": { "adapter": "deepseek" }
  },
```

```json
  {
    "id": "openrouter",
    "name": "OpenRouter",
    "icon": "openrouter.png",
    "channels": [
      { "type": "openai_chat", "base_url": "https://openrouter.ai/api/v1", "models_endpoint": "https://openrouter.ai/api/v1/models" },
      { "type": "openai_responses", "base_url": "https://openrouter.ai/api/v1", "models_endpoint": "https://openrouter.ai/api/v1/models" },
      { "type": "anthropic", "base_url": "https://openrouter.ai/api/v1", "models_endpoint": "https://openrouter.ai/api/v1/models" }
    ],
    "usage": { "adapter": "openrouter" }
  },
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test config::tests --lib`
Expected: PASS（含既有 bridge 测试，共 5 个）

- [ ] **Step 5: Commit**

```bash
git add src/config.rs providers.json
git commit -m "feat: ProviderDef 增加 usage 余额适配声明，deepseek/openrouter 预置"
```

---

### Task 2: DB 表 provider_balance + BalanceRow

**Files:**
- Modify: `src/db/schema.rs`（`run_migrations` 末尾、新增 tests 模块）
- Modify: `src/db/models.rs`（追加 `BalanceRow`）

**Interfaces:**
- Produces: 表 `provider_balance(provider_id PK, adapter, status, data, error_msg, fetched_at)`；`crate::db::models::BalanceRow`（`FromRow + Serialize`，字段同列名，`data: Option<String>` 为 JSON 文本）

- [ ] **Step 1: 写失败测试**

`src/db/schema.rs` 文件末尾追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrations_idempotent_and_create_provider_balance() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap(); // 幂等：二次执行不报错
        let name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='provider_balance'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(name.as_deref(), Some("provider_balance"));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test db::schema --lib`
Expected: FAIL（断言 None != Some("provider_balance")）

- [ ] **Step 3: 最小实现**

`run_migrations` 中 `upstream_models_seen` 建表之后、`Ok(())` 之前追加：

```rust
    // provider 余额最新快照：定时探测 UPSERT 单行。失败只覆写 status/error_msg/fetched_at，
    // 保留上次成功的 data（上游抖动不清空余额展示）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_balance (
            provider_id TEXT PRIMARY KEY,
            adapter     TEXT NOT NULL,
            status      TEXT NOT NULL,
            data        TEXT,
            error_msg   TEXT,
            fetched_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;
```

`src/db/models.rs` 末尾追加：

```rust
/// provider 余额最新快照（一行一 provider；仅配置了 usage 的 provider 会有行）
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct BalanceRow {
    pub provider_id: String,
    pub adapter: String,
    /// 'ok' | 'error'
    pub status: String,
    /// 最近一次成功探测的 adapter JSON 载荷（文本）；失败时保留旧值
    pub data: Option<String>,
    pub error_msg: Option<String>,
    /// 最近一次探测时间 RFC3339（含失败）
    pub fetched_at: String,
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test db::schema --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/db/schema.rs src/db/models.rs
git commit -m "feat: 新增 provider_balance 余额快照表与 BalanceRow"
```

---

### Task 3: balance_svc 适配器（参数校验 + DeepSeek + OpenRouter）

**Files:**
- Create: `src/admin/balance_svc.rs`
- Modify: `src/admin/mod.rs`（注册模块）

**Interfaces:**
- Consumes: `crate::admin::provider_svc::is_safe_base_url`
- Produces: `pub async fn fetch_balance(client: &reqwest::Client, adapter: &str, api_key: &str, params: &serde_json::Map<String, serde_json::Value>) -> anyhow::Result<serde_json::Value>`（Task 4 的探测循环与 Task 6 的 refresh 端点调用）；载荷契约：deepseek → `{"balance": number, "is_available": bool, "currency": "CNY"}`，openrouter → `{"total_credits": number, "total_usage": number, "currency": "USD"}`

- [ ] **Step 1: 注册模块**

`src/admin/mod.rs` 当前内容确认含 `pub mod provider_svc; pub mod stats_svc;`，追加一行：

```rust
pub mod balance_svc;
```

- [ ] **Step 2: 写失败测试**

创建 `src/admin/balance_svc.rs`，先只放测试（编译会因缺实现而失败）：

```rust
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
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test balance_svc --lib`
Expected: 编译失败（`fetch_balance` 未定义）

- [ ] **Step 4: 实现**

在 `src/admin/balance_svc.rs` 测试模块之前写入实现：

```rust
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
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test balance_svc --lib`
Expected: PASS（6 个）

- [ ] **Step 6: Commit**

```bash
git add src/admin/balance_svc.rs src/admin/mod.rs
git commit -m "feat: 余额 adapter 注册表（deepseek/openrouter）与参数校验"
```

---

### Task 4: 快照落库 + 探测循环 + 单 provider 重探

**Files:**
- Modify: `src/admin/balance_svc.rs`（追加快照读写与探测逻辑、测试）

**Interfaces:**
- Consumes: Task 2 的 `provider_balance` 表 / `BalanceRow`；Task 3 的 `fetch_balance`
- Produces:
  - `pub async fn probe_balances(state: &Arc<AppState>) -> anyhow::Result<()>`（Task 5 后台任务调用）
  - `pub async fn probe_one(state: &Arc<AppState>, def: &ProviderDef, api_key: &str) -> anyhow::Result<BalanceRow>`（Task 6 refresh 端点调用；fetch 失败落 error 行后仍返回该行，仅 DB 错误才 Err）

- [ ] **Step 1: 写失败测试**

`src/admin/balance_svc.rs` 的 tests 模块追加（注意补充 imports：`crate::config::{ProviderDef, UsageDef}`、`crate::db::schema::run_migrations`、`crate::state::AppState`、`sqlx::SqlitePool`、`std::collections::HashMap`、`std::sync::Arc`、`tokio::sync::RwLock`）：

```rust
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
            usage: Some(UsageDef { adapter: adapter.into(), params }),
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
                "is_available": true, "balance": "9.5"
            })))
            .mount(&server)
            .await;
        let def = def_with_usage("deepseek", "deepseek", Some(&format!("{}/user/balance", server.uri())));
        let state = build_state(vec![def.clone()]).await;
        let row = probe_one(&state, &def, "sk-test").await.unwrap();
        assert_eq!(row.status, "ok");
        let data: Value = serde_json::from_str(row.data.as_ref().unwrap()).unwrap();
        assert_eq!(data["balance"], 9.5);
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
                "is_available": true, "balance": "1"
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test balance_svc --lib`
Expected: 编译失败（`upsert_balance_ok` 等未定义）

- [ ] **Step 3: 实现**

在 `balance_svc.rs` 的 fetch_balance 及两个 adapter 之后、tests 模块之前追加：

```rust
use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::ProviderDef;
use crate::db::models::BalanceRow;
use crate::state::AppState;

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
        fetch_balance(&state.client, &usage.adapter, api_key, &usage.params).await
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
```

（`use` 语句合并到文件顶部对应位置，不要重复 import。）

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test balance_svc --lib`
Expected: PASS（10 个）

- [ ] **Step 5: Commit**

```bash
git add src/admin/balance_svc.rs
git commit -m "feat: 余额快照落库与定时探测循环（失败保留旧值）"
```

---

### Task 5: 配置字段 balance_interval_min + 后台任务

**Files:**
- Modify: `src/config.rs`（`BridgeConfig`、默认函数、tests）
- Modify: `src/main.rs`（spawn 探测循环）
- Modify: `model-bridge.toml.example`（[bridge] 注释项）

**Interfaces:**
- Consumes: Task 4 的 `probe_balances`
- Produces: `app_config.bridge.balance_interval_min`（u64，serde 默认 10）

- [ ] **Step 1: 写失败测试**

`src/config.rs` tests 中，把既有 `bridge_defaults_probe_interval_when_absent` 补一条断言、并新增显式值测试（旧测试改后应能体现新字段默认 10）：

```rust
    #[test]
    fn bridge_defaults_probe_interval_when_absent() {
        // 旧配置（无 probe_interval_min / balance_interval_min）应解析成功并取默认
        let cfg: BridgeConfig = toml::from_str("refresh_interval_min = 5\n").unwrap();
        assert_eq!(cfg.refresh_interval_min, 5);
        assert_eq!(cfg.probe_interval_min, 1440);
        assert_eq!(cfg.log_retention_days, 730);
        assert_eq!(cfg.balance_interval_min, 10);
    }

    #[test]
    fn bridge_respects_explicit_balance_interval() {
        let cfg: BridgeConfig =
            toml::from_str("refresh_interval_min = 5\nbalance_interval_min = 3\n").unwrap();
        assert_eq!(cfg.balance_interval_min, 3);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test config::tests --lib`
Expected: 编译失败（`BridgeConfig` 无 `balance_interval_min`）

- [ ] **Step 3: 最小实现**

`BridgeConfig` 追加字段：

```rust
    /// 后台探测 provider 余额的间隔（分钟）。默认 10。
    /// serde default 保证旧配置文件（无此字段）仍可解析。
    #[serde(default = "default_balance_interval_min")]
    pub balance_interval_min: u64,
```

紧挨其他默认函数追加：

```rust
fn default_balance_interval_min() -> u64 {
    10
}
```

`impl Default for AppConfig` 的 `bridge` 初始化里加一行 `balance_interval_min: 10,`。

`src/main.rs`：在 drift probe 的 spawn 块（`probe_upstream_models` 那段）之后追加：

```rust
    // 启动后台 provider 余额探测（独立节奏，默认 10 分钟）。首次 tick 立即触发→启动即播种快照。
    let balance_state = state.clone();
    let balance_interval_min = app_config.bridge.balance_interval_min.max(1);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(balance_interval_min * 60));
        loop {
            interval.tick().await;
            if let Err(e) = admin::balance_svc::probe_balances(&balance_state).await {
                tracing::error!("Scheduled balance probe failed: {}", e);
            }
        }
    });
```

`model-bridge.toml.example` 的 `[bridge]` 段追加：

```toml
# 后台探测 provider 余额的间隔（分钟）。仅探测配置了 usage 且启用的 provider；失败保留旧快照。
balance_interval_min = 10
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test config::tests --lib && cargo check`
Expected: PASS + 编译通过

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/main.rs model-bridge.toml.example
git commit -m "feat: 后台余额探测任务（balance_interval_min 默认 10 分钟）"
```

---

### Task 6: Admin API — 列表附带 balance + refresh 端点

**Files:**
- Modify: `src/db/models.rs`（`BalanceSummary`、`ProviderSummary` 加字段、`From<BalanceRow>`）
- Modify: `src/admin/provider_svc.rs`（`list_providers` 附带 usage+balance；`get_provider_config` 可见性改 `pub(crate)`）
- Modify: `src/router/admin.rs`（`refresh_balance` handler）
- Modify: `src/router/mod.rs`(注册路由)

**Interfaces:**
- Consumes: Task 1 `UsageDef`（已 Serialize）、Task 2 `BalanceRow`、Task 4 `probe_one`/`read_balance_row`
- Produces: `GET /api/admin/providers` 每个 provider 附带 `usage`（定义层配置）与 `balance`（`BalanceSummary`，无行为 null/缺省）；`POST /api/admin/providers/{id}/balance/refresh` 返回 `BalanceSummary`

- [ ] **Step 1: DTO 与列表改造**

`src/db/models.rs` 追加：

```rust
/// 列表响应的余额摘要：data 解析为 JSON 对象（前端按 adapter 渲染）
#[derive(Debug, Clone, Serialize)]
pub struct BalanceSummary {
    pub adapter: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_msg: Option<String>,
    pub fetched_at: String,
}

impl From<BalanceRow> for BalanceSummary {
    fn from(r: BalanceRow) -> Self {
        BalanceSummary {
            adapter: r.adapter,
            status: r.status,
            data: r.data.and_then(|s| serde_json::from_str(&s).ok()),
            error_msg: r.error_msg,
            fetched_at: r.fetched_at,
        }
    }
}
```

`ProviderSummary` 加两个字段（`drift` 之后）：

```rust
    /// 余额查询配置（来自定义层 usage 块；未配置则缺省）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::config::UsageDef>,
    /// 最新余额快照（未探测过则缺省）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<BalanceSummary>,
```

`src/admin/provider_svc.rs::list_providers`：

1. 在 drift 批量载入旁一次性载入余额行（避免 N+1）：

```rust
    let balance_all: Vec<BalanceRow> = sqlx::query_as::<_, BalanceRow>(
        "SELECT provider_id, adapter, status, data, error_msg, fetched_at FROM provider_balance",
    )
    .fetch_all(pool)
    .await?;
    let mut balance_by_prov: HashMap<String, BalanceRow> = HashMap::new();
    for r in balance_all {
        balance_by_prov.insert(r.provider_id.clone(), r);
    }
```

2. 循环内构造 `ProviderSummary` 处补两个字段：

```rust
        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            channels,
            drift,
            usage: def.usage.clone(),
            balance: balance_by_prov.remove(&def.id).map(BalanceSummary::from),
        });
```

3. imports 补 `BalanceRow`、`BalanceSummary`（`crate::db::models::{...}`）。

4. `get_provider_config` 签名改为 `pub(crate) async fn`（Task 6 的 refresh handler 要读 api_key/is_enabled）。

- [ ] **Step 2: refresh handler + 路由**

`src/router/admin.rs` 追加（放在 `model_changes` 之后）：

```rust
/// 实时重探单个 provider 余额并返回最新快照。未配置 usage 或 provider 停用时 400；
/// fetch 失败不是 HTTP 错误——快照行以 status='error' 返回，前端据此展示错误态。
pub async fn refresh_balance(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Some(def) = state.provider_defs.iter().find(|d| d.id == id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "provider not found"})),
        )
            .into_response();
    };
    if def.usage.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "usage adapter not configured"})),
        )
            .into_response();
    }
    let config = provider_svc::get_provider_config(&state.db, &id).await;
    let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
    if !is_enabled {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "provider is disabled"})),
        )
            .into_response();
    }
    let api_key = config.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
    match crate::admin::balance_svc::probe_one(&state, def, &api_key).await {
        Ok(row) => Json(crate::db::models::BalanceSummary::from(row)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
```

`src/router/mod.rs::create_admin_router` 在 `/providers/{id}/refresh` 路由之后追加：

```rust
        .route(
            "/providers/{id}/balance/refresh",
            axum::routing::post(admin::refresh_balance),
        )
```

- [ ] **Step 3: 编译 + 全量测试**

Run: `cargo clippy --all-targets && cargo test`
Expected: 无 warning（新增代码）、全部测试 PASS

- [ ] **Step 4: 手工冒烟（dev 实例）**

Run: `cargo run`（仓库根目录，使用 model-bridge.toml；启动即触发首次余额探测，观察日志中 deepseek/openrouter 探测行）
另开终端：

```bash
curl -s localhost:10020/api/admin/providers | python3 -m json.tool | grep -A8 '"usage"'
# deepseek/openrouter 条目应有 usage + balance（key 已配置时 status 为 ok；未配置 key 则 error + api_key 未配置）
curl -s -X POST localhost:10020/api/admin/providers/openrouter/balance/refresh
# 未配置 usage 的 provider：
curl -s -X POST localhost:10020/api/admin/providers/anthropic/balance/refresh
# Expected: 400 {"error":"usage adapter not configured"}
```

冒烟后 Ctrl+C 停掉 dev 实例。

- [ ] **Step 5: Commit**

```bash
git add src/db/models.rs src/admin/provider_svc.rs src/router/admin.rs src/router/mod.rs
git commit -m "feat: providers 列表附带 usage/balance，新增余额实时 refresh 端点"
```

---

### Task 7: 前端卡片余额行 +「查询余额」按钮

**Files:**
- Modify: `web/src/views/Providers.vue`（template 卡片、script、style）

**Interfaces:**
- Consumes: Task 6 的 `GET /api/admin/providers`（`usage`/`balance` 字段）与 `POST /api/admin/providers/{id}/balance/refresh`（返回 `BalanceSummary`）
- Produces: 卡片余额行渲染；前端按 `balance.adapter` 分发渲染逻辑，未知 adapter 兜底 `—`

- [ ] **Step 1: 类型与状态**

`Providers.vue` script：`ProviderSummary` 接口替换为（其余接口不动）：

```ts
interface Balance { adapter: string; status: string; data?: Record<string, any> | null; error_msg?: string | null; fetched_at: string }
interface UsageDef { adapter: string; params?: Record<string, any> }
interface ProviderSummary { id: string; name: string; icon?: string; is_enabled: boolean; channels: ChannelInfo[]; drift?: DriftSummary; usage?: UsageDef; balance?: Balance | null }
```

`selectedChannel` ref 附近追加：

```ts
const refreshingId = ref<string | null>(null)
```

- [ ] **Step 2: 渲染与请求函数**

`sortedChannels` 函数之后追加：

```ts
// 前端渲染与后端 adapter 注册表一一对应：各家载荷字段不同，按 adapter 分发。
function balanceText(p: ProviderSummary): string {
  const d = p.balance?.data
  if (!d) return ''
  switch (p.balance!.adapter) {
    case 'deepseek': return `¥${Number(d.balance).toFixed(2)}`
    case 'openrouter': return `$${(Number(d.total_credits) - Number(d.total_usage)).toFixed(2)}`
    default: return '—'
  }
}

async function refreshBalance(p: ProviderSummary) {
  refreshingId.value = p.id
  try {
    const res = await fetch(`${API_BASE}/providers/${p.id}/balance/refresh`, { method: 'POST' })
    const body = await res.json()
    if (!res.ok) { message.error(body.error || '查询余额失败'); return }
    p.balance = body
  } catch {
    message.error('查询余额失败')
  } finally {
    refreshingId.value = null
  }
}
```

- [ ] **Step 3: 模板**

卡片内 `card-channels` div（`</div>` 结束处、`</article>` 之前）之后追加：

```html
          <div v-if="p.is_enabled && p.usage" class="card-balance">
            <span
              class="bal-value mono"
              :class="{ err: p.balance?.status === 'error' }"
              :title="p.balance?.status === 'error'
                ? (p.balance?.error_msg || '查询失败')
                : `更新于 ${p.balance?.fetched_at || '-'}`"
            >{{ p.balance ? (balanceText(p) || (p.balance.status === 'error' ? '查询失败' : '—')) : '…' }}</span>
            <button
              class="bal-btn mono"
              :disabled="refreshingId === p.id"
              @click.stop="refreshBalance(p)"
            >{{ refreshingId === p.id ? '查询中…' : '查询余额' }}</button>
          </div>
```

- [ ] **Step 4: 样式**

`/* ---- drift badge + changes modal ---- */` 注释之前追加：

```css
/* ---- balance row ---- */
.card-balance { display: flex; align-items: center; justify-content: space-between; gap: 8px; margin-top: 10px; padding-top: 10px; border-top: 1px dashed var(--mb-divider); }
.bal-value { font-size: 12px; font-weight: 600; color: var(--mb-text-2); }
.bal-value.err { color: var(--mb-text-3); }
.bal-btn { font-size: 10px; font-weight: 600; color: var(--mb-text-3); background: var(--mb-surface-inset); border: 1px solid var(--mb-border); border-radius: 999px; padding: 2px 8px; cursor: pointer; line-height: 1.5; transition: color 0.15s, border-color 0.15s; }
.bal-btn:hover:not(:disabled) { color: var(--mb-text-2); border-color: var(--mb-tint-green); }
.bal-btn:disabled { opacity: 0.6; cursor: default; }
```

- [ ] **Step 5: 构建验证**

Run: `cd web && npm run build`
Expected: vite build 成功。构建产物 `web/dist/` 会被下一次 cargo build 内嵌（本任务不重建二进制）。

- [ ] **Step 6: Commit**

```bash
git add web/src/views/Providers.vue
git commit -m "feat(web): provider 卡片余额行与「查询余额」实时刷新"
```

---

### Task 8: 文档同步 + 整体验证

**Files:**
- Modify: `CLAUDE.md`（Admin API Endpoints、Architecture、Source Structure、Route Table Refresh 段）

**Interfaces:** 无

- [ ] **Step 1: 更新 CLAUDE.md**

四处改动：

1. 「Admin API Endpoints」列表：`GET /api/admin/providers` 行补注 `（含 usage 配置与最新 balance 快照）`；在 `POST .../refresh` 行后新增：

```markdown
- `POST /api/admin/providers/{id}/balance/refresh` — 实时重探单个 provider 余额并返回快照；未配置 usage 或 provider 停用时 400。上游失败不是 HTTP 错误，以 `status="error"` 快照返回
```

2. 「Architecture」→「Key Design Decisions」追加一条：

```markdown
- **Provider balance probing.** `ProviderDef.usage`（providers.json / ~/.mb/providers.json）把 provider 绑定到内置余额 adapter（`balance_svc`，按名 match 分发；鉴权/endpoint/解析差异封在各 adapter 内）。后台任务按 `bridge.balance_interval_min`（默认 10）探测启用的 provider，UPSERT `provider_balance` 单行快照；载荷为 adapter 自定义 JSON，失败保留上次成功值。前端按 adapter 名分发渲染。
```

3. 「Route Table Refresh」段末尾的 bridge 字段说明之后补一句：

```markdown
`balance_interval_min`（余额探测，默认 10）同理。
```

4. 「Source Structure」表新增一行：

```markdown
| `src/admin/balance_svc.rs` | Provider 余额 adapter 注册表（deepseek/openrouter）、快照 UPSERT、定时探测循环 |
```

- [ ] **Step 2: 全量验证**

Run: `cargo clippy --all-targets && cargo test && cd web && npm run build`
Expected: 全部通过

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: CLAUDE.md 同步余额查询架构与端点"
```

- [ ] **Step 4: 部署（需用户确认后执行）**

按既有部署形态（本机 systemd user service）：

```bash
cargo build --release
cp target/release/model-bridge ~/.local/bin/model-bridge
systemctl --user restart model-bridge   # 服务名以 systemctl --user list-units 实际为准
```

注意：前端改动需经 `cargo build --release` 重新内嵌 `web/dist/`；若 dist 未变而仅 Rust 变动可先 `cargo clean -p model-bridge` 防宏缓存（本任务两者都变了，直接构建即可）。部署后用浏览器打开 admin UI 确认卡片余额行与按钮行为。
