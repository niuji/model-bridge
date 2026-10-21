use axum::response::{IntoResponse, Response};
use std::sync::Arc;

use crate::state::{
    AnthropicModelEntry, AnthropicModelsList, AppState, OpenAIModelEntry, OpenAIModelsList,
};

fn anthropic_model_id(key: &str, model_id: &str) -> String {
    if model_id.to_lowercase().ends_with("[1m]") {
        format!("{}{}", key, &model_id[model_id.len() - 4..])
    } else {
        key.to_string()
    }
}

#[derive(serde::Serialize)]
struct ChannelModel {
    id: String,
    name: String,
    provider_id: String,
    provider_name: String,
    provider_icon: Option<String>,
}

#[derive(serde::Serialize)]
struct ChannelModels {
    channel_type: &'static str,
    models_endpoint: &'static str,
    models: Vec<ChannelModel>,
}

pub async fn get_channel_models(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl IntoResponse {
    let mut channels = Vec::with_capacity(3);
    for (channel_type, models_endpoint, routes_lock) in [
        (
            "openai_chat",
            "/openai-chat/v1/models",
            &state.openai_chat_routes,
        ),
        (
            "openai_responses",
            "/openai-responses/v1/models",
            &state.openai_responses_routes,
        ),
        ("anthropic", "/anthropic/v1/models", &state.anthropic_routes),
    ] {
        let routes = routes_lock.read().await;
        // 以生效路由为准，避免把已停用或登录失效的供应商配置展示为可用模型。
        let mut models: Vec<_> = routes
            .iter()
            .map(|(key, route)| ChannelModel {
                id: if channel_type == "anthropic" {
                    anthropic_model_id(key, &route.model_id)
                } else {
                    key.clone()
                },
                name: route.model_name.clone(),
                provider_id: route.provider_id.clone(),
                provider_name: route.provider_name.clone(),
                provider_icon: state
                    .provider_defs
                    .iter()
                    .find(|provider| provider.id == route.provider_id)
                    .and_then(|provider| provider.icon.clone()),
            })
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        channels.push(ChannelModels {
            channel_type,
            models_endpoint,
            models,
        });
    }
    axum::Json(channels)
}

async fn openai_models_list(
    routes_lock: &Arc<
        tokio::sync::RwLock<std::collections::HashMap<String, crate::state::ProviderRoute>>,
    >,
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
        let id = anthropic_model_id(key, &route.model_id);
        models.push(AnthropicModelEntry {
            id,
            entry_type: "model".to_string(),
            // display_name 直接用 model_name：冲突模型的 [{provider_id}] 前缀已在建表时写入，
            // 非冲突模型是纯 model_name。仅展示用，不影响路由 key（id）与上游转发（route.model_id）
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
