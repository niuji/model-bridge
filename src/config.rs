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
    /// 注意 ~/.mb/providers.json 对同 id provider 是浅合并：只写 usage 即可为内置 provider 增配余额查询
    /// （channels/name 等自动继承），显式 null 才移除字段。
    #[serde(default)]
    pub usage: Option<UsageDef>,
    /// 声明层校验错误（由 load_providers 派生，非 JSON 字段——serde(skip) 确保用户写了也不生效）。
    /// Some 时该 provider 被 refresh_routes 跳过，不建任何路由；DB 的 is_enabled 不受影响。
    #[serde(skip)]
    pub config_error: Option<String>,
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
/// 校验单个 provider 的 channel 声明：channel_type 必须唯一。
///
/// channel_type 是通道的唯一判别键——`provider_models` 行、`refresh_routes` 的通道↔模型配对、
/// `upstream_models` 快照都按它索引。同 type 出现两次时无从分辨模型归属哪一个：外层通道循环
/// 会把同一批模型处理两遍，令冲突计数翻倍（裸名 key 退化为限定名，客户端原请求名 404），
/// 两个 base_url 还会争抢同一个路由 key。故这类声明整体拒绝，不做部分接受。
pub fn validate_channel_types(def: &ProviderDef) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut dups: Vec<&str> = Vec::new();
    for ch in &def.channels {
        let ct = ch.channel_type.as_str();
        if seen.contains(&ct) {
            if !dups.contains(&ct) {
                dups.push(ct);
            }
        } else {
            seen.push(ct);
        }
    }
    if dups.is_empty() {
        None
    } else {
        Some(format!("channel_type 重复: {}", dups.join(", ")))
    }
}

/// 加载 Provider 定义：编译期内嵌的 providers.json + ~/.mb/providers.json 用户自定义合并。
/// 用户自定义中同 id 浅合并覆盖（用户写的 key 赢，没写的继承内置；显式 null 才移除），新 id 追加。
/// 内置定义停在 Value 层参与合并——struct 层分不清「没写」和「写了个默认值」。
pub fn load_providers() -> anyhow::Result<Vec<ProviderDef>> {
    let builtin: Vec<serde_json::Value> = serde_json::from_str(include_str!("../providers.json"))?;

    // 用户文件可缺省；存在但读不出/JSON 语法坏 → warn 后弃用（内置定义照常生效）
    let user_values = match read_user_values() {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            tracing::warn!("failed to load user provider file ~/.mb/providers.json: {}", e);
            Vec::new()
        }
        None => Vec::new(),
    };

    // 条目缺 id 或合并后反序列化不过 → 同样整体弃用用户文件（沿用旧「单条目坏→全文件弃」，
    // 避免半套配置静默生效）。内置内容编译期内嵌、已知合法，此分支只可能由用户条目触发。
    let providers = match merge_user_defs(&builtin, user_values).and_then(|merged| defs_from_values(&merged)) {
        Ok(providers) => providers,
        Err(e) => {
            tracing::warn!("invalid user provider file ~/.mb/providers.json, using builtin definitions only: {}", e);
            defs_from_values(&builtin)?
        }
    };
    Ok(mark_config_errors(providers))
}

/// 读 ~/.mb/providers.json（文件可缺省）。存在但读不出/JSON 语法坏 → Err。
fn read_user_values() -> Option<anyhow::Result<Vec<serde_json::Value>>> {
    let path = dirs::home_dir()
        .map(|h| h.join(".mb").join("providers.json"))
        .filter(|p| p.exists())?;
    Some((|| -> anyhow::Result<Vec<serde_json::Value>> {
        let raw = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&raw)?)
    })())
}

fn defs_from_values(values: &[serde_json::Value]) -> anyhow::Result<Vec<ProviderDef>> {
    values
        .iter()
        .cloned()
        .map(|v| serde_json::from_value::<ProviderDef>(v).map_err(Into::into))
        .collect()
}

/// 用户覆盖条目与内置定义的浅合并：同 id 时用户条目写到的 key 覆盖、没写的 key 继承内置，
/// 新 id 追加。返回完整合并后的定义列表（Value 层，调用方再统一反序列化）。
/// 任一用户条目不是对象或缺 string id → Err：调用方整体弃用该文件（与旧「单条目坏→全文件弃」一致）。
fn merge_user_defs(builtin: &[serde_json::Value], user: Vec<serde_json::Value>) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut merged = builtin.to_vec();
    for entry in user {
        let obj = entry
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("user provider entry must be an object"))?;
        let id = obj
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("user provider entry missing string 'id'"))?;
        match merged
            .iter_mut()
            .find(|p| p.get("id").and_then(serde_json::Value::as_str) == Some(id))
        {
            Some(existing) => {
                tracing::info!("user provider '{}' overrides builtin definition", id);
                // 浅合并：用户写到的 key 覆盖，其余继承内置；数组/对象值整体替换，不做元素级合并
                let mut combined = existing.as_object().cloned().unwrap_or_default();
                combined.extend(obj.iter().map(|(k, v)| (k.clone(), v.clone())));
                *existing = serde_json::Value::Object(combined);
            }
            None => {
                tracing::info!("user provider '{}' added from ~/.mb/providers.json", id);
                merged.push(entry);
            }
        }
    }
    Ok(merged)
}

/// 给每个 provider 打上声明层校验结果。必须在用户覆盖合并「之后」跑：
/// 同 id 的用户条目可能引入/替换 channels，只校验内建层会漏掉用户引入的重复。
fn mark_config_errors(mut providers: Vec<ProviderDef>) -> Vec<ProviderDef> {
    for def in &mut providers {
        def.config_error = validate_channel_types(def);
        if let Some(err) = &def.config_error {
            tracing::warn!(
                "provider '{}' has an invalid channel declaration ({}); it will be skipped from routing",
                def.id, err
            );
        }
    }
    providers
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

    fn def_with_channels(types: &[&str]) -> ProviderDef {
        ProviderDef {
            id: "p".into(),
            name: "P".into(),
            icon: None,
            channels: types
                .iter()
                .map(|t| ChannelDef {
                    channel_type: (*t).into(),
                    base_url: "https://example.com/v1".into(),
                    models_endpoint: None,
                })
                .collect(),
            usage: None,
            config_error: None,
        }
    }

    #[test]
    fn distinct_channel_types_are_valid() {
        let d = def_with_channels(&["openai_chat", "openai_responses", "anthropic"]);
        assert_eq!(validate_channel_types(&d), None);
    }

    #[test]
    fn duplicate_channel_type_is_reported() {
        let d = def_with_channels(&["openai_chat", "openai_chat"]);
        assert_eq!(
            validate_channel_types(&d).as_deref(),
            Some("channel_type 重复: openai_chat")
        );
    }

    #[test]
    fn multiple_duplicate_channel_types_are_all_reported() {
        // 每个重复 type 只报一次（三个 openai_chat 不该报两遍），且顺序稳定
        let d = def_with_channels(&[
            "openai_chat",
            "anthropic",
            "openai_chat",
            "anthropic",
            "openai_chat",
        ]);
        assert_eq!(
            validate_channel_types(&d).as_deref(),
            Some("channel_type 重复: openai_chat, anthropic")
        );
    }

    #[test]
    fn no_channels_is_valid() {
        assert_eq!(validate_channel_types(&def_with_channels(&[])), None);
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

    // ---- ~/.mb/providers.json 同 id 浅合并 ----

    fn builtin_values() -> Vec<serde_json::Value> {
        serde_json::from_str(
            r#"[
              {"id":"volcark","name":"火山方舟","icon":"volcark.svg",
               "channels":[{"type":"openai_chat","base_url":"https://ark.cn-beijing.volces.com/api/v3"}]},
              {"id":"deepseek","name":"DeepSeek","channels":[],
               "usage":{"adapter":"http","params":{"url":"https://api.deepseek.com/user/balance"}}}
            ]"#,
        )
        .unwrap()
    }

    #[test]
    fn override_inherits_missing_keys_and_overrides_present_ones() {
        // 只写 id + usage：name/icon/channels 继承内置，usage 采用用户值
        let user: Vec<serde_json::Value> = serde_json::from_str(
            r#"[{"id":"volcark","usage":{"adapter":"volcengine","params":{"access_key":"AKLT1"}}}]"#,
        )
        .unwrap();
        let merged = merge_user_defs(&builtin_values(), user).unwrap();
        let volc = merged.iter().find(|v| v["id"] == "volcark").unwrap();
        assert_eq!(volc["name"], "火山方舟");
        assert_eq!(volc["channels"].as_array().unwrap().len(), 1);
        assert_eq!(volc["usage"]["adapter"], "volcengine");
        assert_eq!(volc["usage"]["params"]["access_key"], "AKLT1");
        // 合并结果须能反序列化成完整 ProviderDef（用户条目没写 name 也不缺字段）
        let def: ProviderDef = serde_json::from_value(volc.clone()).unwrap();
        assert_eq!(def.name, "火山方舟");
        assert_eq!(def.usage.unwrap().adapter, "volcengine");
    }

    #[test]
    fn override_replaces_channels_wholesale_and_removes_usage_via_null() {
        // channels 出现即整体替换（不做元素级合并）；usage 显式 null 才移除
        let user: Vec<serde_json::Value> = serde_json::from_str(
            r#"[{"id":"deepseek","channels":[{"type":"anthropic","base_url":"https://x.example.com"}],"usage":null}]"#,
        )
        .unwrap();
        let merged = merge_user_defs(&builtin_values(), user).unwrap();
        let ds = merged.iter().find(|v| v["id"] == "deepseek").unwrap();
        assert_eq!(ds["channels"].as_array().unwrap().len(), 1);
        assert_eq!(ds["channels"][0]["type"], "anthropic");
        let def: ProviderDef = serde_json::from_value(ds.clone()).unwrap();
        assert!(def.usage.is_none());
    }

    #[test]
    fn new_id_entry_appended_intact() {
        let user: Vec<serde_json::Value> =
            serde_json::from_str(r#"[{"id":"trip","name":"Trip","channels":[]}]"#).unwrap();
        let merged = merge_user_defs(&builtin_values(), user).unwrap();
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[2]["id"], "trip");
    }

    #[test]
    fn entry_without_string_id_is_rejected() {
        let user: Vec<serde_json::Value> = serde_json::from_str(r#"[{"name":"X"}]"#).unwrap();
        assert!(merge_user_defs(&builtin_values(), user).is_err());
    }

    #[test]
    fn non_object_entry_is_rejected() {
        let user: Vec<serde_json::Value> = serde_json::from_str(r#"["oops"]"#).unwrap();
        assert!(merge_user_defs(&builtin_values(), user).is_err());
    }
}