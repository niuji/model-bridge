pub mod admin;
pub mod models_list;
pub mod proxy;

use axum::{response::Html, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use crate::state::AppState;

/// 嵌入管理界面 HTML
const WEB_UI: &str = include_str!("../../web/dist/index.html");

pub fn create_router(state: Arc<AppState>) -> Router {
    let openai_router = Router::new()
        .route("/v1/{*path}", axum::routing::any(proxy::openai_handler))
        .with_state(state.clone());

    let anthropic_router = Router::new()
        .route("/v1/{*path}", axum::routing::any(proxy::anthropic_handler))
        .with_state(state.clone());

    let admin_router = Router::new()
        .route(
            "/providers",
            axum::routing::get(admin::list_providers).post(admin::create_provider),
        )
        .route(
            "/providers/{id}",
            axum::routing::get(admin::get_provider)
                .put(admin::update_provider)
                .delete(admin::delete_provider),
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
            axum::routing::delete(admin::delete_api_key),
        )
        .route("/stats/overview", axum::routing::get(admin::stats_overview))
        .route("/stats/models", axum::routing::get(admin::stats_models))
        .route("/stats/daily", axum::routing::get(admin::stats_daily))
        .with_state(state.clone());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .nest("/openai", openai_router)
        .nest("/anthropic", anthropic_router)
        .nest("/api/admin", admin_router)
        .fallback(serve_ui)
        .layer(cors)
}

async fn serve_ui() -> Html<&'static str> {
    Html(WEB_UI)
}
