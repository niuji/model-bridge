use axum::{
    extract::State,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::Digest;

/// 验证 Bearer token 是否匹配已注册的 API Key
pub async fn api_key_auth(
    State(pool): State<sqlx::SqlitePool>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let Some(token) = auth_header else {
        return (
            StatusCode::UNAUTHORIZED,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"missing Authorization header"}"#,
        )
            .into_response();
    };

    let key_hash = format!("{:x}", sha2::Sha256::digest(token.as_bytes()));

    let result = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM api_keys WHERE key_hash = ? AND is_enabled = 1",
    )
    .bind(&key_hash)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(count) if count > 0 => {
            let response = next.run(request).await;
            response
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"invalid api key"}"#,
        )
            .into_response(),
    }
}