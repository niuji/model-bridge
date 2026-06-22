pub mod admin;
pub mod models_list;
pub mod proxy;

use axum::{extract::Request, response::{Html, IntoResponse, Response}, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

/// 嵌入整个管理界面静态资源
static WEB_UI_DIR: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/web/dist");

/// 构建代理路由（OpenAI + Anthropic 协议转发）
pub fn create_proxy_router(state: Arc<AppState>) -> Router {
    let openai_router = Router::new()
        .route("/v1/{*path}", axum::routing::any(proxy::openai_handler))
        .with_state(state.clone());

    let anthropic_router = Router::new()
        .route("/v1/{*path}", axum::routing::any(proxy::anthropic_handler))
        .with_state(state.clone());

    Router::new()
        .nest("/openai", openai_router)
        .nest("/anthropic", anthropic_router)
}

/// 构建管理路由（Admin API + Web 管理界面）
pub fn create_admin_router(state: Arc<AppState>) -> Router {
    let admin_api = Router::new()
        .route(
            "/providers",
            axum::routing::get(admin::list_providers),
        )
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
            "/providers/{id}/refresh",
            axum::routing::post(admin::refresh_provider),
        )
        .route(
            "/api-keys",
            axum::routing::get(admin::list_api_keys).post(admin::create_api_key),
        )
        .route(
            "/api-keys/{id}",
            axum::routing::get(admin::get_api_key)
                .put(admin::toggle_api_key)
                .delete(admin::delete_api_key),
        )
        .route("/logs", axum::routing::get(admin::stats_logs))
        .route("/stats/overview", axum::routing::get(admin::stats_overview))
        .route("/stats/models", axum::routing::get(admin::stats_models))
        .route("/stats/daily", axum::routing::get(admin::stats_daily))
        .route("/stats/hourly", axum::routing::get(admin::stats_hourly))
        .with_state(state);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .nest("/api/admin", admin_api)
        .fallback(serve_ui)
        .layer(cors)
}

async fn serve_ui(request: Request) -> Response {
    // API 路径返回 JSON 404
    if request.uri().path().starts_with("/api/") {
        return (
            axum::http::StatusCode::NOT_FOUND,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            r#"{"error":"not found"}"#,
        )
            .into_response();
    }

    let path = request.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // 尝试在嵌入的静态资源目录中查找文件
    if let Some(file) = WEB_UI_DIR.get_file(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        let content = file.contents();
        return (
            [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
            content.to_vec(),
        )
            .into_response();
    }

    // 其他路径返回 SPA 的 index.html（支持前端路由）
    if let Some(file) = WEB_UI_DIR.get_file("index.html") {
        Html(file.contents_utf8().unwrap_or("")).into_response()
    } else {
        Html("<!DOCTYPE html><html><body>UI not found</body></html>").into_response()
    }
}