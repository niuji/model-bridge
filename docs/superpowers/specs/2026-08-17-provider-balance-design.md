# Provider 余额查询 — 设计

Date: 2026-08-17
Status: Approved (design, pending spec review)

## 背景与现状

Model Bridge 本地记录经网关转发的用量（`usage_records`），但**上游账户的余额/额度**无从得知。各家 provider 的余额查询方式、接口路径、鉴权形态、响应契约完全不同：DeepSeek 是 `GET /user/balance`（Bearer，返回 `balance` 字符串），OpenRouter 是 `GET /api/v1/credits`（Bearer，返回 `total_credits`/`total_usage`），智谱需要 `id.secret` 派生 JWT，OpenAI/Anthropic 普通 key 根本没有公开余额接口。无法用单一通用解析压平。

## 目标

管理员在 Providers 页看到各 provider 的上游账户余额。后台定时探测、落库、页面读库展示。**不做**消耗趋势、历史存储、余额告警（架构不预留，需要时另设计）。

## 关键设计决策

1. **内置代码适配器注册表**：每家契约差异（鉴权构造、endpoint、响应解析）封在 Rust 代码里，按 adapter 名 match 分发。不做 trait/动态分发（数量少，沿用 `fetch_models_from_api` 的 match 风格），不做纯声明式字段映射（各家差异压不平）。
2. **配置在 JSON 定义层**：`ProviderDef` 新增可选 `usage` 块声明 adapter + 参数。内置 provider 在 `providers.json` 预置；用户态 provider（如 trip）在 `~/.mb/providers.json` 声明。选择定义层而非 DB+UI，因为 adapter 绑定描述的是 provider 的契约属性，且免去新增配置 UI。
3. **参数是通用容器，不是固定 key**：`params` 为任意 JSON 对象，各 adapter 声明自己接受哪些 key（有的接受 `endpoint` 覆盖，有的不接受任何参数）。未知 key 一律报错——配置文件没有 UI 校验，静默忽略会让拼错的参数悄悄失效。
4. **快照载荷是 JSON，不做强归一化**：`provider_balance.data` 存 adapter 整理后的 JSON（不是上游原始响应）。adapter 输出形状即 adapter 与前端之间的契约；后端对 data 内容透明，只校验 JSON 合法。各家字段不一致互不影响，上游加噪声字段不影响前端。
5. **前端按 adapter 分发渲染**：展示哪些字段、货币符号、格式由 adapter 名决定，与后端注册表一一对应；不认识的 adapter 走兜底文案，不崩。
6. **定时探测 + 只存最新快照**：后台任务按间隔探测，UPSERT 单行。余额随消耗变化但无需趋势，天级太迟钝、分钟级太勤，默认 **10 分钟**，可配。
7. **失败保留旧值**：探测失败只覆写 `status`/`error_msg`/`fetched_at`，不清空上次成功的 `data`——上游抖动不应让 UI 上余额凭空消失。

## 配置层

`config.rs` 新增：

```rust
#[derive(Clone, Debug, Deserialize)]
pub struct UsageDef {
    pub adapter: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
}
```

`ProviderDef` 加 `#[serde(default)] pub usage: Option<UsageDef>`（可选字段，旧 JSON 不受影响）。

providers.json 示例：

```json
{
  "id": "deepseek",
  "usage": { "adapter": "deepseek" }
}
```

带参数覆盖：

```json
{
  "usage": { "adapter": "deepseek", "params": { "endpoint": "https://my-gw.example.com/user/balance" } }
}
```

**已知代价（沿用现有语义，不新增合并逻辑）**：`~/.mb/providers.json` 对同 id provider 是整体替换。用户覆盖某 provider 时若不带 `usage`，该 provider 的余额查询一并丢失——与今天覆盖 `channels` 的行为一致。

## 数据模型（新增 1 张表）

```sql
CREATE TABLE provider_balance (
  provider_id TEXT PRIMARY KEY,
  adapter     TEXT NOT NULL,
  status      TEXT NOT NULL,        -- 'ok' | 'error'
  data        TEXT,                 -- 该 adapter 定义的 JSON 载荷（最近一次成功）
  error_msg   TEXT,
  fetched_at  TEXT NOT NULL         -- 最近一次探测时间，RFC3339
);
```

- 只有配置了 `usage` 的 provider 才会有行。
- 探测成功：`status='ok'`，覆写 `data`/`fetched_at`，清空 `error_msg`。
- 探测失败：覆写 `status='error'`/`error_msg`（截断 500 字符，沿用 usage_records 约定）/`fetched_at`，**保留**原 `data`。
- `params` 不落库：它是定义层配置，探测时从 `AppState.provider_defs` 读。

## 适配器（新增 `src/admin/balance_svc.rs`）

统一签名，按名分发：

```rust
async fn fetch(client: &reqwest::Client, api_key: &str, params: &Map<String, Value>)
    -> anyhow::Result<serde_json::Value>;   // 该 adapter 定义的 JSON 载荷

match adapter_name {
    "deepseek"   => deepseek_balance(...),
    "openrouter" => openrouter_credits(...),
    "trip"       => trip_balance(...),
    _            => bail!("unknown adapter"),
}
```

每个 adapter 内部负责三件事：

1. **鉴权构造**：按各家契约写死（Bearer / `x-api-key` / JWT 等）。
2. **请求**：默认 endpoint 写在 adapter 代码内；`params.endpoint`（若该 adapter 声明支持）可覆盖，须过 `is_safe_base_url`（拒非 http(s)，防 SSRF）；timeout 10s，与 fetch-models 一致。
3. **解析整理**：从上游响应提取字段，输出稳定 JSON 载荷。上游非 2xx 视为失败。

API key 用 `provider_config` 里已存的 key（后台任务场景；不同于 fetch-models 的 UI 传 key）。配了 `usage` 但没配 key → 该行 `status='error'`，`error_msg='api_key 未配置'`。

### v1 覆盖范围

| adapter | 上游接口 | 载荷示例 | 状态 |
|---|---|---|---|
| `deepseek` | `GET /user/balance`，Bearer | `{"balance": 10.5, "is_available": true, "currency": "CNY"}` | 实现 |
| `openrouter` | `GET /api/v1/credits`，Bearer | `{"total_credits": 100, "total_usage": 30, "currency": "USD"}` | 实现 |
| `trip` | 待定（契约由用户提供） | 待定 | 占位，契约到位后实现 |

其余 provider（OpenAI/Anthropic/Kimi/SiliconFlow/MiniMax/智谱等）v1 不做；后续按同一注册表模式追加，无需结构变更。载荷具体字段以实现时对官方文档的核实为准，表中所列为当前已知形态。

## 后台任务

`main.rs` 新增余额探测循环，与 drift probe 同构：

- `[bridge] balance_interval_min`，`#[serde(default = "default_balance_interval_min")]` 默认 **10**（本仓库铁律：新字段必须 fn 默认值，否则旧配置解析失败）。运行时 `.max(1)` 钳制。
- 启动即探一次，之后按间隔重复。每轮遍历配置了 `usage` **且 `is_enabled`** 的 provider（停用的不探；已有快照行保留，UI 显示旧值），逐个调 adapter，UPSERT `provider_balance`；单个失败只记该行 error，不中断整轮。

## Admin API

- `GET /api/admin/providers`：每个 provider 附带 `balance` 字段。有行为 `{adapter, status, data, error_msg, fetched_at}`（`data` 为解析后的 JSON 对象），无行为 `null`。与 `drift` 同嵌列表响应，前端一次拿全。
- `POST /api/admin/providers/{id}/balance/refresh`：立即重探单个 provider，返回更新后的快照（同 `balance` 字段形状）。用于充值后即时刷新与 adapter 配置验证。provider 未配置 `usage` 或已停用时返回 400。

provider 对象本身已含定义层字段，`usage` 配置随 `ProviderDef` 序列化自动带出，前端据此区分"未配置"与"已配置暂无快照"。

## 前端（`web/src/views/Providers.vue`）

- 卡片展示余额：按 `balance.adapter` 分发渲染（deepseek → `¥xx.xx`；openrouter → `$xx.xx` credits 形态；字段/格式各 adapter 前端自行定义）。
- 状态：`status='error'` → 余额灰显 + hover 显示 `error_msg`；无 `usage` 配置（`balance === null` 且 defs 无 usage）→ 不显示；前端不认识的 adapter → 兜底文案。
- 余额旁小刷新按钮 → `POST .../balance/refresh`，带 loading 态。

## 错误处理汇总

adapter 报错 / 未知 adapter 名 / 未知 param key / 未配 api_key / endpoint 非 http(s) / 上游非 2xx / JSON 解析失败 → 一律落 `status='error'` + `error_msg`（≤500 字符）+ warn 日志，保留上次成功的 `data`。

## 测试

1. **解析测试（纯函数）**：每 adapter 一份上游响应 fixture → 断言输出载荷字段。Trip 的 fixture 待契约提供后补。
2. **params/分发测试**：未知 adapter 报错；未知 param key 报错；`endpoint` 覆盖生效；非 http(s) endpoint 被拒。
3. **wiremock 流程测试**：mock 上游，验证各 adapter 鉴权头形态、成功 UPSERT、失败时 `status='error'` 且保留上次成功的 `data`（关键语义，必须有测试钉住）。

前端无测试基建不加；admin handler 层与现状一致不单独立项。

## 涉及文件

| 文件 | 变更 |
|---|---|
| `src/config.rs` | `UsageDef` 类型、`ProviderDef.usage` 字段 |
| `providers.json` | deepseek/openrouter 预置 `usage` 块 |
| `src/db/schema.rs` | `provider_balance` 表迁移（单连接事务内） |
| `src/admin/balance_svc.rs` | 新增：注册表分发、各 adapter、探测一轮 |
| `src/main.rs` | 余额探测后台任务、`balance_interval_min` 消费 |
| `src/router/admin.rs` | providers 列表附 `balance`；refresh 端点 |
| `web/src/views/Providers.vue` | 卡片余额渲染（按 adapter 分发）+ 刷新按钮 |
| `~/.mb/providers.json`（运行态，不入 git） | trip 的 `usage` 声明（待契约到位） |
