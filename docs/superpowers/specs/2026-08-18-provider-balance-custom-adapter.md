# Provider 余额查询 — 用户自定义 adapter（声明式 http + display 模板）

Date: 2026-08-18
Status: Approved (pending spec review)

## 背景与现状

现有余额查询（`balance_svc.rs`）的 adapter 注册表是编译期封闭的：各家契约封在 Rust 代码里，按名 match 分发；前端按 adapter 名硬编码渲染（`Providers.vue`）。deepseek/openrouter 已内置。但 **trip 是用户本地 provider（`~/.mb/providers.json` 配置，不入仓库）**——它的余额接口与展示逻辑既不适合写进开源仓库，也不应要求用户改代码、重编 gateway 才能接入。

需要一种**声明式、用户可扩展**的机制：后端用通用 `http` adapter 按配置请求上游、按 `result` 路径取回余额相关 JSON；前端用配置声明的 `display` 模板渲染。两端都不再依赖编译进二进制的具体 adapter 名。本 spec 取代 `2026-08-17-provider-balance-design.md` 中 trip 的「占位」条目。

## 目标

用户在 `~/.mb/providers.json` 里给任意 provider（如 trip）声明余额查询，无需改代码、无需重编 gateway：

- 后端：通用 `http` adapter，GET 只读，按配置构造请求，按 `result` 路径切出余额相关 JSON 落库。
- 前端：按 `display` 模板通用渲染落库载荷。

## 关键设计决策

1. **声明式 http adapter，不做可执行插件**：trip 契约是「GET + JSON」，声明式足够。也不做「纯声明式逐字段映射」——当初否决的正是把各家差异压平成字段映射；这里只按 `result` 路径**切出余额相关的整段 JSON**，保留其原始结构，不逐字段提取。
2. **`result` 定位余额子对象，而非整份响应**：上游常包一层信封（`code`/`data`），整份落库会把无关字段带进展示。`result` 是点路径，从响应根定位到「和余额相关的那段 JSON」，落这一段；缺省 = 整份响应。
3. **`display` 模板渲染，前端不硬编码**：展示格式由配置声明，`{path}` 占位符在落库载荷上做点路径取值。SPA 只内置一个通用解析器，不新增 per-adapter 渲染分支。
4. **四个正交字段平级**：`adapter`（选哪个适配器）、`params`（请求构造）、`result`（响应切哪段）、`display`（前端怎么渲染）互不隶属，都在 `UsageDef` 顶层。
5. **错误 fail-fast**：未知 param key、`result` 路径缺失、url 非 http(s)、非 2xx、JSON 解析失败 → 落 `error` 行 + `error_msg`，保留上次成功 `data`（沿用现有语义）。

## 配置层

`config.rs` 的 `UsageDef` 扩展为四字段：

```rust
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UsageDef {
    pub adapter: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// 点路径，指向余额相关 JSON 子对象；None = 整份响应。仅 http adapter 消费，内置 adapter 忽略。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// 前端展示模板，如 "¥{balance}"；{path} 在落库载荷上做点路径取值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}
```

`result`/`display` 均 `Option` + `#[serde(default)]`，旧配置（deepseek/openrouter 只写 `adapter`）不受影响。

trip 配置示例（`~/.mb/providers.json`，值等契约到位后填）：

```json
{
  "id": "trip",
  "usage": {
    "adapter": "http",
    "params": {
      "url": "https://<trip-balance-endpoint>",
      "headers": { "Authorization": "Bearer {api_key}" }
    },
    "result": "data",
    "display": "¥{balance}"
  }
}
```

## 后端：http adapter（`balance_svc.rs`）

`fetch_balance` 签名从 `(adapter, params)` 改为接收 `&UsageDef`（`result` 是 http 专属，但取用统一在分发层）：

```rust
pub async fn fetch_balance(client: &reqwest::Client, usage: &UsageDef, api_key: &str)
    -> anyhow::Result<Value> {
    match usage.adapter.as_str() {
        "deepseek"   => deepseek_balance(client, api_key, &usage.params).await,
        "openrouter" => openrouter_credits(client, api_key, &usage.params).await,
        "http"       => http_balance(client, api_key, &usage.params, usage.result.as_deref()).await,
        _            => anyhow::bail!("unknown usage adapter: {}", usage.adapter),
    }
}
```

`http_balance`：

1. `check_params(params, &["url", "headers"])`（未知 key fail-fast）。
2. `url` 必填，过 `is_safe_base_url`（http(s)，防 SSRF）。
3. `headers` 可选，值里的 `{api_key}` 替换为 `provider_config` 存的 key。
4. GET（只读，不做 method/body），timeout 10s，与 fetch-models 一致。
5. 非 2xx → 报错。
6. `resp.json()` → 若 `result` 提供则按点路径切子对象（对象 `.` 导航，不支持数组索引；路径缺失报错），否则整份。返回值即落库 `data`。

## 前端：display 模板（`Providers.vue`）

- `UsageDef` 前端接口加 `display?: string`、`result?: string`。
- 新增通用模板解析器 `resolveTemplate(tpl, data)`：`{path}` 占位符按点路径取 `data` 值并字符串化（string 原样、number `String()`、bool `true/false`、object `JSON.stringify`）；未命中路径保留 `{path}` 字面量（暴露配置错误）。

渲染优先级（`balanceText`）：

1. `usage.display` 存在 → 解析模板（适配任意 adapter）。
2. `adapter == "deepseek"` → 现有硬编码。
3. `adapter == "openrouter"` → 现有硬编码。
4. 否则 → `—`。

## 错误处理汇总

未知 param key / url 缺失 / url 非 http(s) / result 路径缺失 / 非 2xx / JSON 解析失败 → `status='error'` + `error_msg`（≤500 字符）+ warn 日志，保留上次成功 `data`。

## 测试

- `http` adapter wiremock 测试：`{api_key}` 插值；自定义 header；`result` 切出嵌套子对象且**原样落库**（不扁平化）；无 `result` 落整份；`result` 路径缺失报错；url 必填 / 非 http(s) / 未知 param 拒绝；非 2xx。
- `fetch_balance` 分发：未知 adapter 报错（现有测试更新签名）。
- 前端模板解析器无测试基建，不单独立项（记 Minor）。

## 涉及文件

| 文件 | 变更 |
|---|---|
| `src/config.rs` | `UsageDef` 加 `result`/`display` 字段 |
| `src/admin/balance_svc.rs` | `fetch_balance` 接收 `&UsageDef`；`http` 分支 + `http_balance`；测试更新/新增 |
| `web/src/views/Providers.vue` | `display`/`result` 接口字段 + 模板解析器 + 渲染优先级 |
| `~/.mb/providers.json`（运行态，不入 git） | trip 的 `usage` 块（契约到位后填值） |

## v1 范围与已知限制

- **只读 GET**：不做 POST/body。
- **`result` 不支持数组索引**（`balance_infos.0` 之类）：deepseek 已内置，trip 平铺对象即可，需要时再扩。
- **`display` 无格式化**：数值原样 `String()`，不做 toFixed/货币映射；货币符号由模板字面量写死（如 `¥{balance}`）。
- **trip 实际契约值待定**：本期只做机制 + mock 测试钉住；`~/.mb` 里 trip 的 `url`/`headers`/`result`/`display` 等用户提供契约后填。
