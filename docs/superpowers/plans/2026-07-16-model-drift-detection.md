# Model Drift Detection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 后台自动探测上游 `/v1/models`，以 baseline（上次打开"变更"弹窗时落地的上游快照）的对称差感知新增/下架，在供应商卡片上以角标提示，点开只读弹窗列出变化并清零角标。

**Architecture:** 新增独立定时任务（`probe_interval_min`，默认 1440 分钟/天）调用现有 `fetch_models_from_api` 探测每个已启用通道，把结果写入 `upstream_models` 快照表。漂移在请求时派生（Rust 侧纯函数 `compute_drift`，不存"漂移表"）：当前快照 vs `upstream_models_seen` baseline 的对称差。`GET /providers` 返回每 provider 的 drift 计数（角标）；`GET /providers/{id}/model-changes` 返回明细并落地 baseline（打开即清零）。采纳/移除仍走现有配置弹窗，本特性不含。

**Tech Stack:** Rust (axum 0.8 / sqlx 0.8 sqlite / tokio / reqwest)，Vue 3 + Naive UI + Vite。测试为模块内 `#[cfg(test)] mod tests`（纯函数单测 + sqlx `sqlite::memory:` 集成测）。前端无测试框架，验证靠 `npm run build` + 手动点测。

**Spec:** `docs/superpowers/specs/2026-07-16-model-drift-detection-design.md`

---

## File Structure

| File | Responsibility | Action |
|------|----------------|--------|
| `src/config.rs` | `BridgeConfig` 加 `probe_interval_min` 字段 + 默认 1440 | Modify |
| `src/db/schema.rs` | 两张新表迁移 `upstream_models` / `upstream_models_seen` | Modify |
| `src/db/models.rs` | DTO：`UpstreamModelRow`/`ModelEntry`/`ChannelDrift`/`DriftSummary`，`ProviderSummary.drift` | Modify |
| `src/admin/provider_svc.rs` | 纯 `compute_drift`/`select_probe_targets`；DB 存取 `fetch_*_snapshot`/`land_baseline`；`get_model_changes`/`probe_upstream_models`/`replace_upstream_snapshot`；`list_providers` 加 drift | Modify |
| `src/router/admin.rs` | `model_changes` handler | Modify |
| `src/router/mod.rs` | 注册 `GET /providers/{id}/model-changes` | Modify |
| `src/main.rs` | 独立 `tokio::spawn` 探测任务 | Modify |
| `model-bridge.toml.example` | `[bridge]` 加 `probe_interval_min` | Modify |
| `web/src/views/Providers.vue` | 卡片角标 + 只读"上游变更"弹窗 + `openChanges` | Modify |

---

## Task 1: 配置项 `probe_interval_min`

**Files:**
- Modify: `src/config.rs:36-40`（`BridgeConfig`）, `src/config.rs:79-81`（`Default`）

- [ ] **Step 1: 给 `BridgeConfig` 加字段**

在 `src/config.rs` 的 `BridgeConfig` 中，把：
```rust
    /// 后台自动刷新模型列表间隔（分钟）
    pub refresh_interval_min: u64,
}
```
改为：
```rust
    /// 后台自动刷新模型列表间隔（分钟）
    pub refresh_interval_min: u64,
    /// 后台探测上游 /v1/models 的间隔（分钟），与路由刷新解耦。
    /// 默认 1440（1 天）。模型目录变化是天/周级，日频已足够。
    pub probe_interval_min: u64,
}
```

- [ ] **Step 2: 给 `Default` 加默认值**

把 `impl Default for AppConfig` 中的：
```rust
            bridge: BridgeConfig {
                refresh_interval_min: 10,
            },
```
改为：
```rust
            bridge: BridgeConfig {
                refresh_interval_min: 10,
                probe_interval_min: 1440,
            },
```

- [ ] **Step 3: 验证编译 + 既有测试**

Run: `cargo check && cargo test`
Expected: 编译通过；所有既有测试 PASS（crypto / proxy 单测）。

- [ ] **Step 4: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): 新增 probe_interval_min 配置（默认 1440 分钟/天）"
```

---

## Task 2: 数据库迁移 — 两张快照表

**Files:**
- Modify: `src/db/schema.rs`（`run_migrations` 末尾，`Ok(())` 之前）

- [ ] **Step 1: 加两张表**

在 `src/db/schema.rs` 的 `run_migrations` 函数体末尾、`Ok(())` 之前插入：
```rust
    // 上游模型快照：探测成功后按 (provider, channel) 整体替换；失败时保留旧值。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upstream_models (
            provider_id   TEXT NOT NULL,
            channel_type  TEXT NOT NULL,
            model_id      TEXT NOT NULL,
            model_name    TEXT NOT NULL DEFAULT '',
            last_seen_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (provider_id, channel_type, model_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // baseline：上次打开"变更"弹窗时落地的上游快照（结构与 upstream_models 一致，无 last_seen_at）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upstream_models_seen (
            provider_id   TEXT NOT NULL,
            channel_type  TEXT NOT NULL,
            model_id      TEXT NOT NULL,
            model_name    TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (provider_id, channel_type, model_id)
        )
        "#,
    )
    .execute(pool)
    .await?;
```

- [ ] **Step 2: 验证编译**

Run: `cargo check`
Expected: 编译通过（表的正确性由 Task 6 的集成测试覆盖）。

- [ ] **Step 3: Commit**

```bash
git add src/db/schema.rs
git commit -m "feat(db): 新增 upstream_models / upstream_models_seen 快照表"
```

---

## Task 3: DTO 类型

**Files:**
- Modify: `src/db/models.rs`（文件末尾追加类型；并改 `ProviderSummary`）

- [ ] **Step 1: 在 `src/db/models.rs` 末尾追加四个 DTO**

```rust
/// 上游快照中的一行（FromRow，仅取 diff 所需列）
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UpstreamModelRow {
    pub provider_id: String,
    pub channel_type: String,
    pub model_id: String,
    pub model_name: String,
}

/// 模型条目（变更弹窗展示用）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelEntry {
    pub model_id: String,
    pub model_name: String,
}

/// 单通道的上游变更（current 相对 baseline 的对称差）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelDrift {
    pub channel_type: String,
    pub added: Vec<ModelEntry>,
    pub removed: Vec<ModelEntry>,
}

/// 卡片角标用的变更计数
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DriftSummary {
    pub new: i64,
    pub removed: i64,
}
```

- [ ] **Step 2: 给 `ProviderSummary` 加 `drift` 字段**

把：
```rust
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub is_enabled: bool,
    pub channels: Vec<ChannelDetail>,
}
```
改为：
```rust
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub is_enabled: bool,
    pub channels: Vec<ChannelDetail>,
    /// 自上次打开"上游变更"弹窗以来的变化计数（baseline 为空/未查看过时为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift: Option<DriftSummary>,
}
```

- [ ] **Step 3: 修受 `ProviderSummary` 构造的调用点**

`src/admin/provider_svc.rs::list_providers` 末尾构造 `ProviderSummary { ... }` 缺 `drift` 字段会编译失败。临时补 `drift: None` 让本任务编译通过（Task 8 再填真实值）：

把 `list_providers` 中的：
```rust
        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            channels,
        });
```
改为：
```rust
        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            channels,
            drift: None,
        });
```

- [ ] **Step 4: 验证编译**

Run: `cargo check`
Expected: 编译通过。

- [ ] **Step 5: Commit**

```bash
git add src/db/models.rs src/admin/provider_svc.rs
git commit -m "feat(models): 新增 drift/upstream DTO，ProviderSummary 加 drift 字段"
```

---

## Task 4: 纯函数 `compute_drift` + 单元测试（TDD）

**Files:**
- Modify: `src/admin/provider_svc.rs`（顶部 import + 新函数 + 测试模块）

- [ ] **Step 1: 扩展顶部 import**

把 `src/admin/provider_svc.rs` 顶部的：
```rust
use crate::db::models::{
    ChannelDetail, ProviderChannelConfigRow, ProviderConfigRow, ProviderDetail, ProviderModel,
    ProviderSummary,
};
```
改为：
```rust
use crate::db::models::{
    ChannelDetail, ChannelDrift, DriftSummary, ModelEntry, ProviderChannelConfigRow, ProviderConfigRow,
    ProviderDetail, ProviderModel, ProviderSummary, UpstreamModelRow,
};
```

- [ ] **Step 2: 写失败测试（含 helper）**

在 `src/admin/provider_svc.rs` 文件**末尾**追加测试模块：
```rust
#[cfg(test)]
mod drift_tests {
    use super::*;
    use crate::config::{ChannelDef, ProviderDef};
    use sqlx::SqlitePool;

    fn row(ct: &str, id: &str, name: &str) -> UpstreamModelRow {
        UpstreamModelRow {
            provider_id: "p".into(),
            channel_type: ct.into(),
            model_id: id.into(),
            model_name: name.into(),
        }
    }

    #[test]
    fn added_when_current_has_baseline_missing() {
        let cur = vec![row("openai_chat", "glm-5.2-air", "GLM 5.2 Air")];
        let d = compute_drift(&cur, &[]);
        assert_eq!(
            d,
            vec![ChannelDrift {
                channel_type: "openai_chat".into(),
                added: vec![ModelEntry { model_id: "glm-5.2-air".into(), model_name: "GLM 5.2 Air".into() }],
                removed: vec![],
            }]
        );
    }

    #[test]
    fn removed_when_baseline_has_current_missing() {
        let base = vec![row("openai_chat", "glm-4-air", "GLM 4 Air")];
        let d = compute_drift(&[], &base);
        assert_eq!(
            d,
            vec![ChannelDrift {
                channel_type: "openai_chat".into(),
                added: vec![],
                removed: vec![ModelEntry { model_id: "glm-4-air".into(), model_name: "GLM 4 Air".into() }],
            }]
        );
    }

    #[test]
    fn case_insensitive_match_not_flagged() {
        // current GLM-5.2 / baseline glm-5.2 → 同一模型，不算新增也不算下架
        let cur = vec![row("openai_chat", "GLM-5.2", "x")];
        let base = vec![row("openai_chat", "glm-5.2", "y")];
        assert_eq!(compute_drift(&cur, &base), vec![]);
    }

    #[test]
    fn per_channel_isolation() {
        // 同 model_id 不同通道 → 互不抵消：anthropic 新增、openai_chat 下架
        let cur = vec![row("anthropic", "claude-x", "c")];
        let base = vec![row("openai_chat", "claude-x", "c")];
        let d = compute_drift(&cur, &base);
        let mut chans: Vec<(String, usize, usize)> = d
            .into_iter()
            .map(|c| (c.channel_type, c.added.len(), c.removed.len()))
            .collect();
        chans.sort();
        assert_eq!(
            chans,
            vec![
                ("anthropic".to_string(), 1, 0),
                ("openai_chat".to_string(), 0, 1),
            ]
        );
    }

    #[test]
    fn rename_only_is_ignored() {
        // 同 model_id、不同 model_name → 既不在 added 也不在 removed（跳过改名）
        let cur = vec![row("openai_chat", "glm-5.2", "new name")];
        let base = vec![row("openai_chat", "glm-5.2", "old name")];
        assert_eq!(compute_drift(&cur, &base), vec![]);
    }

    // 以下 helper/池供 Task 5/6 复用，此处先占位避免 unused 警告
    #[allow(dead_code)]
    fn chan(ct: &str, base: &str, ep: Option<&str>) -> ChannelDef {
        ChannelDef { channel_type: ct.into(), base_url: base.into(), models_endpoint: ep.map(String::from) }
    }
    #[allow(dead_code)]
    fn def(id: &str, chans: Vec<ChannelDef>) -> ProviderDef {
        ProviderDef { id: id.into(), name: id.into(), icon: None, channels: chans }
    }
    #[allow(dead_code)]
    async fn mempool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        pool
    }
}
```

- [ ] **Step 3: 跑测试，确认红（函数未定义 → 编译失败）**

Run: `cargo test compute_drift`
Expected: 编译失败，错误信息形如 `cannot find function 'compute_drift'`。

- [ ] **Step 4: 实现 `compute_drift`**

在 `src/admin/provider_svc.rs`（内部辅助函数区，`is_safe_base_url` 附近）追加：
```rust
/// 对称差：current 相对 baseline 的新增（current 有、baseline 无）与下架（baseline 有、current 无）。
/// 按 (channel_type, model_id 小写) 比对——同通道大小写不敏感、跨通道不误配。model_name 变化不算（跳过改名）。
pub fn compute_drift(current: &[UpstreamModelRow], baseline: &[UpstreamModelRow]) -> Vec<ChannelDrift> {
    use std::collections::HashMap;
    let key = |r: &UpstreamModelRow| (r.channel_type.clone(), r.model_id.to_lowercase());
    let base_keys: std::collections::HashSet<(String, String)> = baseline.iter().map(key).collect();
    let cur_keys: std::collections::HashSet<(String, String)> = current.iter().map(key).collect();

    let mut by_channel: HashMap<String, ChannelDrift> = HashMap::new();
    for r in current {
        if !base_keys.contains(&key(r)) {
            by_channel
                .entry(r.channel_type.clone())
                .or_insert_with(|| ChannelDrift {
                    channel_type: r.channel_type.clone(),
                    added: vec![],
                    removed: vec![],
                })
                .added
                .push(ModelEntry { model_id: r.model_id.clone(), model_name: r.model_name.clone() });
        }
    }
    for r in baseline {
        if !cur_keys.contains(&key(r)) {
            by_channel
                .entry(r.channel_type.clone())
                .or_insert_with(|| ChannelDrift {
                    channel_type: r.channel_type.clone(),
                    added: vec![],
                    removed: vec![],
                })
                .removed
                .push(ModelEntry { model_id: r.model_id.clone(), model_name: r.model_name.clone() });
        }
    }

    let mut out: Vec<ChannelDrift> = by_channel.into_values().collect();
    out.sort_by(|a, b| a.channel_type.cmp(&b.channel_type));
    out
}
```

- [ ] **Step 5: 跑测试，确认绿**

Run: `cargo test compute_drift`
Expected: 5 个测试全部 PASS。

- [ ] **Step 6: Commit**

```bash
git add src/admin/provider_svc.rs
git commit -m "feat(provider_svc): 纯函数 compute_drift 及单测"
```

---

## Task 5: 纯函数 `select_probe_targets` + 单元测试（TDD）

**Files:**
- Modify: `src/admin/provider_svc.rs`（新函数 + 在 `drift_tests` 模块补测试）

- [ ] **Step 1: 写失败测试**

在 `drift_tests` 模块中（Task 4 已建的 `chan`/`def` helper 之上）追加：
```rust
    #[test]
    fn probe_skips_disabled_provider_and_missing_key() {
        let defs = vec![def("a", vec![chan("openai_chat", "https://x/v1", Some("https://x/v1/models"))])];
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("a".to_string(), false);
        let mut keys = std::collections::HashMap::new();
        keys.insert("a".to_string(), "sk-1".to_string());
        assert!(select_probe_targets(&defs, &enabled, &keys, &Default::default()).is_empty());

        // 启用但无 key → 仍跳过
        enabled.insert("a".to_string(), true);
        keys.remove("a");
        assert!(select_probe_targets(&defs, &enabled, &keys, &Default::default()).is_empty());
    }

    #[test]
    fn probe_skips_disabled_channel_missing_endpoint_nonhttp() {
        let defs = vec![def("a", vec![
            chan("openai_chat", "https://x/v1", Some("https://x/v1/models")), // 命中
            chan("openai_responses", "https://x/v1", Some("https://x/v1/models2")), // 通道关
            chan("anthropic", "https://x/v1", None),                          // 无 endpoint
            chan("openai_chat2", "https://x/v1", Some("file:///etc/passwd")),  // 非 http
        ])];
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("a".to_string(), true);
        let mut keys = std::collections::HashMap::new();
        keys.insert("a".to_string(), "sk-1".to_string());
        let mut ch_en = std::collections::HashMap::new();
        ch_en.insert(("a".to_string(), "openai_responses".to_string()), false);

        let t = select_probe_targets(&defs, &enabled, &keys, &ch_en);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].channel_type, "openai_chat");
        assert_eq!(t[0].models_endpoint, "https://x/v1/models");
    }
```

- [ ] **Step 2: 跑测试，确认红**

Run: `cargo test select_probe_targets`
Expected: 编译失败，`cannot find function 'select_probe_targets'`。

- [ ] **Step 3: 实现 `select_probe_targets` 与 `ProbeTarget`**

在 `src/admin/provider_svc.rs`（`compute_drift` 附近）追加：
```rust
/// 一轮探测的目标
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeTarget {
    pub provider_id: String,
    pub channel_type: String,
    pub models_endpoint: String,
    pub api_key: String,
}

/// 选出本轮要探测的 (provider, channel)：provider 已启用且有 api_key；
/// 通道已启用、有 models_endpoint、且 models_endpoint 为 http(s)（即真正会被 GET 的 URL）。
pub fn select_probe_targets(
    defs: &[ProviderDef],
    provider_enabled: &HashMap<String, bool>,
    provider_api_key: &HashMap<String, String>,
    channel_enabled: &HashMap<(String, String), bool>,
) -> Vec<ProbeTarget> {
    let mut out = Vec::new();
    for def in defs {
        if !provider_enabled.get(&def.id).copied().unwrap_or(false) {
            continue;
        }
        let Some(api_key) = provider_api_key.get(&def.id).filter(|k| !k.is_empty()).cloned() else {
            continue;
        };
        for ch in &def.channels {
            let ch_on = channel_enabled
                .get(&(def.id.clone(), ch.channel_type.clone()))
                .copied()
                .unwrap_or(true);
            if !ch_on {
                continue;
            }
            let Some(ep) = ch.models_endpoint.as_deref() else { continue };
            if !is_safe_base_url(ep) {
                continue;
            }
            out.push(ProbeTarget {
                provider_id: def.id.clone(),
                channel_type: ch.channel_type.clone(),
                models_endpoint: ep.to_string(),
                api_key: api_key.clone(),
            });
        }
    }
    out
}
```

- [ ] **Step 4: 跑测试，确认绿**

Run: `cargo test select_probe_targets`
Expected: 2 个测试 PASS。

- [ ] **Step 5: Commit**

```bash
git add src/admin/provider_svc.rs
git commit -m "feat(provider_svc): 纯函数 select_probe_targets 及单测"
```

---

## Task 6: `get_model_changes` + DB 存取 + 集成测试（TDD）

**Files:**
- Modify: `src/admin/provider_svc.rs`（DB 存取函数 + `get_model_changes` + 集成测试）

- [ ] **Step 1: 写失败集成测试**

在 `drift_tests` 模块中追加：
```rust
    #[tokio::test]
    async fn model_changes_returns_drift_then_lands_baseline() {
        let pool = mempool().await;
        // 当前快照：a, b（openai_chat 通道）
        insert_up(&pool, "p", "openai_chat", &[("a", "A"), ("b", "B")]).await;

        // 第一次：baseline 为空 → 不灌水，返回空并落地 baseline={a,b}
        let d1 = get_model_changes(&pool, "p").await.unwrap();
        assert!(d1.is_empty(), "first view must not flood");

        // 上游变化：下架 a、新增 c → current={b,c}
        sqlx::query("DELETE FROM upstream_models WHERE provider_id = 'p'")
            .execute(&pool).await.unwrap();
        insert_up(&pool, "p", "openai_chat", &[("b", "B"), ("c", "C")]).await;

        let d2 = get_model_changes(&pool, "p").await.unwrap();
        let added: Vec<String> = d2.iter().flat_map(|c| c.added.iter().map(|m| m.model_id.clone())).collect();
        let removed: Vec<String> = d2.iter().flat_map(|c| c.removed.iter().map(|m| m.model_id.clone())).collect();
        assert_eq!(added, vec!["c".to_string()]);
        assert_eq!(removed, vec!["a".to_string()]);

        // 第三次：baseline 已={b,c}、current={b,c} → 无变化
        let d3 = get_model_changes(&pool, "p").await.unwrap();
        assert!(d3.is_empty());
    }

    async fn insert_up(pool: &SqlitePool, pid: &str, ct: &str, rows: &[(&str, &str)]) {
        for (id, name) in rows {
            sqlx::query(
                "INSERT INTO upstream_models (provider_id, channel_type, model_id, model_name) VALUES (?, ?, ?, ?)",
            )
            .bind(pid).bind(ct).bind(id).bind(name)
            .execute(pool).await.unwrap();
        }
    }
```

- [ ] **Step 2: 跑测试，确认红**

Run: `cargo test model_changes_returns_drift`
Expected: 编译失败，`cannot find function 'get_model_changes'`。

- [ ] **Step 3: 实现 DB 存取与 `get_model_changes`**

在 `src/admin/provider_svc.rs`（内部辅助函数区）追加：
```rust
async fn fetch_current_snapshot(pool: &SqlitePool, provider_id: &str) -> anyhow::Result<Vec<UpstreamModelRow>> {
    Ok(sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}

async fn fetch_baseline_snapshot(pool: &SqlitePool, provider_id: &str) -> anyhow::Result<Vec<UpstreamModelRow>> {
    Ok(sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models_seen WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}

/// 落地 baseline := 当前 upstream_models（per provider，事务）
async fn land_baseline(pool: &SqlitePool, provider_id: &str) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM upstream_models_seen WHERE provider_id = ?")
        .bind(provider_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO upstream_models_seen (provider_id, channel_type, model_id, model_name)
         SELECT provider_id, channel_type, model_id, model_name FROM upstream_models WHERE provider_id = ?",
    )
    .bind(provider_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// 计算并返回 drift（current vs 旧 baseline），然后落地 baseline（打开即清零）。
/// 首看防灌水：baseline 为空时不报全量新增，仅落地。
pub async fn get_model_changes(pool: &SqlitePool, provider_id: &str) -> anyhow::Result<Vec<ChannelDrift>> {
    let current = fetch_current_snapshot(pool, provider_id).await?;
    let baseline = fetch_baseline_snapshot(pool, provider_id).await?;
    if baseline.is_empty() {
        land_baseline(pool, provider_id).await?;
        return Ok(Vec::new());
    }
    let drift = compute_drift(&current, &baseline);
    land_baseline(pool, provider_id).await?;
    Ok(drift)
}
```

- [ ] **Step 4: 跑测试，确认绿**

Run: `cargo test model_changes_returns_drift`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src/admin/provider_svc.rs
git commit -m "feat(provider_svc): get_model_changes 计算漂移并落地 baseline + 集成测试"
```

---

## Task 7: `probe_upstream_models` + `replace_upstream_snapshot`

**Files:**
- Modify: `src/admin/provider_svc.rs`

- [ ] **Step 1: 实现快照替换与探测主循环**

在 `src/admin/provider_svc.rs`（内部辅助函数区）追加：
```rust
/// 用本轮探测结果整体替换某 (provider, channel) 的上游快照（事务）。
async fn replace_upstream_snapshot(
    pool: &SqlitePool,
    provider_id: &str,
    channel_type: &str,
    models: &[(String, String)],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM upstream_models WHERE provider_id = ? AND channel_type = ?")
        .bind(provider_id).bind(channel_type).execute(&mut *tx).await?;
    for (id, name) in models {
        sqlx::query(
            "INSERT INTO upstream_models (provider_id, channel_type, model_id, model_name) VALUES (?, ?, ?, ?)",
        )
        .bind(provider_id).bind(channel_type).bind(id).bind(name)
        .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 后台探测上游 /v1/models，刷新 upstream_models 快照。失败保留旧快照（仅 warn）。
pub async fn probe_upstream_models(state: &Arc<AppState>) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct CfgRow { provider_id: String, is_enabled: bool, api_key: String }
    #[derive(sqlx::FromRow)]
    struct ChCfgRow { provider_id: String, channel_type: String, is_enabled: bool }

    let cfgs: Vec<CfgRow> = sqlx::query_as::<_, CfgRow>(
        "SELECT provider_id, is_enabled, api_key FROM provider_config",
    )
    .fetch_all(&state.db).await?;
    let ch_cfgs: Vec<ChCfgRow> = sqlx::query_as::<_, ChCfgRow>(
        "SELECT provider_id, channel_type, is_enabled FROM provider_channel_config",
    )
    .fetch_all(&state.db).await?;

    let mut provider_enabled: HashMap<String, bool> = HashMap::new();
    let mut provider_api_key: HashMap<String, String> = HashMap::new();
    for c in &cfgs {
        provider_enabled.insert(c.provider_id.clone(), c.is_enabled);
        provider_api_key.insert(c.provider_id.clone(), c.api_key.clone());
    }
    let mut channel_enabled: HashMap<(String, String), bool> = HashMap::new();
    for c in &ch_cfgs {
        channel_enabled.insert((c.provider_id.clone(), c.channel_type.clone()), c.is_enabled);
    }

    let targets = select_probe_targets(
        &state.provider_defs, &provider_enabled, &provider_api_key, &channel_enabled,
    );

    for t in targets {
        match fetch_models_from_api(&state.client, &state.provider_defs, &t.provider_id, &t.channel_type, &t.api_key).await {
            Ok(models) => {
                if let Err(e) = replace_upstream_snapshot(&state.db, &t.provider_id, &t.channel_type, &models).await {
                    tracing::warn!("upstream snapshot persist failed for '{}' '{}': {}", t.provider_id, t.channel_type, e);
                }
            }
            Err(e) => tracing::warn!("upstream probe failed for '{}' '{}': {}", t.provider_id, t.channel_type, e),
        }
    }
    Ok(())
}
```

- [ ] **Step 2: 验证编译 + 全量测试 + clippy**

Run: `cargo test && cargo clippy -- -D warnings`
Expected: 所有测试 PASS；clippy 无新增告警。

- [ ] **Step 3: 手动冒烟（启动即探测，因 `tokio::time::interval` 首次 tick 立即触发）**

Run: `cargo run`（另开终端），观察日志。
Expected: 启动后日志出现对各已启用+已配 key 通道的探测（成功无日志，失败有 `upstream probe failed for ...` warn）。Ctrl-C 退出。

- [ ] **Step 4: Commit**

```bash
git add src/admin/provider_svc.rs
git commit -m "feat(provider_svc): probe_upstream_models 后台探测并刷新上游快照"
```

---

## Task 8: `list_providers` 注入 drift 计数

**Files:**
- Modify: `src/admin/provider_svc.rs::list_providers`

- [ ] **Step 1: 在 `list_providers` 内一次性载入两组快照并按 provider 分组**

把 `list_providers` 函数体开头（`let mut result = Vec::new();` 之后）插入：
```rust
    // 漂移计数：一次性载入上游当前快照与 baseline，按 provider 分组算对称差（避免 N+1）
    let current_all: Vec<UpstreamModelRow> = sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models",
    )
    .fetch_all(pool).await?;
    let baseline_all: Vec<UpstreamModelRow> = sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models_seen",
    )
    .fetch_all(pool).await?;
    let mut cur_by_prov: HashMap<String, Vec<UpstreamModelRow>> = HashMap::new();
    for r in current_all { cur_by_prov.entry(r.provider_id.clone()).or_default().push(r); }
    let mut base_by_prov: HashMap<String, Vec<UpstreamModelRow>> = HashMap::new();
    for r in baseline_all { base_by_prov.entry(r.provider_id.clone()).or_default().push(r); }
```

- [ ] **Step 2: 在构造 `ProviderSummary` 处填入真实 drift**

把 Task 3 临时写入的：
```rust
        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            channels,
            drift: None,
        });
```
改为：
```rust
        let drift = {
            let cur = cur_by_prov.get(&def.id).map(|v| v.as_slice()).unwrap_or(&[]);
            let base = base_by_prov.get(&def.id).map(|v| v.as_slice()).unwrap_or(&[]);
            if base.is_empty() {
                None // 从未打开过变更弹窗 → 不报角标
            } else {
                let d = compute_drift(cur, base);
                let new = d.iter().map(|c| c.added.len() as i64).sum();
                let removed = d.iter().map(|c| c.removed.len() as i64).sum();
                Some(DriftSummary { new, removed })
            }
        };
        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            channels,
            drift,
        });
```

- [ ] **Step 3: 验证编译 + 测试**

Run: `cargo check && cargo test`
Expected: 编译通过，测试 PASS。

- [ ] **Step 4: 手动验证 API**

Run: `cargo run`，另开终端 `curl -s localhost:10020/api/admin/providers | python3 -m json.tool | head -40`
Expected: 每个 provider 对象含 `drift: {new, removed}` 或 `drift` 缺省（未查看过）。Ctrl-C 退出。

- [ ] **Step 5: Commit**

```bash
git add src/admin/provider_svc.rs
git commit -m "feat(provider_svc): list_providers 注入 drift 计数"
```

---

## Task 9: `model_changes` handler + 路由注册

**Files:**
- Modify: `src/router/admin.rs`（新 handler）, `src/router/mod.rs`（注册路由）

- [ ] **Step 1: 在 `src/router/admin.rs` 加 handler**

在 `fetch_provider_models` 之后追加：
```rust
/// 返回该 provider 自上次查看以来的上游变更明细，并落地 baseline（打开即清零角标）。
pub async fn model_changes(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match provider_svc::get_model_changes(&state.db, &id).await {
        Ok(channels) => Json(serde_json::json!({ "channels": channels })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
```

- [ ] **Step 2: 在 `src/router/mod.rs` 注册路由**

把 `create_admin_router` 中：
```rust
        .route(
            "/providers/{id}",
            axum::routing::get(admin::get_provider)
                .put(admin::update_provider),
        )
        .route(
            "/providers/{id}/fetch-models",
            axum::routing::get(admin::fetch_provider_models),
        )
```
改为（在 fetch-models 后追加一行 route）：
```rust
        .route(
            "/providers/{id}",
            axum::routing::get(admin::get_provider)
                .put(admin::update_provider),
        )
        .route(
            "/providers/{id}/fetch-models",
            axum::routing::get(admin::fetch_provider_models),
        )
        .route(
            "/providers/{id}/model-changes",
            axum::routing::get(admin::model_changes),
        )
```

- [ ] **Step 3: 验证编译**

Run: `cargo check`
Expected: 编译通过。

- [ ] **Step 4: 手动验证 API**

Run: `cargo run`，另开终端：`curl -s localhost:10020/api/admin/providers/openai/model-changes | python3 -m json.tool`
Expected: 返回 `{"channels":[{"channel_type":"...","added":[...],"removed":[...]}]}`（首看返回空 channels 但已落地 baseline；再 curl 一次若无变化仍空）。Ctrl-C 退出。

- [ ] **Step 5: Commit**

```bash
git add src/router/admin.rs src/router/mod.rs
git commit -m "feat(router): GET /providers/{id}/model-changes 端点"
```

---

## Task 10: `main.rs` 启动独立探测任务

**Files:**
- Modify: `src/main.rs:132`（在现有 refresh spawn 之后）

- [ ] **Step 1: 加独立 spawn**

在 `src/main.rs` 现有 `tokio::spawn`（refresh 任务）闭合的 `});` 之后、`// 构建路由` 之前插入：
```rust
    // 启动后台上游模型探测（独立节奏，默认 1 天）。tokio::time::interval 首次 tick 立即触发→启动即播种快照。
    let probe_state = state.clone();
    let probe_interval_min = app_config.bridge.probe_interval_min.max(1);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(probe_interval_min * 60));
        loop {
            interval.tick().await;
            if let Err(e) = admin::provider_svc::probe_upstream_models(&probe_state).await {
                tracing::error!("Scheduled upstream probe failed: {}", e);
            }
        }
    });
```

- [ ] **Step 2: 验证构建 + 启动**

Run: `cargo build && cargo run`
Expected: 构建成功；启动日志后约 10s 内出现探测相关 warn（对未配 key 的 provider）或无报错（对已配 key 的成功探测，静默）。Ctrl-C 退出。

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): 启动独立上游探测定时任务"
```

---

## Task 11: `model-bridge.toml.example` 加配置项

**Files:**
- Modify: `model-bridge.toml.example:29-31`

- [ ] **Step 1: 在 `[bridge]` 段加字段**

把：
```toml
[bridge]
# 后台自动刷新模型列表间隔（分钟）
refresh_interval_min = 10
```
改为：
```toml
[bridge]
# 后台自动刷新模型列表间隔（分钟）
refresh_interval_min = 10
# 后台探测上游 /v1/models 的间隔（分钟），与路由刷新解耦。
# 模型目录变化是天/周级，默认 1440（1 天）。探测成功才更新快照；失败保留旧快照。
probe_interval_min = 1440
```

- [ ] **Step 2: Commit**

```bash
git add model-bridge.toml.example
git commit -m "docs(config): toml 模板加 probe_interval_min"
```

---

## Task 12: 前端 — 卡片角标 + 只读"上游变更"弹窗

**Files:**
- Modify: `web/src/views/Providers.vue`

> 前端无测试框架，验证靠 `npm run build` + 手动点测。

- [ ] **Step 1: `ProviderSummary` 接口加 `drift`**

把 `web/src/views/Providers.vue` 的 `<script setup>` 中：
```ts
interface ProviderSummary { id: string; name: string; icon?: string; is_enabled: boolean; channels: ChannelInfo[] }
```
改为：
```ts
interface DriftSummary { new: number; removed: number }
interface ProviderSummary { id: string; name: string; icon?: string; is_enabled: boolean; channels: ChannelInfo[]; drift?: DriftSummary }
interface ChangeEntry { model_id: string; model_name: string }
interface ChannelChange { channel_type: string; added: ChangeEntry[]; removed: ChangeEntry[] }
```

- [ ] **Step 2: 加响应式状态**

在 `const diffResult = ref<DiffResult>(...)` 附近追加：
```ts
const showChanges = ref(false)
const changesProvider = ref<ProviderSummary | null>(null)
const changesData = ref<ChannelChange[]>([])
const changesLoading = ref(false)
```

- [ ] **Step 3: 加 `openChanges` 函数**

在 `fetchModels` 函数附近追加：
```ts
async function openChanges(p: ProviderSummary) {
  changesProvider.value = p
  showChanges.value = true
  changesLoading.value = true
  try {
    const res = await fetch(`${API_BASE}/providers/${p.id}/model-changes`)
    if (res.ok) {
      const data = await res.json()
      changesData.value = (data.channels || []) as ChannelChange[]
    } else {
      message.error('获取上游变更失败')
      changesData.value = []
    }
  } finally {
    changesLoading.value = false
  }
}
```

- [ ] **Step 4: 卡片 `card-meta` 内加角标**

把模板中：
```html
                <span class="meta-sep">·</span>
                <span>{{ enabledModelTotal(p) }} 模型</span>
              </div>
```
改为：
```html
                <span class="meta-sep">·</span>
                <span>{{ enabledModelTotal(p) }} 模型</span>
                <span
                  v-if="p.drift && (p.drift.new > 0 || p.drift.removed > 0)"
                  class="drift-badge mono"
                  role="button"
                  tabindex="0"
                  :title="`自上次查看：${p.drift!.new} 新增 / ${p.drift!.removed} 下架，点击查看`"
                  @click.stop="openChanges(p)"
                  @keydown.enter.prevent="openChanges(p)"
                >
                  <span v-if="p.drift!.new" class="d-add">✚{{ p.drift!.new }}</span>
                  <span v-if="p.drift!.removed" class="d-rem">✖{{ p.drift!.removed }}</span>
                </span>
              </div>
```

- [ ] **Step 5: 加只读变更弹窗**

在现有 `showCloseConfirm` 的 `<n-modal>` 之后追加：
```html
    <n-modal
      v-model:show="showChanges"
      :title="`${changesProvider?.name} · 上游变更`"
      style="width: 560px"
      preset="card"
      class="changes-modal"
      @close="loadProviders"
    >
      <n-spin :show="changesLoading">
        <div v-if="!changesData.length" class="changes-empty mono">自上次查看以来无变化</div>
        <div v-for="ch in changesData" :key="ch.channel_type" class="changes-channel">
          <div class="changes-channel-label mono">{{ channelLabel(ch.channel_type) }}</div>
          <div v-if="ch.added.length" class="changes-group added">
            <span class="changes-group-label">✚ 新增 ({{ ch.added.length }})</span>
            <div v-for="(m, i) in ch.added" :key="'a' + i" class="changes-row mono">{{ m.model_id }}</div>
          </div>
          <div v-if="ch.removed.length" class="changes-group removed">
            <span class="changes-group-label">✖ 下架 ({{ ch.removed.length }})</span>
            <div v-for="(m, i) in ch.removed" :key="'r' + i" class="changes-row mono">{{ m.model_id }}</div>
          </div>
        </div>
      </n-spin>
      <template #footer>
        <n-space justify="end"><n-button @click="showChanges = false">关闭</n-button></n-space>
      </template>
    </n-modal>
```

- [ ] **Step 6: 加样式（scoped 末尾，`@media` 之前）**

```css
/* ---- drift badge + changes modal ---- */
.drift-badge { display: inline-flex; gap: 5px; align-items: center; padding: 1px 7px; border-radius: 999px; background: #eef7f0; border: 1px solid #cbe6d3; font-size: 10px; font-weight: 600; cursor: pointer; line-height: 1.5; transition: background 0.15s; }
.drift-badge:hover { background: #dcefe1; }
.drift-badge:focus-visible { outline: 2px solid rgba(46,168,106,0.5); outline-offset: 1px; }
.d-add { color: #1d7a4c; }
.d-rem { color: #b3261e; }
.changes-modal { --n-title-text-color: #17140f; }
.changes-empty { color: #a89e8c; text-align: center; padding: 24px 0; font-size: 13px; }
.changes-channel { border: 1px solid #e5dccd; border-radius: 10px; padding: 10px 12px; margin-bottom: 10px; }
.changes-channel:last-child { margin-bottom: 0; }
.changes-channel-label { font-size: 11px; font-weight: 600; color: #8c7f6c; text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 8px; }
.changes-group { margin-bottom: 8px; }
.changes-group:last-child { margin-bottom: 0; }
.changes-group-label { display: block; font-size: 12px; font-weight: 600; margin-bottom: 4px; }
.changes-group.added .changes-group-label { color: #1d7a4c; }
.changes-group.removed .changes-group-label { color: #b3261e; }
.changes-row { font-size: 13px; color: #4b443a; padding: 2px 0 2px 12px; }
```

- [ ] **Step 7: 构建前端**

Run: `cd web && npm run build`
Expected: 构建成功，产出 `web/dist/`。

- [ ] **Step 8: 重建二进制以嵌入新前端 + 手动点测**

Run: `cd .. && cargo build && cargo run`
点测：
1. 打开 `http://localhost:10020`，进供应商页。
2. 若某 provider 有上游变化（可先 `curl .../model-changes` 制造 baseline 再等探测，或直接观察已配 key 的 provider），其卡片 `card-meta` 出现 `✚N`/`✖N` 角标。
3. 点角标 → 弹"上游变更"弹窗，按通道列新增/下架；关闭弹窗（`@close` 触发 `loadProviders`）→ 角标清零。
4. 无变化 provider 不显角标。

Expected: 上述行为符合；关闭弹窗后角标消失。Ctrl-C 退出。

- [ ] **Step 9: Commit**

```bash
git add web/src/views/Providers.vue web/dist
git commit -m "feat(web): 供应商卡片上游变更角标 + 只读变更弹窗"
```

---

## Self-Review

**1. Spec coverage（逐条对照 spec）：**
- 自动后台探测、复用 `fetch_models_from_api`、独立 `probe_interval_min`(1440/天) → Task 1/7/10 ✓
- 感知 新增+下架、跳过改名 → `compute_drift` + 测试 `rename_only_is_ignored` ✓
- 基准 = baseline（非 `provider_models`）；`provider_models` 不进 diff → `compute_drift` 仅吃 `upstream_models`/`upstream_models_seen`，`list_providers` drift 也只读快照表 ✓
- 打开变更弹窗落地 baseline、角标清零 → `get_model_changes` 落地 + 前端 `@close="loadProviders"` ✓
- 首看防灌水 → `get_model_changes` baseline 空分支 + `list_providers` `base.is_empty()` ✓
- 探测失败保留旧快照、仅 warn → `probe_upstream_models` 失败分支不调 `replace_upstream_snapshot` ✓
- 卡片角标 + 只读弹窗、无采纳流程、采纳仍走配置弹窗 → Task 12（弹窗只读，无采纳按钮）✓
- 两张表迁移 → Task 2 ✓
- `GET /providers` drift 计数 + `GET /providers/{id}/model-changes` → Task 8/9 ✓
- `model-bridge.toml.example` → Task 11 ✓

**2. Placeholder scan:** 无 TBD/TODO/"add error handling"；每步均有完整代码与命令。✓

**3. Type consistency:** `UpstreamModelRow`/`ModelEntry`/`ChannelDrift`/`DriftSummary` 在 Task 3 定义，Task 4/6/8 一致引用；`ProbeTarget` Task 5 定义、Task 7 引用一致；`compute_drift`/`select_probe_targets`/`get_model_changes`/`probe_upstream_models` 签名跨任务一致；前端 `DriftSummary`/`ChannelChange`/`ChangeEntry` 与后端 JSON 字段（`new`/`removed`/`channel_type`/`added`/`removed`/`model_id`）对齐。✓

无遗漏。
