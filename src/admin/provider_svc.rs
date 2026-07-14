use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{ChannelDef, ProviderDef};
use crate::db::models::{
    ChannelDetail, ProviderChannelConfigRow, ProviderConfigRow, ProviderDetail, ProviderModel,
    ProviderSummary,
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
