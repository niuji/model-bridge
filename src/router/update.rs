use crate::state::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use std::sync::{atomic::Ordering, Arc};

fn error(status: StatusCode, message: impl ToString) -> Response {
    (
        status,
        Json(serde_json::json!({"error":message.to_string()})),
    )
        .into_response()
}
fn allowed_write(headers: &HeaderMap) -> bool {
    if headers
        .get("x-model-bridge-update")
        .and_then(|v| v.to_str().ok())
        != Some("1")
    {
        return false;
    }
    if !headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return false;
    }
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(&format!("http://{host}")) else {
        return false;
    };
    if !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match headers.get("origin") {
        None => true,
        Some(origin) => origin.to_str().ok() == Some(format!("http://{host}").as_str()),
    }
}

pub async fn status(State(state): State<Arc<AppState>>) -> Response {
    let updates = &state.updates;
    let job = match &updates.paths {
        Some(paths) => match paths.job() {
            Ok(j) => j,
            Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        None => None,
    };
    Json(serde_json::json!({"current_version":env!("CARGO_PKG_VERSION"),"supported":updates.paths.is_some(),"unsupported_reason":updates.unsupported_reason,"checking":updates.checking.load(Ordering::Acquire),"check":*updates.check.lock().await,"job":job})).into_response()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRequest {}
pub async fn check(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(_): Json<CheckRequest>,
) -> Response {
    if !allowed_write(&headers) {
        return error(StatusCode::FORBIDDEN, "更新操作需要同源 JSON 请求");
    }
    match state.updates.start_check().await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => error(StatusCode::TOO_MANY_REQUESTS, e),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyRequest {
    pub version: String,
}
pub async fn apply(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ApplyRequest>,
) -> Response {
    if !allowed_write(&headers) {
        return error(StatusCode::FORBIDDEN, "更新操作需要同源 JSON 请求");
    }
    match state.updates.apply(&body.version).await {
        Ok(id) => (StatusCode::ACCEPTED, Json(serde_json::json!({"job_id":id}))).into_response(),
        Err(e) => error(StatusCode::CONFLICT, e),
    }
}
pub async fn readiness(State(state): State<Arc<AppState>>) -> Response {
    let updates = &state.updates;
    let job = match &updates.paths {
        Some(paths) => match paths.job() {
            Ok(j) => j,
            Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, e),
        },
        None => None,
    };
    Json(serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"job_id":job.map(|j|j.id),"ready":updates.ready.load(Ordering::Acquire),"active":updates.active.load(Ordering::Acquire)})).into_response()
}
pub async fn maintenance(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if state.updates.validating()
        && !(request.method() == axum::http::Method::GET
            && matches!(
                request.uri().path(),
                "/api/admin/update" | "/api/admin/update/readiness"
            ))
    {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "服务正在验证更新，请稍后重试",
        );
    }
    next.run(request).await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_write_requires_custom_header_local_host_and_same_origin() {
        let mut headers = HeaderMap::new();
        assert!(!allowed_write(&headers));
        headers.insert("host", "127.0.0.1:10020".parse().unwrap());
        headers.insert("x-model-bridge-update", "1".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());
        assert!(allowed_write(&headers));
        headers.insert("origin", "https://evil.example".parse().unwrap());
        assert!(!allowed_write(&headers));
        headers.insert("origin", "http://127.0.0.1:10020".parse().unwrap());
        assert!(allowed_write(&headers));
        headers.insert("host", "evil.example:10020".parse().unwrap());
        assert!(!allowed_write(&headers));
    }
    #[test]
    fn update_apply_rejects_injected_urls_and_commands() {
        assert!(serde_json::from_value::<ApplyRequest>(
            serde_json::json!({"version":"1.0.0","url":"http://evil"})
        )
        .is_err());
        assert!(
            serde_json::from_value::<CheckRequest>(serde_json::json!({"command":"rm"})).is_err()
        );
    }
}
