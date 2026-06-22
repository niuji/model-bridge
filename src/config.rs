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
        }
    }
}

pub fn load_config(cli: &Cli) -> anyhow::Result<AppConfig> {
    if cli.config.exists() {
        let content = std::fs::read_to_string(&cli.config)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        Ok(AppConfig::default())
    }
}
