use clap::Parser;
use serde::{Deserialize, Serialize};
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
    /// 日志保留天数。超过此天数的 usage_records 行将被自动清理。
    /// 设为 0 禁用自动清理。默认 730（2 年）。
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u64,
    /// 后台探测 provider 余额的间隔（分钟）。默认 10。
    /// serde default 保证旧配置文件（无此字段）仍可解析。
    #[serde(default = "default_balance_interval_min")]
    pub balance_interval_min: u64,
}

/// `probe_interval_min` 的 serde 默认值：缺省时取 1 天，而非 u64::default()=0
/// （否则旧配置会让探测退化为每分钟一次）。
fn default_probe_interval_min() -> u64 {
    1440
}

fn default_log_retention_days() -> u64 {
    730
}

fn default_balance_interval_min() -> u64 {
    10
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
    /// 余额查询适配声明（可选）：adapter 为内置实现名（见 balance_svc），params 为该 adapter 的自定义参数。
    /// 注意 ~/.mb/providers.json 对同 id provider 是整体替换：覆盖时若不带 usage 会一并丢失余额查询。
    #[serde(default)]
    pub usage: Option<UsageDef>,
}

/// 余额查询适配声明。params 接受哪些 key 由各 adapter 自行定义并严格校验（未知 key 报错）。
/// result/display 为声明式 http adapter（及未来用户扩展）服务，内置 adapter 忽略。
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UsageDef {
    pub adapter: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
    /// 点路径，指向响应里余额相关 JSON 子对象；None = 整份响应。仅 http adapter 消费。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// 前端展示模板（如 "¥{balance}"）；{path} 在落库载荷上做点路径取值。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
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
                log_retention_days: 730,
                balance_interval_min: 10,
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
        // 旧配置（无 probe_interval_min / balance_interval_min）应解析成功并取默认
        let cfg: BridgeConfig = toml::from_str("refresh_interval_min = 5\n").unwrap();
        assert_eq!(cfg.refresh_interval_min, 5);
        assert_eq!(cfg.probe_interval_min, 1440);
        assert_eq!(cfg.log_retention_days, 730);
        assert_eq!(cfg.balance_interval_min, 10);
    }

    #[test]
    fn bridge_respects_explicit_probe_interval() {
        let cfg: BridgeConfig = toml::from_str("refresh_interval_min = 5\nprobe_interval_min = 30\n").unwrap();
        assert_eq!(cfg.probe_interval_min, 30);
    }

    #[test]
    fn bridge_respects_explicit_balance_interval() {
        let cfg: BridgeConfig =
            toml::from_str("refresh_interval_min = 5\nbalance_interval_min = 3\n").unwrap();
        assert_eq!(cfg.balance_interval_min, 3);
    }

    #[test]
    fn usage_def_parses_adapter_and_params() {
        let def: ProviderDef = serde_json::from_str(
            r#"{"id":"x","name":"X","usage":{"adapter":"deepseek","params":{"endpoint":"https://gw.example.com/user/balance"}}}"#,
        ).unwrap();
        let usage = def.usage.unwrap();
        assert_eq!(usage.adapter, "deepseek");
        assert_eq!(usage.params["endpoint"], "https://gw.example.com/user/balance");
    }

    #[test]
    fn usage_absent_is_none_and_params_default_empty() {
        let def: ProviderDef = serde_json::from_str(r#"{"id":"x","name":"X"}"#).unwrap();
        assert!(def.usage.is_none());
        let def: ProviderDef =
            serde_json::from_str(r#"{"id":"x","name":"X","usage":{"adapter":"openrouter"}}"#).unwrap();
        assert!(def.usage.unwrap().params.is_empty());
    }

    #[test]
    fn builtin_providers_json_parses_with_usage() {
        // providers.json 编译期内嵌；四家余额均以声明式 http adapter 预置 usage 块
        let defs: Vec<ProviderDef> = serde_json::from_str(include_str!("../providers.json")).unwrap();
        for id in ["deepseek", "kimi", "siliconflow", "openrouter"] {
            let usage = defs.iter().find(|d| d.id == id).unwrap().usage.as_ref().unwrap();
            assert_eq!(usage.adapter, "http", "{}", id);
            assert!(usage.result.is_some() && usage.display.is_some(), "{}", id);
        }
    }

    #[test]
    fn usage_def_parses_result_and_display() {
        let def: ProviderDef = serde_json::from_str(
            r#"{"id":"trip","name":"Trip","usage":{"adapter":"http","params":{"url":"https://gw.example.com/balance"},"result":"data","display":"¥{balance}"}}"#,
        ).unwrap();
        let usage = def.usage.unwrap();
        assert_eq!(usage.adapter, "http");
        assert_eq!(usage.result.as_deref(), Some("data"));
        assert_eq!(usage.display.as_deref(), Some("¥{balance}"));

        // 缺省时两者为 None，旧配置不受影响
        let def: ProviderDef =
            serde_json::from_str(r#"{"id":"x","name":"X","usage":{"adapter":"openrouter"}}"#).unwrap();
        let usage = def.usage.unwrap();
        assert!(usage.result.is_none());
        assert!(usage.display.is_none());
    }
}