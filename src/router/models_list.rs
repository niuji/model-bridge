use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use crate::state::{
    AnthropicModelEntry, AnthropicModelsList, AppState, OpenAIModelEntry, OpenAIModelsList,
};

async fn openai_models_list(
    routes_lock: &Arc<tokio::sync::RwLock<std::collections::HashMap<String, crate::state::ProviderRoute>>>,
    log_path: &str,
) -> Response {
    let routes = routes_lock.read().await;
    let mut models: Vec<OpenAIModelEntry> = Vec::with_capacity(routes.len());

    // id 用检索 key（客户端据此发起请求即可命中路由表）。
    for (key, route) in routes.iter() {
        models.push(OpenAIModelEntry {
            id: key.clone(),
            object: "model".to_string(),
            created: 0,
            owned_by: route.provider_name.clone(),
        });
    }

    // 排序后去重
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);

    tracing::debug!("GET {} → {} models: {:?}", log_path, models.len(), models);

    axum::Json(OpenAIModelsList {
        object: "list".to_string(),
        data: models,
    })
    .into_response()
}

pub async fn get_openai_chat_models(state: Arc<AppState>) -> Response {
    openai_models_list(&state.openai_chat_routes, "/openai-chat/v1/models").await
}

pub async fn get_openai_responses_models(state: Arc<AppState>) -> Response {
    openai_models_list(
        &state.openai_responses_routes,
        "/openai-responses/v1/models",
    )
    .await
}

pub async fn get_anthropic_models(state: Arc<AppState>) -> Response {
    let routes = state.anthropic_routes.read().await;
    let mut models: Vec<AnthropicModelEntry> = Vec::with_capacity(routes.len());

    // 对外暴露的 id 用检索 key（客户端据此发起请求即可命中路由表）。
    // 路由表已将 [1m]/[1M] 后缀剥除（仅作 1M 变体标记），这里按原始 model_id 是否带该后缀补回，
    // 让 Claude Code 据此在本地开启 1M 上下文。
    for (key, route) in routes.iter() {
        let id = if route.model_id.to_lowercase().ends_with("[1m]") {
            // 补回原始大小写的后缀；[1m]/[1M] 为末 4 个 ASCII 字节，ends_with 已保证按字节切安全
            let suffix = &route.model_id[route.model_id.len() - 4..];
            format!("{}{}", key, suffix)
        } else {
            key.clone()
        };
        models.push(AnthropicModelEntry {
            id,
            entry_type: "model".to_string(),
            display_name: route.model_name.clone(),
            created_at: String::new(),
        });
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);

    tracing::debug!(
        "GET /anthropic/v1/models → {} models: {:?}",
        models.len(),
        models
    );

    let result = AnthropicModelsList {
        data: models,
        has_more: false,
        first_id: None,
        last_id: None,
    };

    axum::Json(result).into_response()
}
