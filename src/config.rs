use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "model-bridge")]
pub struct Cli {
    /// 配置文件路径
    #[arg(short, long, default_value = "model-bridge.toml")]
    pub config: PathBuf,
}

#[derive(Clone, Deserialize)]
pub struct AppConfig {
    pub proxy: ServerConfig,
    pub admin: ServerConfig,
    pub database: DatabaseConfig,
    pub bridge: BridgeConfig,
    /// Provider 定义 JSON 文件路径
    #[serde(default = "default_providers_file")]
    pub providers_file: String,
}

#[derive(Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Clone, Deserialize)]
pub struct BridgeConfig {
    /// 后台自动刷新模型列表间隔（分钟）
    pub refresh_interval_min: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProviderDef {
    pub id: String,
    pub name: String,
    /// 图标（emoji 或图片 URL）
    #[serde(default)]
    pub icon: Option<String>,
    /// 获取模型列表的端点，不配置则不支持自动获取
    pub models_endpoint: Option<String>,
    #[serde(default)]
    pub channels: Vec<ChannelDef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChannelDef {
    #[serde(rename = "type")]
    pub channel_type: String,
    pub base_url: String,
}

fn default_providers_file() -> String {
    "providers.json".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 10010,
            },
            admin: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 10020,
            },
            database: DatabaseConfig {
                path: "model-bridge.db".to_string(),
            },
            bridge: BridgeConfig {
                refresh_interval_min: 10,
            },
            providers_file: default_providers_file(),
        }
    }
}

pub fn load_config(cli: &Cli) -> anyhow::Result<AppConfig> {
    let config: AppConfig = if cli.config.exists() {
        let content = std::fs::read_to_string(&cli.config)?;
        toml::from_str(&content)?
    } else {
        AppConfig::default()
    };
    Ok(config)
}

/// 从 JSON 文件加载 Provider 定义
pub fn load_providers(path: &str) -> anyhow::Result<Vec<ProviderDef>> {
    let content = std::fs::read_to_string(path)?;
    let providers: Vec<ProviderDef> = serde_json::from_str(&content)?;
    Ok(providers)
}