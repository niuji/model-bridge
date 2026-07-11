use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::ProviderDef;

/// 内存中的 Provider 路由信息
#[derive(Debug, Clone)]
pub struct ProviderRoute {
    pub provider_id: String,
    pub provider_name: String,
    /// 原始 model_id（保留大小写），用于上游请求
    pub model_id: String,
    /// 显示名称
    pub model_name: String,
    pub base_url: String,
    pub api_key: String,
}

/// 应用程序全局状态
pub struct AppState {
    /// OpenAI Chat 协议（/openai-chat/v1/）路由表：检索 key 全小写。
    /// 仅收录 channel_type=openai_chat 的模型，转发到该通道 base_url。
    pub openai_chat_routes: Arc<RwLock<HashMap<String, ProviderRoute>>>,
    /// OpenAI Responses 协议（/openai-responses/v1/）路由表：检索 key 全小写。
    /// 仅收录 channel_type=openai_responses 的模型，转发到该通道 base_url。
    pub openai_responses_routes: Arc<RwLock<HashMap<String, ProviderRoute>>>,
    /// anthropic 格式的检索 id → ProviderRoute（检索 key 全小写；非 claude-/anthropic 开头的模型写入时补 claude- 前缀）
    pub anthropic_routes: Arc<RwLock<HashMap<String, ProviderRoute>>>,
    /// Provider 定义（来自配置文件）
    pub provider_defs: Vec<ProviderDef>,
    pub db: SqlitePool,
    /// 共享 HTTP 客户端（复用连接池）
    pub client: reqwest::Client,
    /// API Key 内存缓存: key_hash → api_key_id（仅存 is_enabled=1 的 key）
    pub api_key_cache: Arc<RwLock<HashMap<String, String>>>,
    /// 客户端 mb- API key 的 AES-256-GCM 密钥（None 表示明文存储）。
    pub encryption_key: Option<[u8; 32]>,
    /// 代理服务对外的 base URL（http://{host}:{port}；host 0.0.0.0/::/空 → localhost），
    /// 供管理 UI「接入指南」展示客户端接入地址。
    pub proxy_base_url: String,
}

/// OpenAI /v1/models 响应格式
#[derive(serde::Serialize, Debug)]
pub struct OpenAIModelsList {
    pub object: String,
    pub data: Vec<OpenAIModelEntry>,
}

#[derive(serde::Serialize, Debug)]
pub struct OpenAIModelEntry {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

/// Anthropic /v1/models 响应格式
#[derive(serde::Serialize, Debug)]
pub struct AnthropicModelsList {
    pub data: Vec<AnthropicModelEntry>,
    pub has_more: bool,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
}

#[derive(serde::Serialize, Debug)]
pub struct AnthropicModelEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub display_name: String,
    pub created_at: String,
}