# Anthropic 路由：仅冲突模型使用限定名

Date: 2026-08-02

## 背景

当前 anthropic 建表对**每个**模型插双 key：裸名 key（`claude-sonnet-4`）与限定名 key（`claude-{provider_id}/{model}`）。限定名 key 是为解决跨 provider 同名模型而引入，但当前实现是"全量双 key"——连不冲突的模型也生成一条永远用不到的限定名 key，属于冗余。

## 目标

路由表 key 策略从「总是双 key」改为「**按需限定**」：

- 模型名（归一化后的裸名）**不冲突** → 只用裸名 key，不生成限定名 key。
- 模型名**冲突** → 冲突的这些模型只用限定名 key，不使用裸名 key。
- `display_name` 的 `[{provider_id}]` 前缀**也只有冲突的模型才加**；不冲突的模型 display_name 就是纯 `model_name`。

## 冲突判定

沿用现有归一化规则计算裸名 `bare`：`model_id.to_lowercase()` → 剥 `[1m]` 后缀 → 非 `claude`/`anthropic` 开头补 `claude-` 前缀。

对建表过程中收集到的所有 `bare` 统计**总出现次数**（含同 provider 内归一化后相同的模型，如 `claude-sonnet-4` 与 `claude-sonnet-4[1M]`）：

- `count[bare] > 1` → **冲突**（模型走限定名 key）
- `count[bare] == 1` → 不冲突（模型走裸名 key）

## 建表逻辑（两遍构建）

`provider_svc::refresh_routes()` 的 anthropic 部分改为两遍：

**第一遍**：遍历所有启用 anthropic 通道的 (provider, model)，计算每个模型的 `bare`，统计冲突集合。

**第二遍**：再遍历一次，按冲突与否生成 key：

| 场景 | key | 冲突处理 |
|---|---|---|
| 不冲突 | 裸名 `bare` | 直接 insert（count==1 保证 key 唯一，无冲突分支） |
| 冲突 | 限定名 `claude-{provider_id}/{clean_id}`（`clean_id` = 剥 `[1m]` + 剥自带 `claude-` 前缀后的 model_id） | 保留先入者 + warn（覆盖同 provider 归一化同名、限定名 key 撞车边缘情况） |

冲突模型的裸名 key **完全不建**。

**同 provider `[1m]` 变体优先**：当限定名 key 撞车且两条路由属于同一 provider 时（典型场景：同一 provider 同时声明 `claude-sonnet-4` 与 `claude-sonnet-4[1M]`，两者归一化裸名相同），优先保留 `[1m]`-后缀变体——若 incoming 带 `[1m]` 而 existing 不带，则用 incoming 覆盖 existing；其余撞车情况仍保留先入者 + warn。这样无论 DB 返回行的顺序如何，`[1M]` 变体始终胜出，避免 Claude Code 因 `[1M]` 变体从 `/v1/models` 消失而无法开启 1M 上下文。跨 provider 不会撞车（qualified_key 含 provider_id）。

## 列表侧（无新字段方案）

`ProviderRoute.model_name` 的唯一消费点是 `models_list.rs` 的 display_name（`proxy.rs` 转发链路不读 `model_name`，已确认）。因此**不加 `is_ambiguous` 字段**，改为建表时直接改写 `model_name`：

- 冲突模型：建表时 `model_name` 直接设为 `format!("[{}]{}", provider_id, model_name)`，前缀在源头打上。
- 非冲突模型：`model_name` 原样。

列表侧 `get_anthropic_models` 的 display_name 从 `format!("[{}]{}", route.provider_id, route.model_name)` 改为 `route.model_name.clone()`（前缀已在建表时写入）。

`state.rs` 不改动；`id`（检索 key）与 `[1m]` 后缀补回逻辑不变。

## 转发逻辑

**零改动**。`proxy.rs` 用检索 key 查表，`route.model_id` 剥 `[1m]` 后缀后发上游，与 key 形态及 `model_name` 无关。

## 测试改动（`proxy_route_tests.rs`）

1. **`anthropic_qualified_name_routes_to_correct_provider`**（alpha/beta 冲突）：改断言——路由表**不含**裸名 `claude-sonnet-4`，含两条限定名 key；裸名请求 `claude-sonnet-4` → **404**；限定名请求各自命中；display_name 断言保留（`[alpha]Claude Sonnet 4` / `[beta]Claude Sonnet 4`）。
2. **`anthropic_qualified_name_upstream_body_is_clean_model_id`**：原测试 kimi 单模型不冲突，新逻辑下无限定名 key，需**重写为冲突场景**——两个 provider 声明相同 `claude-kimi-k3[1M]`，用限定名请求验证上游收到干净 `claude-kimi-k3`。
3. **新增**：非冲突场景（单 provider 单模型）——路由表只含裸名 key、**不含**限定名 key；裸名请求 200；display_name 为纯 `model_name` 无前缀。

## 受影响文件

- `src/admin/provider_svc.rs`：anthropic 建表改两遍构建 + 冲突模型改写 `model_name`
- `src/router/models_list.rs`：display_name 改为直接取 `route.model_name`
- `src/router/proxy_route_tests.rs`：改两个用例 + 新增一个
