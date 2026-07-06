use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use sha2::Digest;
use std::sync::Arc;

use crate::{admin::provider_svc, admin::stats_svc, state::AppState};

// ===== Provider handlers =====

pub async fn list_providers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match provider_svc::list_providers(&state.db, &state.provider_defs).await {
        Ok(providers) => Json(providers).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ===== Settings =====

/// 返回代理服务对外的 base URL 与二进制版本号，供管理 UI 展示接入地址与页脚版本。
/// version 取自 CARGO_PKG_VERSION（编译期注入），永远与 `model-bridge --version` 一致。
pub async fn get_settings(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(serde_json::json!({
        "proxy_base_url": state.proxy_base_url,
        "version": env!("CARGO_PKG_VERSION"),
    }))
    .into_response()
}

pub async fn get_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match provider_svc::get_provider(&state.db, &state.provider_defs, &id).await {
        Ok(Some(provider)) => Json(provider).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "provider not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateProviderChannel {
    pub channel_type: String,
    pub is_enabled: bool,
}

#[derive(Deserialize)]
pub struct UpdateProviderModel {
    pub model_id: String,
    pub model_name: String,
}

#[derive(Deserialize)]
pub struct UpdateProviderRequest {
    pub api_key: String,
    pub is_enabled: bool,
    pub channels: Vec<UpdateProviderChannel>,
    pub models: Vec<UpdateProviderModel>,
}

pub async fn update_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> impl IntoResponse {
    let channels: Vec<(String, bool)> = req
        .channels
        .into_iter()
        .map(|c| (c.channel_type, c.is_enabled))
        .collect();

    let models: Vec<(String, String)> = req
        .models
        .into_iter()
        .map(|m| (m.model_id, m.model_name))
        .collect();

    match provider_svc::update_provider(
        &state.db,
        &id,
        &req.api_key,
        req.is_enabled,
        &channels,
        &models,
    )
    .await
    {
        Ok(()) => {
            if let Err(e) = provider_svc::refresh_routes(&state).await {
                tracing::error!("Failed to refresh routes: {}", e);
            }
            // 返回更新后的详情
            match provider_svc::get_provider(&state.db, &state.provider_defs, &id).await {
                Ok(Some(provider)) => Json(provider).into_response(),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "failed to read updated provider"})),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize, Default)]
pub struct FetchModelsQuery {
    pub api_key: Option<String>,
}

pub async fn fetch_provider_models(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<FetchModelsQuery>,
) -> impl IntoResponse {
    let api_key = match &query.api_key {
        Some(k) if !k.is_empty() => k.as_str(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "api_key is required"})),
            )
                .into_response();
        }
    };
    match provider_svc::fetch_models_from_api(
        &state.client,
        &state.provider_defs,
        &id,
        api_key,
    )
    .await
    {
        Ok(models) => {
            let result: Vec<serde_json::Value> = models
                .into_iter()
                .map(|(model_id, model_name)| {
                    serde_json::json!({ "model_id": model_id, "model_name": model_name })
                })
                .collect();
            Json(serde_json::json!({ "models": result })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn refresh_provider(
    State(state): State<Arc<AppState>>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    match provider_svc::refresh_routes(&state).await {
        Ok(_) => Json(serde_json::json!({"status": "ok"})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ===== API Key handlers =====

pub async fn list_api_keys(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    #[derive(sqlx::FromRow)]
    struct ApiKeyRow {
        id: String,
        api_key: String,
        name: Option<String>,
        is_enabled: bool,
        created_at: chrono::NaiveDateTime,
    }

    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, api_key, name, is_enabled, created_at FROM api_keys ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await;

    match rows {
        Ok(rows) => {
            let result: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|k| {
                    let key_preview = mask_key(&crate::crypto::reveal(state.encryption_key.as_ref(), &k.api_key));
                    serde_json::json!({
                        "id": k.id,
                        "name": k.name,
                        "is_enabled": k.is_enabled,
                        "created_at": k.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                        "key_preview": key_preview,
                    })
                })
                .collect();
            Json(result).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub(crate) fn mask_key(key: &str) -> String {
    if key.len() <= 10 {
        return key.to_string();
    }
    let body = key.strip_prefix("mb-").unwrap_or(key);
    if body.len() <= 7 {
        return key.to_string();
    }
    format!("mb-{}...{}", &body[..3], &body[body.len() - 4..])
}

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

pub async fn create_api_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateApiKeyRequest>,
) -> impl IntoResponse {
    let id = uuid::Uuid::new_v4().to_string();
    let raw_key = format!("mb-{}", uuid::Uuid::new_v4());
    let key_hash = sha2::Sha256::digest(raw_key.as_bytes());
    let key_hash_hex = format!("{:x}", key_hash);

    // 明文仅在创建时返回一次；落库的是加密后的密文（无 key 时退化为明文）。
    let sealed = crate::crypto::seal(state.encryption_key.as_ref(), &raw_key);

    let result = sqlx::query("INSERT INTO api_keys (id, key_hash, api_key, name) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(&key_hash_hex)
        .bind(&sealed)
        .bind(&req.name)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
            crate::middleware::auth::refresh_api_key_cache(&state).await.ok();
            (
                StatusCode::CREATED,
                Json(serde_json::json!({
                    "id": id,
                    "name": req.name,
                    "key": raw_key,
                    "is_enabled": true,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match sqlx::query("DELETE FROM api_keys WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await
    {
        Ok(result) if result.rows_affected() > 0 => {
            crate::middleware::auth::refresh_api_key_cache(&state).await.ok();
            StatusCode::NO_CONTENT.into_response()
        },
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "api key not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result = sqlx::query_scalar::<_, String>("SELECT api_key FROM api_keys WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await;

    match result {
        Ok(Some(key)) => Json(serde_json::json!({ "key": crate::crypto::reveal(state.encryption_key.as_ref(), &key) })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "api key not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ToggleApiKeyRequest {
    pub is_enabled: bool,
}

pub async fn toggle_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ToggleApiKeyRequest>,
) -> impl IntoResponse {
    match sqlx::query("UPDATE api_keys SET is_enabled = ? WHERE id = ?")
        .bind(req.is_enabled)
        .bind(&id)
        .execute(&state.db)
        .await
    {
        Ok(result) if result.rows_affected() > 0 => {
            crate::middleware::auth::refresh_api_key_cache(&state).await.ok();
            Json(serde_json::json!({ "status": "ok" })).into_response()
        },
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "api key not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ===== Logs handlers =====

#[derive(Deserialize, Default)]
pub struct LogsQuery {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 { 1 }
fn default_page_size() -> i64 { 50 }

pub async fn stats_logs(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<LogsQuery>,
) -> impl IntoResponse {
    match stats_svc::get_logs(&state.db, params.page, params.page_size, state.encryption_key.as_ref()).await {
        Ok(data) => Json(data).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ===== Stats handlers =====

pub async fn stats_overview(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match stats_svc::get_overview(&state.db).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn stats_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match stats_svc::get_model_stats(&state.db).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize, Default)]
pub struct StatsQuery {
    #[serde(default = "default_days")]
    pub days: i64,
}

fn default_days() -> i64 {
    7
}

pub async fn stats_daily(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<StatsQuery>,
) -> impl IntoResponse {
    match stats_svc::get_daily_stats(&state.db, params.days).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn stats_hourly(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match stats_svc::get_hourly_stats(&state.db).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}