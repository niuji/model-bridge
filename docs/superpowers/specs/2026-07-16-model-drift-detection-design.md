# 上游模型漂移自动探测与感知 — 设计

Date: 2026-07-16
Status: Approved (design, pending spec review)

## 背景与现状

`provider_models` 是用户**筛选过**的子集——用户从上游 `/v1/models` 里挑选要路由的模型。当前唯一的"感知上游变化"方式是手动的：在供应商配置弹窗里点「同步」，前端实时探测上游、浏览器内 `computeDiff` 算出 added/removed/renamed、弹差异框勾选、应用、再保存。整个过程不落库。后台定时任务（`refresh_interval_min`，默认 10 分钟）只从**已存** `provider_models` 重建路由表（`refresh_routes`），从不探测上游。

**缺口**：检测按需、浏览器内、不持久；上游悄悄增删模型，管理员除非手动每个通道点同步，否则毫无察觉。

## 目标

让系统**主动**感知上游模型列表的变化，并在供应商卡片上以角标提示"自上次查看以来"的变化；点开一个只读弹窗列出变化，同时清零角标。**不**引入采纳/移除等额外流程——采纳与移除仍走现有配置弹窗。

## 关键设计决策

1. **自动后台探测**：新增独立定时任务，调用现有 `fetch_models_from_api` 探测上游，与路由刷新（`refresh_routes`）解耦。
2. **感知范围**：新增 + 下架（上游相对 baseline 的增减）。跳过改名。
3. **比较基准 = baseline，不是 `provider_models`**：`provider_models` 是用户筛选集，不能当 diff 基准（会把"故意没采纳的"误报成新增）。baseline = 上次打开"变更"弹窗时落地的上游快照。变更 = 当前上游 vs baseline 的对称差。`provider_models` 不进 diff。
4. **交互**：卡片角标（变化计数）+ 新的只读"变更"弹窗（按通道列 new/removed）。打开弹窗即落地 baseline、角标清零。无采纳/移除流程（仍走配置弹窗）。
5. **探测节奏独立**：新增 `[bridge] probe_interval_min`，默认 **1440（1 天）**，`.max(1)` 钳制。模型目录变化是天/周级，每 10 分钟探测上游偏勤；独立日频更贴上游实际节奏，且不绑死路由刷新。

## 配置

`[bridge]` 新增字段：

```toml
[bridge]
refresh_interval_min = 10   # 路由表刷新（已有）
probe_interval_min   = 1440 # 上游模型探测（新增，默认 1 天）
```

- `config.rs` 的 `Bridge` 结构加 `pub probe_interval_min: u64`，默认 `1440`。
- 运行时 `.max(1)` 钳制（与 `refresh_interval_min` 一致，避免配 0 退化成忙循环）。

## 数据模型（新增 2 张表）

```sql
-- 上游"最近一次成功探测"的当前快照。探测成功后按 (provider, channel) 整体替换。
CREATE TABLE upstream_models (
  provider_id   TEXT NOT NULL,
  channel_type  TEXT NOT NULL,
  model_id      TEXT NOT NULL,
  model_name    TEXT NOT NULL DEFAULT '',
  last_seen_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (provider_id, channel_type, model_id)
);

-- baseline：上次打开"变更"弹窗时落地的上游快照。结构与 upstream_models 一致。
CREATE TABLE upstream_models_seen (
  provider_id   TEXT NOT NULL,
  channel_type  TEXT NOT NULL,
  model_id      TEXT NOT NULL,
  model_name    TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (provider_id, channel_type, model_id)
);
```

- `upstream_models` 探测成功后整体替换（事务内 DELETE + INSERT）；探测失败时**保留旧快照不清空**。
- `upstream_models_seen` 仅在打开变更弹窗时落地（:= 当前 `upstream_models`）。
- 迁移加在 `schema.rs::run_migrations` 末尾，`CREATE TABLE IF NOT EXISTS`。

## 后台探测

新增 `probe_upstream_models(state: &Arc<AppState>)`，在 `main.rs` 用**独立** `tokio::spawn` 任务调用（镜像现有 refresh 任务结构）：

- `tokio::time::interval(Duration::from_secs(probe_interval_min.max(1) * 60))`；首次 tick 立即触发→启动即播种快照，之后每 `probe_interval_min` 一次。
- 遍历 `state.provider_defs`；跳过 `is_enabled=false` 或无 `api_key` 的 provider。
- 对每个通道，要求：channel `is_enabled` 且有 `models_endpoint` 且 `is_safe_base_url(base_url)`。
- 调用现有 `fetch_models_from_api(&state.client, &state.provider_defs, &def.id, &ch.channel_type, &stored_api_key)`——它已处理 anthropic→`x-api-key` / 其余→Bearer、10s 超时、空列表 bail。
- 成功：事务内 `DELETE FROM upstream_models WHERE provider_id=? AND channel_type=?`，批量 INSERT 新行。
- 失败（无 key / 401 / 超时 / 非 2xx / 空列表）：**不动** `upstream_models`，`tracing::warn!` 一次（不落状态表）。
- 探测串行（小规模通道墙钟可接受）。

## 漂移计算（读时派生）

不存"漂移表"。在请求时用 `NOT EXISTS` 反查 `upstream_models` vs `upstream_models_seen`，model_id 大小写不敏感（与路由表 lowercased 查找、前端 `normId` 一致）：

- **new**（自上次查看以来上游新增）= `upstream_models` 有、`upstream_models_seen` 同通道没有的。
- **removed**（自上次查看以来上游下架）= `upstream_models_seen` 有、`upstream_models` 同通道没有的。

`provider_models` 不参与。

## API

- **`GET /api/admin/providers`**（扩展）：每个 provider 附带 `drift: { new: number, removed: number }`（聚合该 provider 所有通道）。用两条 set-based `NOT EXISTS ... GROUP BY provider_id` 一次查全，避免 N+1。**不**落地 baseline。
- **`GET /api/admin/providers/{id}/model-changes`**（新增）：
  1. 算 drift（current vs **旧** baseline），按通道返回 `{ channels: [{ channel_type, added: [{model_id, model_name}], removed: [...] }] }`；
  2. **落地 baseline**（事务）：`DELETE FROM upstream_models_seen WHERE provider_id=?`，`INSERT INTO upstream_models_seen (provider_id, channel_type, model_id, model_name) SELECT provider_id, channel_type, model_id, model_name FROM upstream_models WHERE provider_id=?`；
  3. 返回 drift。
  - **首看防灌水**：若该 provider 的 `upstream_models_seen` 为空（从没打开过），drift 返回空，仍落地 baseline。
  - baseline 粒度 per-provider（打开变更弹窗 = 看了该 provider 全部通道）。

## 前端

- **卡片角标**：`ProviderSummary` 增加 `drift?: { new: number; removed: number }`。在 `card-meta`（"在线 · N 模型"旁）加小药丸，仅当 `new>0 || removed>0` 渲染，如 `✚3` / `✚3 ✖1`（绿 `#2ea86a` / 红 `#b3261e`）。`@click.stop="openChanges(p)"` 开变更弹窗（不触发卡片 `openConfig`）。
- **变更弹窗（新、只读）**：
  - `openChanges` → `GET /providers/{id}/model-changes`（打开即落地 baseline）→ 按通道列 `✚ added` / `✖ removed` 模型名。
  - 只读，无采纳/移除按钮。底部仅「关闭」（打开即已清零角标）。
  - 空时显示"自上次查看以来无变化"。
- **配置弹窗 + 「同步」**：不变。采纳新增 / 移除下架 仍在此手动编辑或用「同步」（实时探测 + `computeDiff` vs `provider_models`，现有逻辑）——这是"我现在就要最新"的即时通道，不受日频探测限制。

## 边界与取舍

- **首看防灌水**：baseline 缺失时 drift 置空、只落地，避免首次打开把全量当新增。
- **探测失败**：`tracing::warn!` + 保留旧快照；不当作漂移，不落状态表。
- **角标=瞬态**：随打开变更弹窗清零；不做"已忽略"持久化。
- **角标新鲜度上界 = `probe_interval_min`**（默认 24h）：上游在两次探测之间的变化，角标要到下次探测才反映；要即时看，用配置弹窗「同步」。
- **下架且仍在路由**：不做持久告警（用户明确"不用其他流程"）；用户在变更弹窗看到下架后，去配置弹窗手动移除。v1 仅列变化，不标"仍在路由"。

## 不做（YAGNI）

- 改名/重命名感知。
- per-model 首次出现时间历史。
- apply-drift 采纳/移除端点（采纳走现有配置弹窗）。
- 探测状态表 / 失败持久化 / 角标"上次检查失败"提示。
- 持久"已忽略"或"下架失效"告警。
- 独立"模型更新"收件箱页。

## 涉及文件（预估）

- `src/config.rs` — `Bridge` 加 `probe_interval_min`（默认 1440）。
- `src/db/schema.rs` — 两张新表迁移。
- `src/admin/provider_svc.rs` — `probe_upstream_models`、drift 计算、`list_providers` 加 drift、新增 `get_model_changes`（算 + 落地 baseline）。
- `src/router/admin.rs` — 新 handler `model_changes`。
- `src/router/mod.rs` — 注册 `GET /providers/{id}/model-changes`。
- `src/db/models.rs` — `DriftSummary` 等 DTO。
- `src/main.rs` — 新增独立 `tokio::spawn` 探测任务。
- `web/src/views/Providers.vue` — 卡片角标 + 变更弹窗 + `openChanges`。
