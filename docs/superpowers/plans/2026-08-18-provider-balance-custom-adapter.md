# Provider 余额 — 用户自定义 adapter（声明式 http + display 模板）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让用户在 `~/.mb/providers.json` 里给任意 provider（如 trip）声明余额查询与展示，无需改代码、无需重编 gateway——后端通用 `http` adapter 按 `result` 切出余额 JSON 落库，前端按 `display` 模板渲染。

**Architecture:** `UsageDef` 扩展 `result`（响应内点路径）与 `display`（前端模板）两个平级字段；`balance_svc::fetch_balance` 签名改为接收 `&UsageDef`，新增 `http` 分发分支 + `extract_by_path` 切片；`Providers.vue` 新增通用 `resolveTemplate` 解析器，`display` 优先于内置 adapter 硬编码渲染。

**Tech Stack:** Rust（axum/sqlx/reqwest/serde_json/wiremock）、Vue 3 + Naive UI + TypeScript（Vite）。

## Global Constraints

- 新增 `UsageDef` 字段必须 `#[serde(default, ...)]`（不能裸 `default`），旧配置缺字段仍可解析——本仓库铁律。
- `http` adapter 的 `url` 必须过 `crate::admin::provider_svc::is_safe_base_url`（拒非 http(s)，防 SSRF）。
- 未知 param key、url 缺失/非 http(s)、result 路径缺失、非 2xx、JSON 解析失败 → 一律 `error` 行 + `error_msg`，保留上次成功 `data`（不改 `upsert_balance_error` 的「失败不动 data」语义）。
- 只读 GET，不做 method/body；`result` 仅对象 `.` 导航，不支持数组索引。
- 每个 commit message 末尾加 `Co-Authored-By: Claude <noreply@anthropic.com>`。
- 测试用 `cargo test <filter>`（本 crate 无 lib target，`--lib` 会报错）。
- 前端改动后须 `cd web && npm run build`；部署时需 `cargo clean -p model-bridge` 再 rebuild（嵌入旧 dist 的坑，见 CLAUDE.md）。

---

### Task 1: `UsageDef` 加 `result` / `display` 字段

**Files:**
- Modify: `src/config.rs:84-90`（`UsageDef` 结构体）
- Test: `src/config.rs` 的 `tests` 模块（新增一个解析测试）

**Interfaces:**
- Produces: `UsageDef { adapter: String, params: Map<String, Value>, result: Option<String>, display: Option<String> }`（`result`/`display` 供 Task 2/3 消费）。

- [ ] **Step 1: 改结构体**

把 `src/config.rs` 的 `UsageDef`（当前 `derive(Clone, Debug, Deserialize, Serialize)`，字段 `adapter` + `params`）替换为：

```rust
/// 余额查询适配声明。params 接受哪些 key 由各 adapter 自行定义并严格校验（未知 key 报错）。
/// result/display 为声明式 http adapter（及未来用户扩展）服务，内置 adapter 忽略。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UsageDef {
    pub adapter: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// 点路径，指向响应里余额相关 JSON 子对象；None = 整份响应。仅 http adapter 消费。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// 前端展示模板（如 "¥{balance}"）；{path} 在落库载荷上做点路径取值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}
```

- [ ] **Step 2: 写失败测试**

在 `src/config.rs` 的 `tests` 模块末尾（`builtin_providers_json_parses_with_usage` 之后、闭合 `}` 之前）加：

```rust
    #[test]
    fn usage_def_parses_result_and_display() {
        let def: ProviderDef = serde_json::from_str(
            r#"{"id":"trip","name":"Trip","usage":{"adapter":"http","params":{"url":"https://gw.example.com/balance"},"result":"data","display":"¥{balance}"}}"#,
        ).unwrap();
        let usage = def.usage.unwrap();
        assert_eq!(usage.adapter, "http");
        assert_eq!(usage.result.as_deref(), Some("data"));
        assert_eq!(usage.display.as_deref(), Some("¥{balance}"));

        // 缺省时两者为 None，旧配置不受影响
        let def: ProviderDef =
            serde_json::from_str(r#"{"id":"x","name":"X","usage":{"adapter":"openrouter"}}"#).unwrap();
        let usage = def.usage.unwrap();
        assert!(usage.result.is_none());
        assert!(usage.display.is_none());
    }
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test config::tests::usage_def_parses_result_and_display`
Expected: FAIL——`UsageDef` 尚无 `result`/`display` 字段（编译错误）。

- [ ] **Step 4: 运行测试确认通过**

（Step 1 已改结构体；若 Step 1 与 Step 3 顺序倒置，改完结构体后重跑同一命令。）
Run: `cargo test config::`
Expected: PASS——含新增测试在内的所有 config 测试通过。已有测试 `usage_absent_is_none_and_params_default_empty`、`usage_def_parses_adapter_and_params` 不受影响。

- [ ] **Step 5: 提交**

```bash
git add src/config.rs
git commit -m "feat(config): UsageDef 加 result/display 字段（声明式 http adapter 用）

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: `balance_svc` 签名重构 + `http` adapter + `result` 切片

**Files:**
- Modify: `src/admin/balance_svc.rs`（`fetch_balance` 签名、`probe_one` 调用点、新增 `http_balance` + `extract_by_path`、测试模块重构 + 新增 http 测试）

**Interfaces:**
- Consumes: Task 1 的 `UsageDef { adapter, params, result, display }`。
- Produces: `fetch_balance(client: &reqwest::Client, usage: &UsageDef, api_key: &str) -> anyhow::Result<Value>`（`http` 分支消费 `usage.result`）。

- [ ] **Step 1: 改导入与 `fetch_balance` 签名**

`src/admin/balance_svc.rs` 顶部把 `use crate::config::ProviderDef;` 改为：

```rust
use crate::config::{ProviderDef, UsageDef};
```

把 `fetch_balance`（当前 `(client, adapter: &str, api_key: &str, params: &Map)`）替换为：

```rust
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
```

- [ ] **Step 2: 新增 `http_balance` + `extract_by_path`**

在 `openrouter_credits` 之后、`now_rfc3339` 之前插入：

```rust
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

/// 按点路径在 JSON 上导航（仅对象 `.`，不支持数组索引），返回命中值引用。
fn extract_by_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = cur.get(seg)?;
    }
    Some(cur)
}
```

- [ ] **Step 3: 改 `probe_one` 调用点**

`probe_one` 中（当前 `fetch_balance(&state.client, &usage.adapter, api_key, &usage.params).await`）替换为：

```rust
        fetch_balance(&state.client, usage, api_key).await
```

- [ ] **Step 4: 重构测试模块既有 `fetch_balance` 调用**

在 `tests` 模块加一个 helper（放在 `client()` 之后）：

```rust
    fn usage_def(adapter: &str, params: &Map<String, Value>) -> UsageDef {
        UsageDef { adapter: adapter.into(), params: params.clone(), result: None, display: None }
    }
```

把 `def_with_usage` 里的 `UsageDef { adapter: adapter.into(), params }` 补成：

```rust
        UsageDef { adapter: adapter.into(), params, result: None, display: None }
```

把下面 6 处既有 `fetch_balance(&client(), "ADAPTER", "KEY", &PARAMS)` 调用改为 `fetch_balance(&client(), &usage_def("ADAPTER", &PARAMS), "KEY")`：

1. `deepseek_parses_balance_payload_with_bearer`：
   `fetch_balance(&client(), "deepseek", "sk-test", &params_with_endpoint(...))` → `fetch_balance(&client(), &usage_def("deepseek", &params_with_endpoint(...)), "sk-test")`
2. `openrouter_parses_credits_payload`：同上，`"openrouter"`/`"or-test"`。
3. `unknown_adapter_rejected`：`fetch_balance(&client(), "nope", "k", &Map::new())` → `fetch_balance(&client(), &usage_def("nope", &Map::new()), "k")`
4. `unknown_param_key_rejected`：`fetch_balance(&client(), "deepseek", "k", &p)` → `fetch_balance(&client(), &usage_def("deepseek", &p), "k")`
5. `non_http_endpoint_rejected`：`fetch_balance(&client(), "deepseek", "k", &params_with_endpoint("file:///etc/passwd"))` → `fetch_balance(&client(), &usage_def("deepseek", &params_with_endpoint("file:///etc/passwd")), "k")`
6. `upstream_non_2xx_is_error`：`fetch_balance(&client(), "deepseek", "k", &params_with_endpoint(...))` → `fetch_balance(&client(), &usage_def("deepseek", &params_with_endpoint(...)), "k")`

- [ ] **Step 5: 新增 http adapter 测试**

在 `tests` 模块末尾（`probe_balances_skips_disabled_and_unconfigured` 之后）加：

```rust
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
```

- [ ] **Step 6: 运行测试**

Run: `cargo test balance_svc::`
Expected: PASS——既有 deepseek/openrouter/probe 测试 + 新增 http 测试全部通过。

- [ ] **Step 7: 提交**

```bash
git add src/admin/balance_svc.rs
git commit -m "feat(balance): 通用 http adapter + result 切片 + fetch_balance 接收 UsageDef

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: 前端 `display` 模板渲染

**Files:**
- Modify: `web/src/views/Providers.vue:307`（`UsageDef` 接口）、`:355-363`（`balanceText`）+ 新增 `resolveTemplate`

**Interfaces:**
- Consumes: Task 1 的 `UsageDef.display`/`result`（经 `GET /api/admin/providers` 的 `usage` 字段序列化到前端）。

- [ ] **Step 1: 扩展接口**

`interface UsageDef { adapter: string; params?: Record<string, any> }` 改为：

```ts
interface UsageDef { adapter: string; params?: Record<string, any>; result?: string; display?: string }
```

- [ ] **Step 2: 改 `balanceText` + 新增 `resolveTemplate`**

把 `balanceText`（当前按 adapter switch，default 返回 `—`）替换为：

```ts
// 前端渲染与后端 adapter 注册表一一对应：各家载荷字段不同，按 adapter 分发。
// usage.display 是通用模板逃生门，优先于内置 adapter 的硬编码渲染。
function balanceText(p: ProviderSummary): string {
  const d = p.balance?.data
  if (!d) return ''
  const display = p.usage?.display
  if (display) return resolveTemplate(display, d)
  switch (p.balance!.adapter) {
    case 'deepseek': return `¥${Number(d.total_balance).toFixed(2)}`
    case 'openrouter': return `$${(Number(d.total_credits) - Number(d.total_usage)).toFixed(2)}`
    default: return '—'
  }
}

// 通用模板解析：{path} 占位符按点路径取 data 值并字符串化；未命中保留原占位符（暴露配置错误）。
function resolveTemplate(tpl: string, data: Record<string, any>): string {
  return tpl.replace(/\{([\w.]+)\}/g, (m, path: string) => {
    const v = path.split('.').reduce((acc: any, k: string) => (acc && typeof acc === 'object' ? acc[k] : undefined), data)
    if (v === undefined || v === null) return m
    if (typeof v === 'object') return JSON.stringify(v)
    return String(v)
  })
}
```

- [ ] **Step 3: 前端构建**

Run: `cd web && npm run build`
Expected: 构建通过、无 TS 报错，产出 `web/dist/`。

- [ ] **Step 4: 提交**

```bash
git add web/src/views/Providers.vue
git commit -m "feat(web): 余额 display 模板渲染（通用 resolveTemplate）

Co-Authored-By: Claude <noreply@anthropic.com>"
```

> 注：`web/dist/` 是构建产物、不入 git；此处只提交 `Providers.vue` 源文件，部署时的 dist 重建见 Task 4 的验证说明。

---

### Task 4: 文档同步与整体验证

**Files:**
- Modify: `CLAUDE.md`（Source Structure 两行、Key Design Decisions 余额 bullet）

- [ ] **Step 1: 同步 Source Structure**

`| `src/admin/balance_svc.rs` | Provider 余额 adapter 注册表（deepseek/openrouter）、快照 UPSERT、单轮探测 |`

改为：

`| `src/admin/balance_svc.rs` | Provider 余额 adapter 注册表（deepseek/openrouter/http）、快照 UPSERT、单轮探测 |`

`| `src/config.rs` | CLI args (clap), TOML config parsing, `providers.json` loading, `ProviderDef`/`ChannelDef` types |`

改为：

`| `src/config.rs` | CLI args (clap), TOML config parsing, `providers.json` loading, `ProviderDef`/`ChannelDef`/`UsageDef` types |`

- [ ] **Step 2: 同步 Key Design Decisions 余额 bullet**

在 `- **Provider balance probing.** … 前端按 adapter 名分发渲染。` 末尾（句号前）追加：

`；用户本地 provider（如 trip）用声明式 `http` adapter（`params.url`/`headers` + `result` 切片）+ `display` 模板接入，见 spec `2026-08-18-provider-balance-custom-adapter.md``

- [ ] **Step 3: 整体验证**

```bash
cargo test          # 全部测试（90+ 现有 + 新增）
cargo clippy        # 仅允许既有的 proxy.rs too_many_arguments 告警，无新增
cd web && npm run build
```

Expected: 测试全绿、clippy 无新增告警、前端构建通过。

- [ ] **Step 4: 提交**

```bash
git add CLAUDE.md
git commit -m "docs: CLAUDE.md 同步 http adapter 与 display 模板

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## 部署（待用户确认，不在计划内自动执行）

机制落地后，`~/.mb/providers.json` 里 trip 的 `usage` 块（`url`/`headers`/`result`/`display`）等用户提供 trip 契约后填值。部署时：`cargo clean -p model-bridge` → `cargo build --release` → 停服务 → 覆盖 `~/.local/bin/model-bridge` → `systemctl --user restart`（前端 dist 已重建，clean 是防 stale embed 的坑）。
