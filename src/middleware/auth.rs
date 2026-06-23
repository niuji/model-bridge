use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
    Json,
};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::Arc;

use crate::state::AppState;

/// 认证通过后注入到 request extensions 中的 API Key ID
#[derive(Clone, Debug)]
pub struct AuthenticatedKey(pub String);

/// 从数据库加载所有启用的 API Key 到内存缓存
pub async fn refresh_api_key_cache(state: &AppState) -> anyhow::Result<()> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT key_hash, id FROM api_keys WHERE is_enabled = 1",
    )
    .fetch_all(&state.db)
    .await?;

    let mut cache = HashMap::new();
    for (key_hash, id) in rows {
        cache.insert(key_hash, id);
    }

    let mut guard = state.api_key_cache.write().await;
    *guard = cache;
    tracing::info!("api_key_cache refreshed: {} keys loaded", guard.len());
    Ok(())
}

/// API Key 认证中间件
/// 从 Authorization: Bearer mb-xxx 请求头提取 key，SHA-256 哈希后查内存缓存，
/// 验证通过后将 AuthenticatedKey 注入 request extensions 供下游使用
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> axum::response::Response {
    // 支持两种格式: Authorization: Bearer mb-xxx (OpenAI) 和 x-api-key: mb-xxx (Anthropic)
    let key = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
        });

    let key = match key {
        Some(k) => k,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "missing API key, use Authorization: Bearer mb-xxx or x-api-key: mb-xxx"})),
            )
                .into_response();
        }
    };

    let key_hash = sha2::Sha256::digest(key.as_bytes());
    let key_hash_hex = format!("{:x}", key_hash);

    // 查内存缓存，无需 DB 查询
    let cache = state.api_key_cache.read().await;
    if let Some(api_key_id) = cache.get(&key_hash_hex) {
        let api_key_id = api_key_id.clone();
        drop(cache);
        request.extensions_mut().insert(AuthenticatedKey(api_key_id));
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid or disabled API key"})),
        )
            .into_response()
    }
}