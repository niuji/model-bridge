# 同名模型多 Provider 路由（provider/model 限定名）

日期：2026-08-01

## 背景

网关允许多个 provider 声明同名模型（如 DeepSeek 和 OpenRouter 都提供 `deepseek-chat`）。现状：

- 路由表是 `HashMap<lowercased_model_id, ProviderRoute>`，同名冲突时 **OpenAI 端点保留先入者 + warn**（`provider_svc.rs:225-236`），**Anthropic 端点静默覆盖**（`provider_svc.rs:194`）。
- `/v1/models` 列表用路由表 key（即裸模型名）下发，同名只出现一次。
- 客户端无法指定「同名模型用哪个 provider」——请求体只有 `model` 字段，路由结果取决于 provider 注册顺序。

**目标**：客户端通过**模型名本身**选择 provider。`/v1/models` 下发两个形态——裸名（全景）与 `provider/model` 限定名（精确入口），请求时用限定名精确路由。

## 决策

1. **限定名格式**：`{provider_id}/{model_id}`（OpenRouter 风格），如 `deepseek/deepseek-chat`。provider_id 用 `providers.json` 里的 `id`（小写）。
2. **无前缀回退**：裸名（如 `deepseek-chat`）沿用现有先到先得逻辑，不做 404 也不做负载均衡。
3. **列表暴露**：`/v1/models` 同时下发裸名 + 限定名，客户端一次拿到两种形态。
4. **Anthropic 一致性修复**：Anthropic 端点的裸名冲突从「静默覆盖」改为「保留先入者 + warn」，与 OpenAI 端点对齐。
5. **不做**：`mb-` key 维度的分流、请求级 fallback、round-robin 负载均衡。

## 架构

**不改路由表数据结构，只在 key 上做文章**。每张路由表（chat / responses / anthropic）的每个模型，从「1 个 key」变成「最多 2 个 key」：

- **裸名 key**（`deepseek-chat`）→ 先到先得回退路由
- **限定名 key**（`deepseek/deepseek-chat`）→ 精确路由

两个 key 指向**同一个 `ProviderRoute` 实例**（clone），无重复数据。

### 数据流

```
客户端 /v1/models
  → 裸名 deepseek-chat          （先到先得，回退路由）
  → 限定名 deepseek/deepseek-chat（精确路由，忽略 provider 注册顺序）

客户端请求 model="deepseek/deepseek-chat"
  → proxy.rs 查 key = "deepseek/deepseek-chat"
  → 命中限定名 key → 路由到 DeepSeek 的 base_url + api_key
  → replace_model_in_body 用 route.model_id（干净名 "deepseek-chat"）回写
```

## 改动点

### 1. `refresh_routes()`（`src/admin/provider_svc.rs:177-238`）

**OpenAI 通道**（`:215-237`）：每个 model 除裸名 key 外，多插 `format!("{}/{}", def.id, model.model_id.to_lowercase())`。

- 裸名 key 冲突 → 保留先入者 + warn（现有行为不变）
- 限定名 key 冲突 → `def.id` 在 provider_defs 唯一，不可能冲突，直接 `insert`

**Anthropic 通道**（`:177-196`）：每个 model 除现有检索 key（剥 `[1m]`、补 `claude-` 前缀）外，多插 `{provider_id}/{检索key}`。

- 裸名 key 冲突 → 改为**保留先入者 + warn**（修复静默覆盖，`:194`）

### 2. 查找逻辑（`src/router/proxy.rs:199`）

**基本不动**。请求模型名直接 `to_lowercase()` 后查 key：

- `deepseek-chat` → 命中裸名 key
- `deepseek/deepseek-chat` → 命中限定名 key

天然兼容，无需在查找端拆字符串。

### 3. `/v1/models` 列表（`src/router/models_list.rs`）

当前用路由表 key 直接作为模型 id（`openai_models_list` `:17`，`anthropic_models_list` 对应逻辑）。改为**遍历时按 route 去重，同时暴露裸名 + 限定名**：

- 裸名 id：`deepseek-chat`
- 限定名 id：`deepseek/deepseek-chat`

同一 `route`（同一个 provider 模型）产生两条列表项。排序后去重逻辑需相应调整——不再是按 `id` 去重（否则两条 id 不同都会保留），而是**按 route 身份去重**（同一 `provider_id` + `model_id` 只发一条裸名 + 一条限定名）。

Anthropic 列表的 `[1m]` 后缀补回逻辑（`get_anthropic_models`）需保持：限定名 id 的后缀补回要放在 provider 前缀之后，即 `{provider_id}/{key}{suffix}`。

### 4. 上游名回写（`src/router/proxy.rs:261-271`）

**不受影响**。`replace_model_in_body` 用 `route.model_id`（干净 model_id，不含 provider 前缀）。不管请求命中裸名还是限定名 key，`route` 都是同一个，回写都是干净名。

### 5. 测试（`src/router/proxy_route_tests.rs`）

补用例：

- 两个 provider 声明同名模型 → 裸名请求命中先注册者
- 两个 provider 声明同名模型 → `{provider_id}/{model}` 请求精确命中指定 provider（验证上游收到的 base_url + api_key）
- 限定名请求的上游 body 中 `model` 被回写为干净 model_id（不带 provider 前缀）
- `/v1/models` 列表同时含裸名 + 限定名
- Anthropic 端点同名冲突保留先入者（如已覆盖该路径）

## 影响范围

- `src/admin/provider_svc.rs` — `refresh_routes()` 插入逻辑
- `src/router/models_list.rs` — 列表去重与 id 生成
- `src/router/proxy.rs` — 查找逻辑（预期不动，仅验证）
- `src/router/proxy_route_tests.rs` — 补用例

## 验证

- `cargo test`（含新用例）
- `cargo clippy`
- 手动：admin 配置两个 provider 同名模型 → 裸名 / 限定名请求各自路由正确；`/v1/models` 列表含两种形态
