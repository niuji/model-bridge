use clap::Parser;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "model-bridge", version)]
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
    /// 客户端 mb- API key 的静态加密密钥（base64 编码的 32 字节）。
    /// 未配置时按明文存储（仅适用于 admin 仅暴露在 loopback 的受限场景）。
    #[serde(default)]
    pub encryption_key: Option<String>,
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
    #[serde(default)]
    pub channels: Vec<ChannelDef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChannelDef {
    #[serde(rename = "type")]
    pub channel_type: String,
    /// 转发请求的上游端点
    pub base_url: String,
    /// 拉取该通道模型列表的端点（不配置则该通道不支持「同步」）
    #[serde(default)]
    pub models_endpoint: Option<String>,
}

fn default_providers_file() -> String {
    "providers.json".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            proxy: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 10010,
            },
            admin: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 10020,
            },
            database: DatabaseConfig {
                path: "model-bridge.db".to_string(),
                encryption_key: None,
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

/// 从 JSON 文件加载 Provider 定义，并合并 ~/.mb/*.json 中的用户自定义 Provider（同名优先）
pub fn load_providers(path: &str) -> anyhow::Result<Vec<ProviderDef>> {
    // 优先读磁盘上的 providers_file（dev / 自定义部署，便于本地编辑后立即生效）；
    // 磁盘缺失则回退到编译期嵌入的内置定义，使发布二进制（zip 仅含 exe）开箱即用。
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!(
                "providers file '{}' not found on disk, using embedded builtin providers",
                path
            );
            include_str!("../providers.json").to_string()
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "failed to read providers file '{}': {}",
                path,
                e
            ));
        }
    };
    let mut providers: Vec<ProviderDef> = serde_json::from_str(&content)?;

    let user_file = dirs::home_dir().map(|h| h.join(".mb").join("providers.json"));
    if let Some(file_path) = user_file.filter(|p| p.exists()) {
        let raw = match std::fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("failed to read user provider file {:?}: {}", file_path, e);
                return Ok(providers);
            }
        };
        let user_defs: Vec<ProviderDef> = match serde_json::from_str(&raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse user provider file {:?}: {}", file_path, e);
                return Ok(providers);
            }
        };
        for user_def in user_defs {
            if let Some(existing) = providers.iter_mut().find(|p| p.id == user_def.id) {
                tracing::info!("user provider '{}' overrides builtin definition", user_def.id);
                *existing = user_def;
            } else {
                tracing::info!("user provider '{}' added from ~/.mb/providers.json", user_def.id);
                providers.push(user_def);
            }
        }
    }

    Ok(providers)
}