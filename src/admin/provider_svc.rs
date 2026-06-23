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
        let channels = merge_channels(&def.channels, &channel_configs);

        let model_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM provider_models WHERE provider_id = ?")
                .bind(&def.id)
                .fetch_one(pool)
                .await?;

        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            is_enabled,
            models_endpoint: def.models_endpoint.clone(),
            channels,
            model_count: model_count.0,
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
    let channels = merge_channels(&def.channels, &channel_configs);

    let models = sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, model_id, model_name FROM provider_models WHERE provider_id = ? ORDER BY model_id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    Ok(Some(ProviderDetail {
        id: def.id.clone(),
        name: def.name.clone(),
        icon: def.icon.clone(),
        api_key,
        is_enabled,
        models_endpoint: def.models_endpoint.clone(),
        channels,
        models,
    }))
}

/// 从 DB 加载所有 enabled providers，构建路由表
pub async fn refresh_routes(state: &Arc<AppState>) -> anyhow::Result<()> {
    let mut openai_routes: HashMap<String, ProviderRoute> = HashMap::new();
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
            "SELECT id, provider_id, model_id, model_name FROM provider_models WHERE provider_id = ?",
        )
        .bind(&def.id)
        .fetch_all(&state.db)
        .await?;

        // 分离启用的 anthropic channel 与 openai channel。
        // openai_chat / openai_responses 同一 provider 下 base_url 相同，合并为一条 route，
        // channels 字段记录该 provider 实际启用了哪些 openai channel type，供 proxy 按请求 path 过滤。
        let enabled: Vec<&ChannelDetail> = channels.iter().filter(|c| c.is_enabled).collect();
        let openai_channels: Vec<&ChannelDetail> = enabled
            .iter()
            .copied()
            .filter(|c| c.channel_type != "anthropic")
            .collect();
        let anthropic_channels: Vec<&ChannelDetail> = enabled
            .iter()
            .copied()
            .filter(|c| c.channel_type == "anthropic")
            .collect();

        let openai_channel_types: Vec<String> = openai_channels
            .iter()
            .map(|c| c.channel_type.clone())
            .collect();

        for model in &models {
            // openai route：provider 至少启用一个 openai channel 才插入；base_url 取首个 openai channel
            if let Some(first_openai) = openai_channels.first() {
                let route = ProviderRoute {
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: first_openai.base_url.clone(),
                    api_key: api_key.clone(),
                    channels: openai_channel_types.clone(),
                };
                openai_routes.insert(route.model_id.to_lowercase(), route);
            }

            // anthropic route：每个启用的 anthropic channel 各插一条（不同 channel 的 base_url 可能不同）
            for ch in &anthropic_channels {
                let route = ProviderRoute {
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                    channels: Vec::new(),
                };
                // anthropic 路由表的检索 id 全小写；非 claude-/anthropic 开头的模型
                // 补 claude- 前缀，使 Claude Code 等网关客户端能识别。
                let lower = route.model_id.to_lowercase();
                let key = if lower.starts_with("claude") || lower.starts_with("anthropic") {
                    lower
                } else {
                    format!("claude-{}", lower)
                };
                anthropic_routes.insert(key, route);
            }
        }
    }

    {
        let mut o = state.openai_routes.write().await;
        *o = openai_routes;
    }
    {
        let mut a = state.anthropic_routes.write().await;
        *a = anthropic_routes;
    }

    tracing::info!(
        "Routes refreshed: {} openai models, {} anthropic models",
        state.openai_routes.read().await.len(),
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
    channels: &[(String, String, bool)], // (channel_type, base_url, is_enabled)
    models: &[(String, String)],          // (model_id, model_name)
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

    // upsert channel_configs
    for (channel_type, base_url, enabled) in channels {
        let base_url_opt: Option<&str> = if base_url.is_empty() {
            None
        } else {
            Some(base_url)
        };
        sqlx::query(
            "INSERT INTO provider_channel_config (provider_id, channel_type, base_url, is_enabled) VALUES (?, ?, ?, ?)
             ON CONFLICT(provider_id, channel_type) DO UPDATE SET base_url = excluded.base_url, is_enabled = excluded.is_enabled",
        )
        .bind(id)
        .bind(channel_type)
        .bind(base_url_opt)
        .bind(*enabled as i32)
        .execute(pool)
        .await?;
    }

    // 替换模型列表
    sqlx::query("DELETE FROM provider_models WHERE provider_id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    for (model_id, model_name) in models {
        if model_id.is_empty() {
            continue;
        }
        let mid = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, model_id, model_name) VALUES (?, ?, ?, ?)",
        )
        .bind(&mid)
        .bind(id)
        .bind(model_id)
        .bind(model_name)
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// 从 Provider 的 API 拉取模型列表（不写入 DB），使用前端传入的 api_key
pub async fn fetch_models_from_api(
    client: &reqwest::Client,
    pool: &SqlitePool,
    defs: &[ProviderDef],
    provider_id: &str,
    ui_api_key: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let Some(def) = defs.iter().find(|d| d.id == provider_id) else {
        anyhow::bail!("provider not found");
    };

    let endpoint = def
        .models_endpoint
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("models_endpoint not configured"))?;

    // 使用前端传入的 key
    let api_key = if ui_api_key.is_empty() {
        anyhow::bail!("api_key not configured");
    } else {
        ui_api_key.to_string()
    };

    let channel_configs = get_channel_configs(pool, provider_id).await;
    let channels = merge_channels(&def.channels, &channel_configs);

    let Some(channel) = channels.iter().find(|c| c.is_enabled) else {
        anyhow::bail!("no enabled channel found");
    };

    let url = endpoint;
    let mut req = client
        .get(url)
        .timeout(std::time::Duration::from_secs(10));

    match channel.channel_type.as_str() {
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
        "SELECT provider_id, channel_type, base_url, is_enabled FROM provider_channel_config WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// 合并配置定义与用户覆盖：用户覆盖的 base_url 优先，否则用配置默认值
fn merge_channels(
    defs: &[ChannelDef],
    configs: &[ProviderChannelConfigRow],
) -> Vec<ChannelDetail> {
    defs.iter()
        .map(|def| {
            let cfg = configs.iter().find(|c| c.channel_type == def.channel_type);
            let base_url = cfg
                .and_then(|c| c.base_url.as_deref())
                .unwrap_or(&def.base_url)
                .to_string();
            let is_enabled = cfg.map(|c| c.is_enabled).unwrap_or(true);
            ChannelDetail {
                channel_type: def.channel_type.clone(),
                base_url,
                is_enabled,
            }
        })
        .collect()
}