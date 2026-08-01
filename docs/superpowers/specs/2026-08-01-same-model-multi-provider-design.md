# 同名模型多 Provider 路由（provider/model 限定名）

日期：2026-08-01

## 背景

网关允许多个 provider 声明同名模型（如 DeepSeek 和 OpenRouter 都提供 `deepseek-chat`）。现状：

- 路由表是 `HashMap<lowercased_model_id, ProviderRoute>`，同名冲突时 **OpenAI 端点保留先入者 + warn**（`provider_svc.rs:225-236`），**Anthropic 端点静默覆盖**（`provider_svc.rs:194`）。
- `/v1/models` 列表用路由表 key（即裸模型名）下发，同名只出现一次。
- 客户端无法指定「同名模型用哪个 provider」——请求体只有 `model` 字段，路由结果取决于 provider 注册顺序。

**目标**：客户端通过**模型名本身**选择 provider。核心原则：**网关的下发（`/v1/models`）与模型调用（请求路由）必须闭环** —— 客户端从列表拿到的任何名字必须能直接调用，列表之外的行为不承诺。

## 决策

1. **限定名格式**：`{provider_id}/{model_id}`（OpenRouter 风格），如 `deepseek/deepseek-chat`。provider_id 用 `providers.json` 里的 `id`。
2. **路由表只存限定名 key**。裸名 key 不进路由表 → 裸名请求自然 404（不在列表、不在路由表，闭环成立）。
3. **列表只暴露限定名**。`/v1/models` 每个条目就是一个可请求的限定名，客户端无需理解「裸名 vs 限定名」映射。
4. **Anthropic 一致性修复**：Anthropic 端点的同名冲突从「静默覆盖」改为「保留先入者 + warn」，与 OpenAI 端点对齐（同名冲突只可能在限定名 key 的 provider_id 相同、model_id 相同——即同一 provider 内部重复时出现，此时保留先入者是正确行为）。
5. **不做**：`mb-` key 维度的分流、请求级 fallback、round-robin 负载均衡、裸名默认 provider。

## 架构

**不改路由表数据结构，只改 key 的形态**。每张路由表（chat / responses / anthropic）的每个模型，key 从裸 `model_id` 改为 `{provider_id}/{model_id}`。

```
客户端 GET /v1/models
  → [{id: "deepseek/deepseek-chat"}, {id: "openrouter/deepseek-chat"}, ...]

客户端 POST 请求 model="deepseek/deepseek-chat"
  → proxy.rs 查 key = "deepseek/deepseek-chat"
  → 命中 → 路由到 DeepSeek 的 base_url + api_key
  → replace_model_in_body 用 route.model_id（干净名 "deepseek-chat"）回写

客户端 POST 请求 model="deepseek-chat"（裸名）
  → 路由表无此 key → 404（闭环：列表里也没有此名）
```

## 改动点

### 1. `refresh_routes()`（`src/admin/provider_svc.rs:177-238`）

**OpenAI 通道**（`:215-237`）：key 从 `model.model_id.to_lowercase()` 改为 `format!("{}/{}", def.id, model.model_id.to_lowercase())`。

- 冲突处理（`Entry::Occupied`）保留——限定名 key 下 provider_id 相同才可能冲突（同一 provider 内部模型重复），保留先入者 + warn 正确。

**Anthropic 通道**（`:177-196`）：key 派生逻辑顺序调整为——**先拼 provider 前缀，再执行补 `claude-` 前缀的判定**：

```rust
let lower = route.model_id.to_lowercase();
let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);          // 剥 [1m] 后缀
let qualified = format!("{}/{}", def.id, clean);                    // 先拼 provider 前缀
let key = if qualified.starts_with("claude") || qualified.starts_with("anthropic") {
    qualified.to_string()                                            // claude/anthropic 限定名不加前缀
} else {
    format!("claude-{}", qualified)                                  // 非 claude 限定名在最前补 claude-
};
```

- 关键差异：`claude-` 前缀加在**整个限定名最前**（如 `claude-deepseek/deepseek-chat`），而非 `provider/model` 中间。这使 `claude-` 成为「限定名整体的检索名前缀」——Claude Code 据此识别 1M 变体。
- 裸 `insert`（`:194`）改为与 OpenAI 一致的 `Entry::Occupied` 保留先入者 + warn（修复静默覆盖）。

### 2. 查找逻辑（`src/router/proxy.rs:199`）

**不用改**。请求 `model` 直接 `to_lowercase()` 查 key，限定名天然命中，裸名天然 404。

### 3. `/v1/models` 列表（`src/router/models_list.rs`）

`openai_models_list` 与 `get_anthropic_models` 现在直接用路由表 key 作为列表 id。key 变成限定名后，列表**自动就是限定名**。仅需：

- 去掉 `dedup_by(|a, b| a.id == b.id)`（限定名天然唯一，去重反而掩盖问题）。
- Anthropic 的 `[1m]` 后缀补回逻辑（`:58-64`）改为在 key 之后拼接：`format!("{}{}", key, suffix)`（key 已含 provider_id 前缀，直接拼接即 `{provider_id}/{模型}{suffix}`，`claude-` 前缀若存在则在最前）。

### 4. 上游名回写（`src/router/proxy.rs:261-271`）

**不受影响**。`replace_model_in_body` 用 `route.model_id`（干净 model_id，不含 provider 前缀）回写。

### 5. 测试（`src/router/proxy_route_tests.rs`）

补用例：

- 两个 provider 声明同名模型 → `{provider_id}/{model}` 请求精确命中指定 provider（验证上游 base_url + api_key）
- 裸名请求 404（列表与路由表均无此名）
- 限定名请求的上游 body 中 `model` 被回写为干净 model_id（不带 provider 前缀）
- `/v1/models` 列表只含限定名，同名模型两个 provider 各一条

## 影响范围

- `src/admin/provider_svc.rs` — `refresh_routes()` 的 key 拼接 + anthropic 冲突处理
- `src/router/models_list.rs` — 去掉 dedup + anthropic 后缀拼接位置
- `src/router/proxy.rs` — 预期不动，仅验证
- `src/router/proxy_route_tests.rs` — 补用例

## 验证

- `cargo test`（含新用例）
- `cargo clippy`
- 手动：admin 配置两个 provider 同名模型 → `/v1/models` 只含限定名；限定名请求路由正确；裸名请求 404
