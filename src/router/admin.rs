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
    match provider_svc::list_providers(&state.db).await {
        Ok(providers) => Json(providers).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn get_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match provider_svc::get_provider(&state.db, &id).await {
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
pub struct CreateProviderRequest {
    pub name: String,
    pub openai_base_url: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub api_key: String,
}

pub async fn create_provider(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateProviderRequest>,
) -> impl IntoResponse {
    if req.openai_base_url.is_none() && req.anthropic_base_url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "at least one of openai_base_url or anthropic_base_url is required"})),
        )
            .into_response();
    }

    let (openai_ok, anthropic_ok) = provider_svc::validate_provider(
        &state.client,
        req.openai_base_url.as_deref(),
        req.anthropic_base_url.as_deref(),
        &req.api_key,
    )
    .await;

    if req.openai_base_url.is_some() && !openai_ok {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "openai_base_url /models probe failed"})),
        )
            .into_response();
    }
    if req.anthropic_base_url.is_some() && !anthropic_ok {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "anthropic_base_url /models probe failed"})),
        )
            .into_response();
    }

    let id = uuid::Uuid::new_v4().to_string();
    match provider_svc::create_provider(
        &state.db,
        &id,
        &req.name,
        req.openai_base_url.as_deref(),
        req.anthropic_base_url.as_deref(),
        &req.api_key,
    )
    .await
    {
        Ok(provider) => {
            if let Err(e) = provider_svc::refresh_routes(&state).await {
                tracing::error!("Failed to refresh routes: {}", e);
            }
            (StatusCode::CREATED, Json(provider)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct UpdateProviderRequest {
    pub name: String,
    pub openai_base_url: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub api_key: Option<String>,
    pub is_enabled: bool,
}

pub async fn update_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> impl IntoResponse {
    // 验证至少配置了一个 URL（与 create_provider 保持一致）
    if req.openai_base_url.is_none() && req.anthropic_base_url.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "at least one of openai_base_url or anthropic_base_url is required"})),
        )
            .into_response();
    }

    match provider_svc::update_provider(
        &state.db,
        &id,
        &req.name,
        req.openai_base_url.as_deref(),
        req.anthropic_base_url.as_deref(),
        req.api_key.as_deref(),
        req.is_enabled,
    )
    .await
    {
        Ok(Some(provider)) => {
            if let Err(e) = provider_svc::refresh_routes(&state).await {
                tracing::error!("Failed to refresh routes: {}", e);
            }
            Json(provider).into_response()
        }
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

pub async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match provider_svc::delete_provider(&state.db, &id).await {
        Ok(true) => {
            if let Err(e) = provider_svc::refresh_routes(&state).await {
                tracing::error!("Failed to refresh routes: {}", e);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => (
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

pub async fn refresh_provider(
    State(state): State<Arc<AppState>>,
    Path(_id): Path<String>,
) -> impl IntoResponse {
    // 刷新所有 provider 的路由表（_id 保留用于未来按需刷新单个 provider）
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
    let keys = sqlx::query_as::<_, crate::db::models::ApiKey>(
        "SELECT id, key_hash, name, is_enabled, created_at FROM api_keys ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await;

    match keys {
        Ok(keys) => {
            let result: Vec<serde_json::Value> = keys
                .into_iter()
                .map(|k| {
                    serde_json::json!({
                        "id": k.id,
                        "name": k.name,
                        "is_enabled": k.is_enabled,
                        "created_at": k.created_at.to_string(),
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

    let result = sqlx::query("INSERT INTO api_keys (id, key_hash, api_key, name) VALUES (?, ?, ?, ?)")
        .bind(&id)
        .bind(&key_hash_hex)
        .bind(&raw_key)
        .bind(&req.name)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": id,
                "name": req.name,
                "key": raw_key,
                "is_enabled": true,
            })),
        )
            .into_response(),
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
        Ok(result) if result.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
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
        Ok(Some(key)) => Json(serde_json::json!({ "key": key })).into_response(),
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
