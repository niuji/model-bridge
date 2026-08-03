# OpenAI（chat/responses）路由：跨 provider 同名模型限定名

Date: 2026-08-03

## 背景

当前 openai 建表（`provider_svc.rs:286-321`）对 `openai_chat` 与 `openai_responses` 两通道各自独立建表，key 为 `model_id.to_lowercase()`（裸名）。跨 provider 声明同名模型时，**保留先入者 + warn，后入者完全丢弃**——后入 provider 的模型在 `/v1/models` 里看不到，请求也 404，且无任何途径指定。

anthropic 已实现「仅冲突模型使用限定名 key」方案（`claude-{provider_id}/{model_id}`），冲突模型可在多 provider 间精确路由。本变更把同一套原则推广到 openai 的两个通道。

## 目标

openai 跨 provider 同名模型支持，行为与 anthropic 对齐：

- 归一化裸名**不冲突**（count==1）→ 只用裸名 key（如 `gpt-4o`），同现状。
- 归一化裸名**冲突**（count>1）→ 该模型的 key 改用限定名 `{provider_id}/{model_id}`，裸名 key **完全不建**。
- 冲突模型的 `model_name` 加 `[{provider_id}]` 前缀（展示层区分来源）。

## 限定名格式

`{provider_id}/{model_id}`，其中：

- `provider_id` **转小写**。代理查找侧总是 `model.to_lowercase()`（`proxy.rs:194`），限定名 key 若嵌入大写 provider id，冲突模型将彻底不可达（2026-08-03 已修 anthropic 同款 bug，见 commit）。
- `model_id` 转小写（`to_lowercase()` 后的裸名本身）。

示例：provider `kimi` 与 `bigmodel` 都声明 `glm-5.2` → key 为 `kimi/glm-5.2` 与 `bigmodel/glm-5.2`。

## 冲突判定

openai 的 `bare` 归一化**比 anthropic 简单**：anthropic 需剥 `[1m]` 后缀 + 补 `claude-` 前缀；openai 的 `bare` 就是 `model_id.to_lowercase()`，无后缀、无前缀规则。

**冲突检测粒度：`openai_chat` 与 `openai_responses` 各自独立统计**（按通道隔离的现状不被破坏）：

- chat 表只统计 `channel_type == "openai_chat"` 的模型。
- responses 表只统计 `channel_type == "openai_responses"` 的模型。
- 一张表内的同名模型互相冲突；跨表（chat vs responses）同名不互相影响。

## 建表逻辑（`provider_svc::refresh_routes`）

openai 部分改为两遍构建，与 anthropic 结构对称：

**第一遍（预扫描）**：遍历所有启用 openai 通道的 (provider, model)，按通道各自统计裸名出现次数。新增两个 map：

- `openai_chat_bare_counts: HashMap<String, usize>`
- `openai_responses_bare_counts: HashMap<String, usize>`

**第二遍（主循环）**：openai 循环（现 `provider_svc.rs:286-321`）按当前通道查对应 map，按冲突与否生成 key：

| 场景 | key | 冲突处理 |
|---|---|---|
| 不冲突（count==1） | 裸名 `model_id.to_lowercase()` | 直接 insert（count==1 保证 key 唯一，无冲突分支） |
| 冲突（count>1） | 限定名 `{provider_id_lower}/{model_id_lower}` | Vacant 入库；Occupied 保留先入者 + warn |

冲突模型的裸名 key **完全不建**。冲突模型的 `model_name` 打上 `[{provider_id}]` 前缀（与 anthropic 一致，`provider_id` 保留原始大小写，前缀仅展示用）。

**无 `[1m]` 变体优先级**：anthropic 的 `[1m]` 优先分支（`provider_svc.rs:258-274`）是 Claude Code 1M 上下文专用，OpenAI 无此概念，Occupied 分支只保留先入者 + warn。

## 列表侧（`models_list.rs`）

**零改动**。openai 列表（`openai_models_list`）的 `id` 直接用路由 key（冲突时即限定名），`owned_by` 用 `route.provider_name`——本方案不改 `owned_by`：

- 冲突模型的来源信息已完整含在 `id`（`kimi/glm-5.2`）。
- anthropic 的 `[{provider_id}]` 前缀写在 `display_name` 是因为 Anthropic 协议有独立展示字段；OpenAI 协议的 `owned_by` 不是展示字段，保留 `provider_name` 即可，信息不重复。

## 转发逻辑

**零改动**。`proxy.rs` 用检索 key（已 `to_lowercase`）查表，命中后 `replace_model_in_body` 用 `route.model_id`（原始大小写）回写 body 再发上游。限定名 key 只是路由表的检索 key，上游仍收到原始 `model_id`（如 `glm-5.2`），无 provider 前缀。

## 测试改动（`proxy_route_tests.rs`）

复用现有 mock 模式（wiremock + `build_state_with_defs` + `update_provider` + `refresh_routes`），新增 3 个用例：

1. **`openai_chat_conflicting_models_use_qualified_key`**：两 provider 声明同名 `gpt-4o`（chat 通道）→ 路由表只含 `alpha/gpt-4o`、`beta/gpt-4o` 两个限定名 key，裸名 `gpt-4o` 不存在；`/v1/models` 下发这两个 id；`alpha/gpt-4o` 请求 → 命中 server_a；裸名 `gpt-4o` 请求 → 404。
2. **`openai_chat_non_conflicting_uses_only_bare_key`**：单 provider 声明 `gpt-4o` → 只有裸名 key，无限定名 key；`/v1/models` 下发 `gpt-4o`；裸名请求 200。
3. **`openai_chat_responses_conflict_independent`**：同 provider 在 chat 与 responses 各声明同名 `gpt-4o`；两表互不影响——各自独立，chat 的表只有裸名 key，responses 的表也只有裸名 key（单 provider 内同名跨表不冲突）。

## 受影响文件

- `src/admin/provider_svc.rs`：openai 建表改两遍构建（新增 openai 通道裸名冲突预扫描 + 冲突模型走限定名 key + 改写 `model_name`）
- `src/router/proxy_route_tests.rs`：新增 3 个用例

## 不做的事

- 不改 `models_list.rs`、`proxy.rs`、`state.rs`、前端。
- 不引入通用抽象把 anthropic 与 openai 的冲突检测耦合（anthropic 统一统计、openai 按通道独立，语义不同，刻意各自实现）。
- 非冲突模型不发布限定名 key（与 anthropic 一致）。
