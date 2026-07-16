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
    /// 后台探测上游 /v1/models 的间隔（分钟），与路由刷新解耦。
    /// 默认 1440（1 天）。模型目录变化是天/周级，日频已足够。
    /// serde default 保证旧配置文件（无此字段）仍可解析为 1440。
    #[serde(default = "default_probe_interval_min")]
    pub probe_interval_min: u64,
}

/// `probe_interval_min` 的 serde 默认值：缺省时取 1 天，而非 u64::default()=0
/// （否则旧配置会让探测退化为每分钟一次）。
fn default_probe_interval_min() -> u64 {
    1440
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
                probe_interval_min: 1440,
            },
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

/// 加载 Provider 定义：编译期内嵌的 providers.json + ~/.mb/providers.json 用户自定义合并。
/// 用户自定义中同 id 覆盖内置，新 id 追加。
pub fn load_providers() -> anyhow::Result<Vec<ProviderDef>> {
    let mut providers: Vec<ProviderDef> =
        serde_json::from_str(include_str!("../providers.json"))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_defaults_probe_interval_when_absent() {
        // 旧配置（无 probe_interval_min）应解析成功并默认 1440
        let cfg: BridgeConfig = toml::from_str("refresh_interval_min = 5\n").unwrap();
        assert_eq!(cfg.refresh_interval_min, 5);
        assert_eq!(cfg.probe_interval_min, 1440);
    }

    #[test]
    fn bridge_respects_explicit_probe_interval() {
        let cfg: BridgeConfig = toml::from_str("refresh_interval_min = 5\nprobe_interval_min = 30\n").unwrap();
        assert_eq!(cfg.probe_interval_min, 30);
    }
}