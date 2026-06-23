use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use crate::state::{
    AnthropicModelEntry, AnthropicModelsList, AppState, OpenAIModelEntry, OpenAIModelsList,
};

pub async fn get_openai_models(state: Arc<AppState>) -> Response {
    let routes = state.openai_routes.read().await;
    let mut models: Vec<OpenAIModelEntry> = Vec::with_capacity(routes.len());

    for (_key, route) in routes.iter() {
        models.push(OpenAIModelEntry {
            id: route.model_id.clone(),
            object: "model".to_string(),
            created: 0,
            owned_by: route.provider_name.clone(),
        });
    }

    // 排序后去重
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);

    tracing::info!(
        "GET /openai/v1/models → {} models: {:?}",
        models.len(),
        models
    );

    let result = OpenAIModelsList {
        object: "list".to_string(),
        data: models,
    };

    axum::Json(result).into_response()
}

pub async fn get_anthropic_models(state: Arc<AppState>) -> Response {
    let routes = state.anthropic_routes.read().await;
    let mut models: Vec<AnthropicModelEntry> = Vec::with_capacity(routes.len());

    // 对外暴露的 id 用路由表 key（已补 claude- 前缀且全小写），route.model_id 是转发上游用的原始 id。
    for (key, route) in routes.iter() {
        models.push(AnthropicModelEntry {
            id: key.clone(),
            entry_type: "model".to_string(),
            display_name: route.model_name.clone(),
            created_at: String::new(),
        });
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);

    tracing::info!(
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
