use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{ChannelDef, ProviderDef};
use crate::db::models::{
    ChannelDetail, ChannelDrift, DriftSummary, ModelEntry, ProviderChannelConfigRow, ProviderConfigRow,
    ProviderDetail, ProviderModel, ProviderSummary, UpstreamModelRow,
};
use crate::state::{AppState, ProviderRoute};

/// 合并 Provider 定义与用户 DB 覆盖，返回摘要列表
pub async fn list_providers(
    pool: &SqlitePool,
    defs: &[ProviderDef],
) -> anyhow::Result<Vec<ProviderSummary>> {
    let mut result = Vec::new();
    for def in defs {
        let config = get_provider_config(pool, &def.id).await;
        let channel_configs = get_channel_configs(pool, &def.id).await;

        let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
        let mut channels = merge_channels(&def.channels, &channel_configs);

        // 按通道统计模型数：UNIQUE(provider_id, channel_type, model_id) 保证同一通道内无重复，
        // 故每通道 COUNT(*) 即该通道去重后的模型数；卡片据此「区分通道」展示。
        let counts: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
            "SELECT channel_type, COUNT(*) FROM provider_models WHERE provider_id = ? GROUP BY channel_type",
        )
        .bind(&def.id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
        for ch in &mut channels {
            ch.model_count = *counts.get(&ch.channel_type).unwrap_or(&0);
        }

        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            channels,
            drift: None,
        });
    }
    Ok(result)
}

/// 合并 Provider 定义与用户 DB 覆盖，返回详情
pub async fn get_provider(
    pool: &SqlitePool,
    defs: &[ProviderDef],
    id: &str,
) -> anyhow::Result<Option<ProviderDetail>> {
    let Some(def) = defs.iter().find(|d| d.id == id) else {
        return Ok(None);
    };

    let config = get_provider_config(pool, id).await;
    let channel_configs = get_channel_configs(pool, id).await;

    let api_key = config.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
    let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
    let mut channels = merge_channels(&def.channels, &channel_configs);

    let models = sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, channel_type, model_id, model_name FROM provider_models WHERE provider_id = ? ORDER BY model_id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    // 与列表卡片一致：按通道去重计数。
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for m in &models {
        *counts.entry(m.channel_type.as_str()).or_insert(0) += 1;
    }
    for ch in &mut channels {
        ch.model_count = *counts.get(ch.channel_type.as_str()).unwrap_or(&0);
    }

    Ok(Some(ProviderDetail {
        id: def.id.clone(),
        name: def.name.clone(),
        icon: def.icon.clone(),
        api_key,
        is_enabled,
        channels,
        models,
    }))
}

/// 从 DB 加载所有 enabled providers，构建路由表
pub async fn refresh_routes(state: &Arc<AppState>) -> anyhow::Result<()> {
    let mut openai_chat_routes: HashMap<String, ProviderRoute> = HashMap::new();
    let mut openai_responses_routes: HashMap<String, ProviderRoute> = HashMap::new();
    let mut anthropic_routes: HashMap<String, ProviderRoute> = HashMap::new();

    for def in &state.provider_defs {
        let config = get_provider_config(&state.db, &def.id).await;
        let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
        if !is_enabled {
            continue;
        }
        let api_key = config.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
        if api_key.is_empty() {
            continue;
        }

        let channel_configs = get_channel_configs(&state.db, &def.id).await;
        let channels = merge_channels(&def.channels, &channel_configs);

        let models = sqlx::query_as::<_, ProviderModel>(
            "SELECT id, provider_id, channel_type, model_id, model_name FROM provider_models WHERE provider_id = ?",
        )
        .bind(&def.id)
        .fetch_all(&state.db)
        .await?;

        // 拒绝非 http(s) 的 base_url（file:// 等），避免被导向本地资源
        for c in channels
            .iter()
            .filter(|c| c.is_enabled && !is_safe_base_url(&c.base_url))
        {
            tracing::warn!(
                "provider '{}' channel '{}' base_url '{}' is not http(s), excluded from routing",
                def.id, c.channel_type, c.base_url
            );
        }
        let enabled: Vec<&ChannelDetail> = channels
            .iter()
            .filter(|c| c.is_enabled && is_safe_base_url(&c.base_url))
            .collect();

        // 模型按通道隔离：channel_type 为空（迁移残留）的跳过路由并告警
        for m in models.iter().filter(|m| m.channel_type.is_empty()) {
            tracing::warn!(
                "provider '{}' model '{}' has empty channel_type, skipped from routing",
                def.id, m.model_id
            );
        }

        // ---- anthropic 路由：每个启用的 anthropic 通道，插入「归属该通道」的模型 ----
        // 检索 key 由 model_id 派生（剥 [1m] 后缀；非 claude/anthropic 开头的补 claude- 前缀），
        // 与 proxy 转发剥除 [1m] 的逻辑配套。
        for ch in enabled.iter().copied().filter(|c| c.channel_type == "anthropic") {
            for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let route = ProviderRoute {
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                };
                let lower = route.model_id.to_lowercase();
                let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
                let key = if clean.starts_with("claude") || clean.starts_with("anthropic") {
                    clean.to_string()
                } else {
                    format!("claude-{}", clean)
                };
                anthropic_routes.insert(key, route);
            }
        }

        // ---- openai 路由：chat 与 responses 各自独立建表，不再合并 ----
        // 每个启用的 openai 通道单独成一张路由表：openai_chat → openai_chat_routes，
        // openai_responses → openai_responses_routes。模型归属哪个通道就进哪张表，转发用该通道 base_url，
        // 无需按 path 过滤。跨 provider 同名 model_id 冲突时保留先入者并告警。
        use std::collections::hash_map::Entry;
        for ch in enabled.iter().copied().filter(|c| c.channel_type != "anthropic") {
            let table: &mut HashMap<String, ProviderRoute> = match ch.channel_type.as_str() {
                "openai_chat" => &mut openai_chat_routes,
                "openai_responses" => &mut openai_responses_routes,
                other => {
                    tracing::warn!(
                        "provider '{}' channel '{}' has unknown openai channel type, skipped from routing",
                        def.id, other
                    );
                    continue;
                }
            };
            for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let key = model.model_id.to_lowercase();
                let route = ProviderRoute {
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                };
                match table.entry(key) {
                    Entry::Vacant(v) => {
                        v.insert(route);
                    }
                    Entry::Occupied(o) => {
                        let existing = o.get();
                        tracing::warn!(
                            "model '{}' on '{}' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                            model.model_id, ch.channel_type, existing.provider_id, existing.base_url, def.id
                        );
                    }
                }
            }
        }
    }

    {
        let mut c = state.openai_chat_routes.write().await;
        *c = openai_chat_routes;
    }
    {
        let mut r = state.openai_responses_routes.write().await;
        *r = openai_responses_routes;
    }
    {
        let mut a = state.anthropic_routes.write().await;
        *a = anthropic_routes;
    }

    tracing::info!(
        "Routes refreshed: {} chat models, {} responses models, {} anthropic models",
        state.openai_chat_routes.read().await.len(),
        state.openai_responses_routes.read().await.len(),
        state.anthropic_routes.read().await.len()
    );

    Ok(())
}

/// 更新 provider 用户配置（api_key, is_enabled, channels, models）
pub async fn update_provider(
    pool: &SqlitePool,
    id: &str,
    api_key: &str,
    is_enabled: bool,
    channels: &[(String, bool)], // (channel_type, is_enabled)
    models: &[(String, String, String)], // (channel_type, model_id, model_name)
) -> anyhow::Result<()> {
    // upsert provider_config
    sqlx::query(
        "INSERT INTO provider_config (provider_id, api_key, is_enabled) VALUES (?, ?, ?)
         ON CONFLICT(provider_id) DO UPDATE SET api_key = excluded.api_key, is_enabled = excluded.is_enabled",
    )
    .bind(id)
    .bind(api_key)
    .bind(is_enabled as i32)
    .execute(pool)
    .await?;

    // upsert channel_configs：base_url 以配置文件为准、不持久化，仅存 channel 启用状态
    for (channel_type, enabled) in channels {
        sqlx::query(
            "INSERT INTO provider_channel_config (provider_id, channel_type, is_enabled) VALUES (?, ?, ?)
             ON CONFLICT(provider_id, channel_type) DO UPDATE SET is_enabled = excluded.is_enabled",
        )
        .bind(id)
        .bind(channel_type)
        .bind(*enabled as i32)
        .execute(pool)
        .await?;
    }

    // 替换模型列表（按通道）
    sqlx::query("DELETE FROM provider_models WHERE provider_id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    for (channel_type, model_id, model_name) in models {
        if model_id.is_empty() {
            continue;
        }
        let mid = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, channel_type, model_id, model_name) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&mid)
        .bind(id)
        .bind(channel_type)
        .bind(model_id)
        .bind(model_name)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// 从指定通道的 models_endpoint 拉取模型列表（不写入 DB），使用前端传入的 api_key
pub async fn fetch_models_from_api(
    client: &reqwest::Client,
    defs: &[ProviderDef],
    provider_id: &str,
    channel_type: &str,
    ui_api_key: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let Some(def) = defs.iter().find(|d| d.id == provider_id) else {
        anyhow::bail!("provider not found");
    };
    let Some(ch) = def.channels.iter().find(|c| c.channel_type == channel_type) else {
        anyhow::bail!("channel '{}' not found on provider '{}'", channel_type, provider_id);
    };
    let endpoint = ch
        .models_endpoint
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("models_endpoint not configured for channel '{}'", channel_type))?;

    // 使用前端传入的 key
    let api_key = if ui_api_key.is_empty() {
        anyhow::bail!("api_key not configured");
    } else {
        ui_api_key.to_string()
    };

    let mut req = client.get(endpoint).timeout(std::time::Duration::from_secs(10));

    // 鉴权方式按通道类型推导：anthropic 通道用 x-api-key（Anthropic 约定），
    // 其余（openai_chat / openai_responses）用 Bearer。
    match ch.channel_type.as_str() {
        "anthropic" => {
            req = req
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01");
        }
        _ => {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }
    }

    let resp = req.send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    let models: Vec<(String, String)> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| {
                    let id = m["id"].as_str().unwrap_or("").to_string();
                    let name = m["display_name"].as_str().unwrap_or(&id).to_string();
                    (id, name)
                })
                .collect()
        })
        .unwrap_or_default();

    if models.is_empty() {
        anyhow::bail!("empty model list");
    }

    Ok(models)
}

/// 迁移辅助：把 schema 迁移后 channel_type 为空的 provider_models 行按启发式回填。
/// claude/anthropic 前缀或含 [1M] 的归该 provider 的 anthropic 通道；
/// 其余归首个非 anthropic 通道；都不匹配则归首个通道。
/// 仅迁移期使用，正常运行无空值不触发。
pub async fn backfill_model_channels(pool: &SqlitePool, defs: &[ProviderDef]) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct Row {
        provider_id: String,
        model_id: String,
    }
    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT provider_id, model_id FROM provider_models WHERE channel_type = '' OR channel_type IS NULL",
    )
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    for Row {
        provider_id,
        model_id,
    } in rows
    {
        let lower = model_id.to_lowercase();
        let anthropic_style = lower.starts_with("claude")
            || lower.starts_with("anthropic")
            || lower.contains("[1m]");
        let target = defs
            .iter()
            .find(|d| d.id == provider_id)
            .and_then(|def| {
                if anthropic_style {
                    def.channels
                        .iter()
                        .find(|c| c.channel_type == "anthropic")
                        .map(|c| c.channel_type.clone())
                } else {
                    def.channels
                        .iter()
                        .find(|c| c.channel_type != "anthropic")
                        .map(|c| c.channel_type.clone())
                }
            })
            .or_else(|| {
                defs.iter()
                    .find(|d| d.id == provider_id)
                    .and_then(|d| d.channels.first().map(|c| c.channel_type.clone()))
            });
        let Some(target) = target else {
            tracing::warn!(
                "backfill: provider '{}' has no channels, model '{}' left unmapped",
                provider_id,
                model_id
            );
            continue;
        };
        sqlx::query("UPDATE provider_models SET channel_type = ? WHERE provider_id = ? AND model_id = ?")
            .bind(&target)
            .bind(&provider_id)
            .bind(&model_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

// ===== 内部辅助函数 =====

async fn get_provider_config(pool: &SqlitePool, id: &str) -> Option<ProviderConfigRow> {
    sqlx::query_as::<_, ProviderConfigRow>(
        "SELECT provider_id, api_key, is_enabled FROM provider_config WHERE provider_id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

async fn get_channel_configs(
    pool: &SqlitePool,
    provider_id: &str,
) -> Vec<ProviderChannelConfigRow> {
    sqlx::query_as::<_, ProviderChannelConfigRow>(
        "SELECT provider_id, channel_type, is_enabled FROM provider_channel_config WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// 合并配置定义与 DB 覆盖：base_url / models_endpoint 一律以配置文件定义为准
/// （不允许 DB 自定义），仅 is_enabled 取 DB 覆盖值。
fn merge_channels(
    defs: &[ChannelDef],
    configs: &[ProviderChannelConfigRow],
) -> Vec<ChannelDetail> {
    defs.iter()
        .map(|def| {
            let is_enabled = configs
                .iter()
                .find(|c| c.channel_type == def.channel_type)
                .map(|c| c.is_enabled)
                .unwrap_or(true);
            ChannelDetail {
                channel_type: def.channel_type.clone(),
                base_url: def.base_url.clone(),
                models_endpoint: def.models_endpoint.clone(),
                is_enabled,
                model_count: 0,
            }
        })
        .collect()
}

/// 仅允许 http(s) 的 base_url，拒绝 file:// 等本地协议及非 URL 字符串。
pub fn is_safe_base_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// 对称差：current 相对 baseline 的新增（current 有、baseline 无）与下架（baseline 有、current 无）。
/// 按 (channel_type, model_id 小写) 比对——同通道大小写不敏感、跨通道不误配。model_name 变化不算（跳过改名）。
pub fn compute_drift(current: &[UpstreamModelRow], baseline: &[UpstreamModelRow]) -> Vec<ChannelDrift> {
    use std::collections::{HashMap, HashSet};
    let key = |r: &UpstreamModelRow| (r.channel_type.clone(), r.model_id.to_lowercase());
    let base_keys: HashSet<(String, String)> = baseline.iter().map(|r| key(r)).collect();
    let cur_keys: HashSet<(String, String)> = current.iter().map(|r| key(r)).collect();

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

#[cfg(test)]
mod drift_tests {
    use super::*;
    use crate::config::{ChannelDef, ProviderDef};
    use crate::db::models::{ChannelDrift, ModelEntry, UpstreamModelRow};

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
            vec![("anthropic".to_string(), 1, 0), ("openai_chat".to_string(), 0, 1)]
        );
    }

    #[test]
    fn rename_only_is_ignored() {
        // 同 model_id、不同 model_name → 既不在 added 也不在 removed（跳过改名）
        let cur = vec![row("openai_chat", "glm-5.2", "new name")];
        let base = vec![row("openai_chat", "glm-5.2", "old name")];
        assert_eq!(compute_drift(&cur, &base), vec![]);
    }

    fn chan(ct: &str, base: &str, ep: Option<&str>) -> ChannelDef {
        ChannelDef { channel_type: ct.into(), base_url: base.into(), models_endpoint: ep.map(String::from) }
    }
    fn def(id: &str, chans: Vec<ChannelDef>) -> ProviderDef {
        ProviderDef { id: id.into(), name: id.into(), icon: None, channels: chans }
    }

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
            chan("openai_chat", "https://x/v1", Some("https://x/v1/models")),       // 命中
            chan("openai_responses", "https://x/v1", Some("https://x/v1/models2")), // 通道关
            chan("anthropic", "https://x/v1", None),                                // 无 endpoint
            chan("openai_chat2", "https://x/v1", Some("file:///etc/passwd")),       // 非 http
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
}
