use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::db::models::Provider;
use crate::state::{ProviderRoute, AppState};

/// 从 DB 加载所有 enabled providers，探测模型列表，构建路由表
pub async fn refresh_routes(state: &Arc<AppState>) -> anyhow::Result<()> {
    let providers = sqlx::query_as::<_, Provider>(
        "SELECT id, name, openai_base_url, anthropic_base_url, api_key_encrypted, is_enabled, created_at FROM providers WHERE is_enabled = 1",
    )
    .fetch_all(&state.db)
    .await?;

    let mut openai_routes: HashMap<String, ProviderRoute> = HashMap::new();
    let mut anthropic_routes: HashMap<String, ProviderRoute> = HashMap::new();

    for provider in &providers {
        let route = ProviderRoute {
            provider_id: provider.id.clone(),
            provider_name: provider.name.clone(),
            base_url: String::new(),
            api_key: provider.api_key_encrypted.clone(),
        };

        // 探测 openai 格式
        if let Some(ref base_url) = provider.openai_base_url {
            match probe_openai_models(&state.client, base_url, &provider.api_key_encrypted).await {
                Ok(models) => {
                    for model_id in models {
                        let mut r = route.clone();
                        r.base_url = base_url.clone();
                        openai_routes.insert(model_id, r);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Provider {} openai model probe failed: {}",
                        provider.name,
                        e
                    );
                }
            }
        }

        // 探测 anthropic 格式
        if let Some(ref base_url) = provider.anthropic_base_url {
            match probe_anthropic_models(&state.client, base_url, &provider.api_key_encrypted).await {
                Ok(models) => {
                    for model_id in models {
                        let mut r = route.clone();
                        r.base_url = base_url.clone();
                        anthropic_routes.insert(model_id, r);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Provider {} anthropic model probe failed: {}",
                        provider.name,
                        e
                    );
                }
            }
        }
    }

    // 原子更新
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

/// 探测 provider 的 OpenAI 格式 /models 端点
async fn probe_openai_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    let models: Vec<String> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if models.is_empty() {
        anyhow::bail!("empty model list");
    }

    Ok(models)
}

/// 探测 provider 的 Anthropic 格式 /models 端点
async fn probe_anthropic_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> anyhow::Result<Vec<String>> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {}", resp.status());
    }

    let body: serde_json::Value = resp.json().await?;
    let models: Vec<String> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    if models.is_empty() {
        anyhow::bail!("empty model list");
    }

    Ok(models)
}

/// 检查 provider 配置的有效性
pub async fn validate_provider(
    client: &reqwest::Client,
    openai_base_url: Option<&str>,
    anthropic_base_url: Option<&str>,
    api_key: &str,
) -> (bool, bool) {
    let (mut openai_ok, mut anthropic_ok) = (false, false);

    if let Some(url) = openai_base_url {
        openai_ok = probe_openai_models(client, url, api_key).await.is_ok();
    }
    if let Some(url) = anthropic_base_url {
        anthropic_ok = probe_anthropic_models(client, url, api_key).await.is_ok();
    }

    (openai_ok, anthropic_ok)
}

/// 向 provider 注册新的 API Key
pub async fn create_provider(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    openai_base_url: Option<&str>,
    anthropic_base_url: Option<&str>,
    api_key: &str,
) -> anyhow::Result<Provider> {
    let provider = sqlx::query_as::<_, Provider>(
        "INSERT INTO providers (id, name, openai_base_url, anthropic_base_url, api_key_encrypted) VALUES (?, ?, ?, ?, ?) RETURNING id, name, openai_base_url, anthropic_base_url, api_key_encrypted, is_enabled, created_at",
    )
    .bind(id)
    .bind(name)
    .bind(openai_base_url)
    .bind(anthropic_base_url)
    .bind(api_key)
    .fetch_one(pool)
    .await?;

    Ok(provider)
}

/// 列出所有 providers
pub async fn list_providers(pool: &SqlitePool) -> anyhow::Result<Vec<Provider>> {
    let providers = sqlx::query_as::<_, Provider>(
        "SELECT id, name, openai_base_url, anthropic_base_url, api_key_encrypted, is_enabled, created_at FROM providers ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(providers)
}

/// 获取单个 provider
pub async fn get_provider(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<Provider>> {
    let provider = sqlx::query_as::<_, Provider>(
        "SELECT id, name, openai_base_url, anthropic_base_url, api_key_encrypted, is_enabled, created_at FROM providers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(provider)
}

/// 更新 provider
pub async fn update_provider(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    openai_base_url: Option<&str>,
    anthropic_base_url: Option<&str>,
    api_key: Option<&str>,
    is_enabled: bool,
) -> anyhow::Result<Option<Provider>> {
    let provider = if let Some(api_key) = api_key {
        sqlx::query_as::<_, Provider>(
            "UPDATE providers SET name = ?, openai_base_url = ?, anthropic_base_url = ?, api_key_encrypted = ?, is_enabled = ? WHERE id = ? RETURNING id, name, openai_base_url, anthropic_base_url, api_key_encrypted, is_enabled, created_at",
        )
        .bind(name)
        .bind(openai_base_url)
        .bind(anthropic_base_url)
        .bind(api_key)
        .bind(is_enabled as i32)
        .bind(id)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, Provider>(
            "UPDATE providers SET name = ?, openai_base_url = ?, anthropic_base_url = ?, is_enabled = ? WHERE id = ? RETURNING id, name, openai_base_url, anthropic_base_url, api_key_encrypted, is_enabled, created_at",
        )
        .bind(name)
        .bind(openai_base_url)
        .bind(anthropic_base_url)
        .bind(is_enabled as i32)
        .bind(id)
        .fetch_optional(pool)
        .await?
    };
    Ok(provider)
}

/// 删除 provider
pub async fn delete_provider(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM providers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}