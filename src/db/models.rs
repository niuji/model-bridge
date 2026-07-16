use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

/// 用户对 Provider 的配置覆盖（DB 存储）
#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct ProviderConfigRow {
    pub provider_id: String,
    pub api_key: String,
    pub is_enabled: bool,
}

/// 用户对 Channel 的配置覆盖（DB 存储）
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProviderChannelConfigRow {
    pub provider_id: String,
    pub channel_type: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProviderModel {
    pub id: String,
    pub provider_id: String,
    /// 该模型所属通道（openai_chat / openai_responses / anthropic）
    pub channel_type: String,
    pub model_id: String,
    pub model_name: String,
}

/// 合并后的 Provider 详情（配置定义 + 用户覆盖）
#[derive(Debug, Clone, Serialize)]
pub struct ProviderDetail {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub api_key: String,
    pub is_enabled: bool,
    pub channels: Vec<ChannelDetail>,
    pub models: Vec<ProviderModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelDetail {
    pub channel_type: String,
    pub base_url: String,
    /// 该通道拉取模型列表的端点（无则不支持「同步」）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_endpoint: Option<String>,
    pub is_enabled: bool,
    /// 该通道已配置的模型数（同一通道内 UNIQUE 去重后的计数）
    pub model_count: i64,
}

/// 合并后的 Provider 摘要（列表用，不含 api_key 和 models）
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub is_enabled: bool,
    pub channels: Vec<ChannelDetail>,
    /// 自上次打开"上游变更"弹窗以来的变化计数（baseline 为空/未查看过时为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift: Option<DriftSummary>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[allow(dead_code)]
pub struct ApiKey {
    pub id: String,
    pub key_hash: String,
    pub name: String,
    pub is_enabled: bool,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
#[allow(dead_code)]
pub struct UsageRecord {
    pub id: i64,
    pub api_key_id: Option<String>,
    pub model_id: String,
    pub provider_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub latency_ms: i64,
    pub status: String,
    pub error_msg: Option<String>,
    pub client: Option<String>,
    pub created_at: NaiveDateTime,
}

/// 上游快照中的一行（FromRow，仅取 diff 所需列）
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct UpstreamModelRow {
    pub provider_id: String,
    pub channel_type: String,
    pub model_id: String,
    pub model_name: String,
}

/// 模型条目（变更弹窗展示用）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelEntry {
    pub model_id: String,
    pub model_name: String,
}

/// 单通道的上游变更（current 相对 baseline 的对称差）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelDrift {
    pub channel_type: String,
    pub added: Vec<ModelEntry>,
    pub removed: Vec<ModelEntry>,
}

/// 卡片角标用的变更计数
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DriftSummary {
    pub new: i64,
    pub removed: i64,
}