use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{AccessType, ChannelDef, ProviderDef};
use crate::db::models::{
    BalanceRow, BalanceSummary, ChannelDetail, ChannelDrift, DriftSummary, ModelEntry,
    ProviderChannelConfigRow, ProviderConfigRow, ProviderDetail, ProviderModel, ProviderSettings,
    ProviderSummary, UpstreamModelRow,
};
use crate::state::{AppState, ProviderRoute};

/// 合并 Provider 定义与用户 DB 覆盖，返回摘要列表
pub async fn list_providers(
    pool: &SqlitePool,
    defs: &[ProviderDef],
) -> anyhow::Result<Vec<ProviderSummary>> {
    let mut result = Vec::new();

    // 漂移计数：一次性载入上游当前快照与 baseline，按 provider 分组算对称差（避免 N+1）
    let current_all: Vec<UpstreamModelRow> = sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models",
    )
    .fetch_all(pool)
    .await?;
    let baseline_all: Vec<UpstreamModelRow> = sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models_seen",
    )
    .fetch_all(pool)
    .await?;
    let mut cur_by_prov: HashMap<String, Vec<UpstreamModelRow>> = HashMap::new();
    for r in current_all {
        cur_by_prov.entry(r.provider_id.clone()).or_default().push(r);
    }
    let mut base_by_prov: HashMap<String, Vec<UpstreamModelRow>> = HashMap::new();
    for r in baseline_all {
        base_by_prov.entry(r.provider_id.clone()).or_default().push(r);
    }

    let balance_all: Vec<BalanceRow> = sqlx::query_as::<_, BalanceRow>(
        "SELECT provider_id, adapter, status, data, error_msg, fetched_at FROM provider_balance",
    )
    .fetch_all(pool)
    .await?;
    let mut balance_by_prov: HashMap<String, BalanceRow> = HashMap::new();
    for r in balance_all {
        balance_by_prov.insert(r.provider_id.clone(), r);
    }

    for def in defs {
        let config = get_provider_config(pool, &def.id).await?;
        let channel_configs = get_channel_configs(pool, &def.id).await?;

        let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
        let mut channels = merge_channels(&def.channels, &channel_configs);

        // 按通道统计模型数：UNIQUE(provider_id, channel_type, model_id) 保证同一通道内无重复，
        // 故每通道 COUNT(*) 即该通道去重后的模型数；卡片据此「区分通道」展示。
        let counts: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>(
            "SELECT channel_type, COUNT(*) FROM provider_models WHERE provider_id = ? GROUP BY channel_type",
        )
        .bind(&def.id)
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect();
        for ch in &mut channels {
            ch.model_count = *counts.get(&ch.channel_type).unwrap_or(&0);
        }

        let drift = {
            let cur = cur_by_prov.get(&def.id).map(|v| v.as_slice()).unwrap_or(&[]);
            let base = base_by_prov.get(&def.id).map(|v| v.as_slice()).unwrap_or(&[]);
            let d = compute_drift(cur, base);
            let new = d.iter().map(|c| c.added.len() as i64).sum();
            let removed = d.iter().map(|c| c.removed.len() as i64).sum();
            Some(DriftSummary { new, removed })
        };
        result.push(ProviderSummary {
            id: def.id.clone(),
            name: def.name.clone(),
            icon: def.icon.clone(),
            console_url: def.console_url.clone(),
            is_enabled,
            has_cost_api_key: config.as_ref().is_some_and(|c| !c.cost_api_key.is_empty()),
            channels,
            drift,
            usage: def.usage.clone(),
            balance: balance_by_prov.remove(&def.id).map(BalanceSummary::from),
            config_error: def.config_error.clone(),
        });
    }
    Ok(result)
}

/// 合并 Provider 定义与用户 DB 覆盖，返回详情
pub async fn get_provider(
    pool: &SqlitePool,
    defs: &[ProviderDef],
    id: &str,
) -> anyhow::Result<Option<ProviderDetail>> {
    let Some(def) = defs.iter().find(|d| d.id == id) else {
        return Ok(None);
    };

    let config = get_provider_config(pool, id).await?;
    let channel_configs = get_channel_configs(pool, id).await?;

    let api_key = config.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
    let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
    let mut channels = merge_channels(&def.channels, &channel_configs);

    let models = sqlx::query_as::<_, ProviderModel>(
        "SELECT id, provider_id, channel_type, model_id, model_name FROM provider_models WHERE provider_id = ? ORDER BY model_id",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    // 与列表卡片一致：按通道去重计数。
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for m in &models {
        *counts.entry(m.channel_type.as_str()).or_insert(0) += 1;
    }
    for ch in &mut channels {
        ch.model_count = *counts.get(ch.channel_type.as_str()).unwrap_or(&0);
    }

    Ok(Some(ProviderDetail {
        id: def.id.clone(),
        name: def.name.clone(),
        icon: def.icon.clone(),
        api_key,
        workspace_id: config
            .as_ref()
            .map(|c| c.workspace_id.clone())
            .unwrap_or_default(),
        has_cost_api_key: config.as_ref().is_some_and(|c| !c.cost_api_key.is_empty()),
        is_enabled,
        channels,
        models,
    }))
}

/// 从 DB 加载所有 enabled providers，构建路由表
pub async fn refresh_routes(state: &Arc<AppState>) -> anyhow::Result<()> {
    let mut openai_chat_routes: HashMap<String, ProviderRoute> = HashMap::new();
    let mut openai_responses_routes: HashMap<String, ProviderRoute> = HashMap::new();
    let mut anthropic_routes: HashMap<String, ProviderRoute> = HashMap::new();

    // 单遍加载所有启用 provider 的配置/通道/模型，避免预扫描与主循环重复查询（3N → N 次 DB 查询）。
    struct LoadedProvider<'a> {
        def: &'a ProviderDef,
        api_key: String,
        workspace_id: String,
        channels: Vec<ChannelDetail>,
        models: Vec<ProviderModel>,
    }
    let mut loaded: Vec<LoadedProvider> = Vec::new();
    for def in &state.provider_defs {
        // 声明层校验失败 → 整体跳过，一条路由都不建。DB 的 is_enabled 不动：
        // 配置错误是声明层问题，篡改用户的启用意图会让人修好 JSON 后还得手动重新启用。
        // 此处用 debug：load_providers 启动时已 warn 过一次，按刷新周期重复 warn 是噪声。
        if let Some(err) = &def.config_error {
            tracing::debug!("provider '{}' skipped from routing: {}", def.id, err);
            continue;
        }
        let config = get_provider_config(&state.db, &def.id).await?;
        let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
        if !is_enabled {
            continue;
        }
        let api_key = config.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
        match def.access_type {
            AccessType::ApiKey if api_key.is_empty() => continue,
            AccessType::Subscription if state.subscription.summary(&def.id).await?.status != "authorized" => continue,
            _ => {}
        }
        let channel_configs = get_channel_configs(&state.db, &def.id).await?;
        let channels = merge_channels(&def.channels, &channel_configs);
        let models = sqlx::query_as::<_, ProviderModel>(
            "SELECT id, provider_id, channel_type, model_id, model_name FROM provider_models WHERE provider_id = ?",
        )
        .bind(&def.id)
        .fetch_all(&state.db)
        .await?;
        let workspace_id = config
            .as_ref()
            .map(|c| c.workspace_id.clone())
            .unwrap_or_default();
        loaded.push(LoadedProvider {
            def,
            api_key,
            workspace_id,
            channels,
            models,
        });
    }

    // ---- 预扫描：跨所有 provider 统计归一化裸名冲突 ----
    // anthropic 统一统计（bare = to_lowercase → 剥 [1m] → 非 claude/anthropic 开头补 claude- 前缀）；
    // openai chat/responses 各自独立统计（bare = 纯 to_lowercase）。
    let mut bare_counts: HashMap<String, usize> = HashMap::new();
    let mut openai_chat_bare_counts: HashMap<String, usize> = HashMap::new();
    let mut openai_responses_bare_counts: HashMap<String, usize> = HashMap::new();
    for lp in &loaded {
        let enabled: Vec<&ChannelDetail> = lp.channels
            .iter()
            .filter(|c| c.is_enabled && is_safe_base_url(&c.base_url))
            .collect();
        for ch in enabled.iter().copied().filter(|c| c.channel_type == "anthropic") {
            for model in lp.models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let lower = model.model_id.to_lowercase();
                let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
                let bare = if clean.starts_with("claude") || clean.starts_with("anthropic") {
                    clean.to_string()
                } else {
                    format!("claude-{}", clean)
                };
                *bare_counts.entry(bare).or_insert(0) += 1;
            }
        }
        for ch in enabled.iter().copied().filter(|c| c.channel_type == "openai_chat" || c.channel_type == "openai_responses") {
            let table: &mut HashMap<String, usize> = match ch.channel_type.as_str() {
                "openai_chat" => &mut openai_chat_bare_counts,
                "openai_responses" => &mut openai_responses_bare_counts,
                _ => unreachable!(),
            };
            for model in lp.models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let bare = model.model_id.to_lowercase();
                *table.entry(bare).or_insert(0) += 1;
            }
        }
    }

    // ---- 主循环：按冲突与否生成 key，构建三张路由表 ----
    use std::collections::hash_map::Entry;
    for lp in &loaded {
        let def = lp.def;
        let api_key = &lp.api_key;

        // 拒绝非 http(s) 的 base_url（file:// 等），避免被导向本地资源
        for c in lp.channels.iter().filter(|c| c.is_enabled && !is_safe_base_url(&c.base_url)) {
            tracing::warn!(
                "provider '{}' channel '{}' base_url '{}' is not http(s), excluded from routing",
                def.id, c.channel_type, c.base_url
            );
        }
        let enabled: Vec<&ChannelDetail> = lp.channels
            .iter()
            .filter(|c| c.is_enabled && is_safe_base_url(&c.base_url))
            .collect();

        // 模型按通道隔离：channel_type 为空（迁移残留）的跳过路由并告警
        for m in lp.models.iter().filter(|m| m.channel_type.is_empty()) {
            tracing::warn!(
                "provider '{}' model '{}' has empty channel_type, skipped from routing",
                def.id, m.model_id
            );
        }

        // ---- anthropic 路由：每个启用的 anthropic 通道，插入「归属该通道」的模型 ----
        // 检索 key 由 model_id 派生（剥 [1m] 后缀；非 claude/anthropic 开头的补 claude- 前缀），
        // 与 proxy 转发剥除 [1m] 的逻辑配套。
        //
        // 两遍构建：先统计归一化裸名出现次数判定冲突，再按冲突与否生成 key。
        //   - 非冲突模型：只用裸名 key（count==1 保证唯一，直接 insert）。
        //   - 冲突模型（归一化裸名 count>1，含同 provider 归一化同名）：
        //     只用 `claude-{provider}/{model}` 限定名 key，裸名 key 完全不建；
        //     model_name 改写为带 [{provider_id}] 前缀（列表侧区分同名来源，转发不看它）。
        for ch in enabled.iter().copied().filter(|c| c.channel_type == "anthropic") {
            for model in lp.models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let route = ProviderRoute { access_type: def.access_type,
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                    workspace_id: lp.workspace_id.clone(),
                };
                let lower = route.model_id.to_lowercase();
                let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
                let bare = if clean.starts_with("claude") || clean.starts_with("anthropic") {
                    clean.to_string()
                } else {
                    format!("claude-{}", clean)
                };
                if bare_counts.get(&bare).copied().unwrap_or(0) == 1 {
                    // 非冲突：裸名 key（count==1 保证唯一，无冲突分支）
                    anthropic_routes.insert(bare, route);
                } else {
                    // 冲突：只用限定名 key；model_name 打上 [{provider_id}] 前缀（列表侧区分来源）
                    let clean_id = clean.strip_prefix("claude-").unwrap_or(clean);
                    // def.id 必须转小写：代理查找侧总是 to_lowercase，限定名 key 若嵌入
                    // 大写 id（用户自定义 provider 合法形态），冲突模型将彻底不可达
                    let qualified_key = format!("claude-{}/{}", def.id.to_lowercase(), clean_id);
                    // 先把 model_name 打上 [{provider_id}] 前缀（Vacant 入库与 Occupied 覆盖均需此前缀）
                    let mut prefixed_route = route;
                    prefixed_route.model_name = format!("[{}]{}", def.id, prefixed_route.model_name);
                    // 同 provider 归一化同名（如 claude-sonnet-4 与 claude-sonnet-4[1M]）等边缘场景下
                    // 限定名 key 会撞车：优先保留 [1m] 变体（让 Claude Code 在客户端开启 1M 上下文），
                    // 其余情况保留先入者 + warn。跨 provider 不会撞车（qualified_key 含 provider_id）。
                    match anthropic_routes.entry(qualified_key) {
                        Entry::Vacant(v) => {
                            v.insert(prefixed_route);
                        }
                        Entry::Occupied(mut o) => {
                            let existing = o.get();
                            let incoming_is_1m = prefixed_route.model_id.to_lowercase().ends_with("[1m]");
                            let existing_is_1m = existing.model_id.to_lowercase().ends_with("[1m]");
                            if incoming_is_1m && !existing_is_1m && existing.provider_id == def.id {
                                tracing::debug!(
                                    "same-provider [1m] variant preferred: replacing model '{}' with '{}' on 'anthropic' channel for provider '{}'",
                                    existing.model_id, prefixed_route.model_id, def.id
                                );
                                o.insert(prefixed_route);
                            } else {
                                tracing::warn!(
                                    "model '{}' on 'anthropic' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                                    model.model_id, existing.provider_id, existing.base_url, def.id
                                );
                            }
                        }
                    }
                }
            }
        }

        // ---- openai 路由：chat 与 responses 各自独立建表，不再合并 ----
        // 每个启用的 openai 通道单独成一张路由表：openai_chat → openai_chat_routes，
        // openai_responses → openai_responses_routes。模型归属哪个通道就进哪张表，转发用该通道 base_url，
        // 无需按 path 过滤。跨 provider 同名 model_id 冲突时（按通道独立统计）改用限定名 key
        // `{provider_id}/{model_id}`，裸名 key 不建；不冲突的模型仍用裸名 key。
        for ch in enabled.iter().copied().filter(|c| c.channel_type != "anthropic") {
            let table: &mut HashMap<String, ProviderRoute> = match ch.channel_type.as_str() {
                "openai_chat" => &mut openai_chat_routes,
                "openai_responses" => &mut openai_responses_routes,
                other => {
                    tracing::warn!(
                        "provider '{}' channel '{}' has unknown openai channel type, skipped from routing",
                        def.id, other
                    );
                    continue;
                }
            };
            // 冲突计数表：按通道选对应的预扫描结果
            let bare_counts: &HashMap<String, usize> = match ch.channel_type.as_str() {
                "openai_chat" => &openai_chat_bare_counts,
                "openai_responses" => &openai_responses_bare_counts,
                _ => unreachable!(),
            };
            for model in lp.models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let key_lower = model.model_id.to_lowercase();
                let route = ProviderRoute { access_type: def.access_type,
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                    workspace_id: String::new(),
                };
                if bare_counts.get(&key_lower).copied().unwrap_or(0) == 1 {
                    // 非冲突：裸名 key（count==1 保证唯一，无冲突分支）
                    match table.entry(key_lower) {
                        Entry::Vacant(v) => {
                            v.insert(route);
                        }
                        Entry::Occupied(o) => {
                            // 理论上不可达（count==1），防御性保留
                            let existing = o.get();
                            tracing::warn!(
                                "model '{}' on '{}' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                                model.model_id, ch.channel_type, existing.provider_id, existing.base_url, def.id
                            );
                        }
                    }
                } else {
                    // 冲突：只用限定名 key；model_name 打上 [{provider_id}] 前缀（列表侧区分来源）
                    let mut prefixed_route = route;
                    prefixed_route.model_name = format!("[{}]{}", def.id, prefixed_route.model_name);
                    let qualified_key = format!("{}/{}", def.id.to_lowercase(), key_lower);
                    match table.entry(qualified_key) {
                        Entry::Vacant(v) => {
                            v.insert(prefixed_route);
                        }
                        Entry::Occupied(o) => {
                            let existing = o.get();
                            tracing::warn!(
                                "model '{}' on '{}' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                                model.model_id, ch.channel_type, existing.provider_id, existing.base_url, def.id
                            );
                        }
                    }
                }
            }
        }
    }

    {
        let mut c = state.openai_chat_routes.write().await;
        *c = openai_chat_routes;
    }
    {
        let mut r = state.openai_responses_routes.write().await;
        *r = openai_responses_routes;
    }
    {
        let mut a = state.anthropic_routes.write().await;
        *a = anthropic_routes;
    }

    tracing::info!(
        "Routes refreshed: {} chat models, {} responses models, {} anthropic models",
        state.openai_chat_routes.read().await.len(),
        state.openai_responses_routes.read().await.len(),
        state.anthropic_routes.read().await.len()
    );

    Ok(())
}

/// 更新 provider 用户配置（api_key, is_enabled, channels, models）
pub async fn update_provider(
    pool: &SqlitePool,
    id: &str,
    api_key: &str,
    is_enabled: bool,
    channels: &[(String, bool)],         // (channel_type, is_enabled)
    models: &[(String, String, String)], // (channel_type, model_id, model_name)
    settings: &ProviderSettings,
) -> anyhow::Result<()> {
    update_provider_inner(
        pool,
        id,
        Some(api_key),
        is_enabled,
        channels,
        models,
        settings,
    )
    .await
}

pub async fn update_subscription_provider(
    pool: &SqlitePool,
    id: &str,
    is_enabled: bool,
    channels: &[(String, bool)],
    models: &[(String, String, String)],
) -> anyhow::Result<()> {
    update_provider_inner(
        pool,
        id,
        None,
        is_enabled,
        channels,
        models,
        &ProviderSettings::default(),
    )
    .await
}

async fn update_provider_inner(
    pool: &SqlitePool,
    id: &str,
    api_key: Option<&str>,
    is_enabled: bool,
    channels: &[(String, bool)],         // (channel_type, is_enabled)
    models: &[(String, String, String)], // (channel_type, model_id, model_name)
    settings: &ProviderSettings,
) -> anyhow::Result<()> {
    let workspace_id = settings.workspace_id.as_deref().map(str::trim);
    let cost_api_key = settings.cost_api_key.as_deref().map(str::trim);
    if let Some(value) = workspace_id {
        reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| anyhow::anyhow!("invalid workspace_id header value"))?;
    }
    // 配置、通道与模型必须同时提交，失败时保留整份旧配置。
    let mut tx = pool.begin().await?;
    if api_key.is_some() {
        // 查询范围或凭证变化后，旧费用不能继续显示成新配置的数据。
        sqlx::query("DELETE FROM provider_balance WHERE provider_id = ? AND adapter = 'anthropic_cost'
        AND EXISTS (SELECT 1 FROM provider_api_key_credentials WHERE provider_id = ?
          AND (workspace_id != COALESCE(?, workspace_id) OR cost_api_key != COALESCE(?, cost_api_key)))")
        .bind(id).bind(id).bind(workspace_id).bind(cost_api_key)
        .execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO provider_config(provider_id,is_enabled) VALUES (?,?) ON CONFLICT(provider_id) DO UPDATE SET is_enabled=excluded.is_enabled")
        .bind(id).bind(is_enabled).execute(&mut *tx).await?;
    if let Some(api_key) = api_key {
        // An empty key is also the legacy API-key clear action. Subscription callers
        // do not persist credentials through this common configuration endpoint.
        if !api_key.is_empty()
            || sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM provider_api_key_credentials WHERE provider_id=?",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?
                > 0
            || workspace_id.is_some()
            || cost_api_key.is_some()
        {
            sqlx::query("INSERT INTO provider_api_key_credentials(provider_id,api_key,workspace_id,cost_api_key) VALUES (?,?,COALESCE(?,''),COALESCE(?,'')) ON CONFLICT(provider_id) DO UPDATE SET api_key=excluded.api_key,workspace_id=COALESCE(?,provider_api_key_credentials.workspace_id),cost_api_key=COALESCE(?,provider_api_key_credentials.cost_api_key)")
            .bind(id).bind(api_key).bind(workspace_id).bind(cost_api_key).bind(workspace_id).bind(cost_api_key)
            .execute(&mut *tx).await?;
        }
    }

    // upsert channel_configs：base_url 以配置文件为准、不持久化，仅存 channel 启用状态
    for (channel_type, enabled) in channels {
        sqlx::query(
            "INSERT INTO provider_channel_config (provider_id, channel_type, is_enabled) VALUES (?, ?, ?)
             ON CONFLICT(provider_id, channel_type) DO UPDATE SET is_enabled = excluded.is_enabled",
        )
        .bind(id)
        .bind(channel_type)
        .bind(*enabled as i32)
        .execute(&mut *tx)
        .await?;
    }

    // 替换模型列表（按通道），任一 INSERT 失败会回滚整个保存操作。
    sqlx::query("DELETE FROM provider_models WHERE provider_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    for (channel_type, model_id, model_name) in models {
        if model_id.is_empty() {
            continue;
        }
        let mid = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, channel_type, model_id, model_name) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&mid)
        .bind(id)
        .bind(channel_type)
        .bind(model_id)
        .bind(model_name)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(())
}

/// 从指定通道的 models_endpoint 拉取模型列表（不写入 DB），使用前端传入的 api_key
pub async fn fetch_models_from_api(
    client: &reqwest::Client,
    defs: &[ProviderDef],
    provider_id: &str,
    channel_type: &str,
    ui_api_key: &str,
    workspace_id: &str,
) -> anyhow::Result<Vec<(String, String)>> {
    let Some(def) = defs.iter().find(|d| d.id == provider_id) else {
        anyhow::bail!("provider not found");
    };
    let Some(ch) = def.channels.iter().find(|c| c.channel_type == channel_type) else {
        anyhow::bail!("channel '{}' not found on provider '{}'", channel_type, provider_id);
    };
    let endpoint = ch
        .models_endpoint
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("models_endpoint not configured for channel '{}'", channel_type))?;

    // 使用前端传入的 key
    let api_key = if ui_api_key.is_empty() {
        anyhow::bail!("api_key not configured");
    } else {
        ui_api_key.to_string()
    };

    let mut models = Vec::new();
    let mut seen_models = std::collections::HashSet::new();
    let mut cursors = std::collections::HashSet::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut req = client
            .get(endpoint)
            .timeout(std::time::Duration::from_secs(10));
        if ch.channel_type == "anthropic" {
            req = req
                .header("x-api-key", &api_key)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("anthropic-version", "2023-06-01");
            if !workspace_id.is_empty() {
                req = req.header("anthropic-workspace-id", workspace_id);
            }
            if let Some(after_id) = &cursor {
                req = req.query(&[("after_id", after_id)]);
            }
        } else {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("HTTP {}", resp.status());
        }
        let body: serde_json::Value = resp.json().await?;
        let data = body["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing model data array"))?;
        for model in data {
            let id = model["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| anyhow::anyhow!("missing model id"))?;
            if seen_models.insert(id.to_string()) {
                models.push((
                    id.to_string(),
                    model["display_name"].as_str().unwrap_or(id).to_string(),
                ));
            }
        }
        // 兼容不返回分页字段的 Anthropic-compatible 上游；只在明确有下一页时继续。
        if ch.channel_type != "anthropic" || body["has_more"] != true {
            break;
        }
        let next = body["last_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing last_id on paginated model response"))?;
        if data.is_empty() || !cursors.insert(next.to_string()) {
            anyhow::bail!("model pagination cursor did not advance");
        }
        cursor = Some(next.to_string());
    }
    if models.is_empty() {
        anyhow::bail!("empty model list");
    }

    Ok(models)
}

#[cfg(test)]
mod anthropic_model_tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn anthropic_models_collects_all_pages() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(wiremock::matchers::header("x-api-key", "key"))
            .and(wiremock::matchers::header("Authorization", "Bearer key"))
            .and(wiremock::matchers::header(
                "anthropic-workspace-id",
                "wrkspc_test",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "claude-a", "display_name": "A"}],
                "has_more": true, "last_id": "claude-a"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(query_param("after_id", "claude-a"))
            .and(wiremock::matchers::header("x-api-key", "key"))
            .and(wiremock::matchers::header("Authorization", "Bearer key"))
            .and(wiremock::matchers::header(
                "anthropic-workspace-id",
                "wrkspc_test",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "claude-b", "display_name": "B"}],
                "has_more": false, "last_id": "claude-b"
            })))
            .with_priority(1)
            .mount(&server)
            .await;
        let defs = vec![ProviderDef { access_type: crate::config::AccessType::ApiKey, adapter: None,
            id: "anthropic".into(),
            name: "Anthropic".into(),
            icon: None,
            console_url: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server.uri(),
                models_endpoint: Some(format!("{}/v1/models", server.uri())),
            }],
            usage: None,
            config_error: None,
        }];
        let models = fetch_models_from_api(
            &reqwest::Client::new(),
            &defs,
            "anthropic",
            "anthropic",
            "key",
            "wrkspc_test",
        )
        .await
        .unwrap();
        assert_eq!(
            models,
            vec![
                ("claude-a".into(), "A".into()),
                ("claude-b".into(), "B".into())
            ]
        );
    }

    #[tokio::test]
    async fn anthropic_models_rejects_incomplete_or_cyclic_pages() {
        for second_page in [
            ResponseTemplate::new(500),
            ResponseTemplate::new(200).set_body_json(json!({"data": [], "has_more": true})),
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [{"id": "a"}], "has_more": true, "last_id": "a"})),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "data": [{"id": "a"}], "has_more": true, "last_id": "a"
                })))
                .mount(&server)
                .await;
            Mock::given(query_param("after_id", "a"))
                .respond_with(second_page)
                .with_priority(1)
                .mount(&server)
                .await;
            let defs: Vec<ProviderDef> = serde_json::from_value(json!([{
                "id": "a", "name": "A", "channels": [{"type": "anthropic", "base_url": server.uri(), "models_endpoint": server.uri()}]
            }])).unwrap();
            assert!(fetch_models_from_api(
                &reqwest::Client::new(),
                &defs,
                "a",
                "anthropic",
                "key",
                ""
            )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn anthropic_settings_preserve_on_omission_and_clear_explicitly() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        let settings = ProviderSettings {
            workspace_id: Some(" wrkspc_a ".into()),
            cost_api_key: Some(" admin ".into()),
        };
        update_provider(&pool, "a", "inference", true, &[], &[], &settings)
            .await
            .unwrap();
        crate::admin::balance_svc::upsert_balance_ok(
            &pool,
            "a",
            "anthropic_cost",
            &json!({"cost_usd": 10}),
        )
        .await
        .unwrap();
        update_provider(
            &pool,
            "a",
            "inference",
            false,
            &[],
            &[],
            &Default::default(),
        )
        .await
        .unwrap();
        let config = get_provider_config(&pool, "a").await.unwrap().unwrap();
        assert_eq!(config.workspace_id, "wrkspc_a");
        assert_eq!(config.cost_api_key, "admin");
        assert!(crate::admin::balance_svc::read_balance_row(&pool, "a")
            .await
            .unwrap()
            .is_some());
        let clear = ProviderSettings {
            workspace_id: Some("".into()),
            cost_api_key: Some("".into()),
        };
        update_provider(&pool, "a", "inference", true, &[], &[], &clear)
            .await
            .unwrap();
        let config = get_provider_config(&pool, "a").await.unwrap().unwrap();
        assert!(config.workspace_id.is_empty());
        assert!(config.cost_api_key.is_empty());
        assert!(crate::admin::balance_svc::read_balance_row(&pool, "a")
            .await
            .unwrap()
            .is_none());
    }
}

/// 迁移辅助：把 schema 迁移后 channel_type 为空的 provider_models 行按启发式回填。
/// claude/anthropic 前缀或含 [1M] 的归该 provider 的 anthropic 通道；
/// 其余归首个非 anthropic 通道；都不匹配则归首个通道。
/// 仅迁移期使用，正常运行无空值不触发。
pub async fn backfill_model_channels(pool: &SqlitePool, defs: &[ProviderDef]) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct Row {
        provider_id: String,
        model_id: String,
    }
    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT provider_id, model_id FROM provider_models WHERE channel_type = '' OR channel_type IS NULL",
    )
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    for Row {
        provider_id,
        model_id,
    } in rows
    {
        let lower = model_id.to_lowercase();
        let anthropic_style = lower.starts_with("claude")
            || lower.starts_with("anthropic")
            || lower.contains("[1m]");
        let target = defs
            .iter()
            .find(|d| d.id == provider_id)
            .and_then(|def| {
                if anthropic_style {
                    def.channels
                        .iter()
                        .find(|c| c.channel_type == "anthropic")
                        .map(|c| c.channel_type.clone())
                } else {
                    def.channels
                        .iter()
                        .find(|c| c.channel_type != "anthropic")
                        .map(|c| c.channel_type.clone())
                }
            })
            .or_else(|| {
                defs.iter()
                    .find(|d| d.id == provider_id)
                    .and_then(|d| d.channels.first().map(|c| c.channel_type.clone()))
            });
        let Some(target) = target else {
            tracing::warn!(
                "backfill: provider '{}' has no channels, model '{}' left unmapped",
                provider_id,
                model_id
            );
            continue;
        };
        sqlx::query("UPDATE provider_models SET channel_type = ? WHERE provider_id = ? AND model_id = ?")
            .bind(&target)
            .bind(&provider_id)
            .bind(&model_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

// ===== 内部辅助函数 =====

pub(crate) async fn get_provider_config(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<ProviderConfigRow>, sqlx::Error> {
    sqlx::query_as::<_, ProviderConfigRow>(
        "SELECT c.provider_id, c.is_enabled, COALESCE(k.api_key, '') AS api_key, COALESCE(k.workspace_id, '') AS workspace_id, COALESCE(k.cost_api_key, '') AS cost_api_key FROM provider_config c LEFT JOIN provider_api_key_credentials k USING(provider_id) WHERE c.provider_id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

async fn get_channel_configs(
    pool: &SqlitePool,
    provider_id: &str,
) -> Result<Vec<ProviderChannelConfigRow>, sqlx::Error> {
    sqlx::query_as::<_, ProviderChannelConfigRow>(
        "SELECT provider_id, channel_type, is_enabled FROM provider_channel_config WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await
}

/// 合并配置定义与 DB 覆盖：base_url / models_endpoint 一律以配置文件定义为准
/// （不允许 DB 自定义），仅 is_enabled 取 DB 覆盖值。
fn merge_channels(
    defs: &[ChannelDef],
    configs: &[ProviderChannelConfigRow],
) -> Vec<ChannelDetail> {
    defs.iter()
        .map(|def| {
            let is_enabled = configs
                .iter()
                .find(|c| c.channel_type == def.channel_type)
                .map(|c| c.is_enabled)
                .unwrap_or(true);
            ChannelDetail {
                channel_type: def.channel_type.clone(),
                base_url: def.base_url.clone(),
                models_endpoint: def.models_endpoint.clone(),
                is_enabled,
                model_count: 0,
            }
        })
        .collect()
}

/// 仅允许 http(s) 的 base_url，拒绝 file:// 等本地协议及非 URL 字符串。
pub fn is_safe_base_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// 对称差：current 相对 baseline 的新增（current 有、baseline 无）与下架（baseline 有、current 无）。
/// 按 (channel_type, model_id 小写) 比对——同通道大小写不敏感、跨通道不误配。model_name 变化不算（跳过改名）。
pub fn compute_drift(current: &[UpstreamModelRow], baseline: &[UpstreamModelRow]) -> Vec<ChannelDrift> {
    use std::collections::{HashMap, HashSet};
    fn key(r: &UpstreamModelRow) -> (String, String) {
        (r.channel_type.clone(), r.model_id.to_lowercase())
    }
    let base_keys: HashSet<(String, String)> = baseline.iter().map(key).collect();
    let cur_keys: HashSet<(String, String)> = current.iter().map(key).collect();

    let mut by_channel: HashMap<String, ChannelDrift> = HashMap::new();
    for r in current {
        if !base_keys.contains(&key(r)) {
            by_channel
                .entry(r.channel_type.clone())
                .or_insert_with(|| ChannelDrift {
                    channel_type: r.channel_type.clone(),
                    added: vec![],
                    removed: vec![],
                })
                .added
                .push(ModelEntry { model_id: r.model_id.clone(), model_name: r.model_name.clone() });
        }
    }
    for r in baseline {
        if !cur_keys.contains(&key(r)) {
            by_channel
                .entry(r.channel_type.clone())
                .or_insert_with(|| ChannelDrift {
                    channel_type: r.channel_type.clone(),
                    added: vec![],
                    removed: vec![],
                })
                .removed
                .push(ModelEntry { model_id: r.model_id.clone(), model_name: r.model_name.clone() });
        }
    }

    let mut out: Vec<ChannelDrift> = by_channel.into_values().collect();
    out.sort_by(|a, b| a.channel_type.cmp(&b.channel_type));
    out
}

/// 一轮探测的目标
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeTarget {
    pub provider_id: String,
    pub channel_type: String,
    pub models_endpoint: String,
    pub api_key: String,
}

/// 选出本轮要探测的 (provider, channel)：provider 已启用且有 api_key；
/// 通道已启用、有 models_endpoint、且 models_endpoint 为 http(s)（即真正会被 GET 的 URL）。
pub fn select_probe_targets(
    defs: &[ProviderDef],
    provider_enabled: &HashMap<String, bool>,
    provider_api_key: &HashMap<String, String>,
    channel_enabled: &HashMap<(String, String), bool>,
) -> Vec<ProbeTarget> {
    let mut out = Vec::new();
    for def in defs {
        if def.access_type != AccessType::ApiKey || def.config_error.is_some() {
            continue;
        }
        if !provider_enabled.get(&def.id).copied().unwrap_or(false) {
            continue;
        }
        let Some(api_key) = provider_api_key.get(&def.id).filter(|k| !k.is_empty()).cloned() else {
            continue;
        };
        for ch in &def.channels {
            let ch_on = channel_enabled
                .get(&(def.id.clone(), ch.channel_type.clone()))
                .copied()
                .unwrap_or(true);
            if !ch_on {
                continue;
            }
            let Some(ep) = ch.models_endpoint.as_deref() else { continue };
            if !is_safe_base_url(ep) {
                continue;
            }
            out.push(ProbeTarget {
                provider_id: def.id.clone(),
                channel_type: ch.channel_type.clone(),
                models_endpoint: ep.to_string(),
                api_key: api_key.clone(),
            });
        }
    }
    out
}

/// 计算并返回 drift（current vs 旧 baseline），然后落地 baseline（打开即清零）。
/// 初始化场景：baseline 为空时，所有 current 模型视为新增，确保首次打开即有提醒。
/// drift 读取与 baseline 落地在同一事务内：探针若在两者之间提交，要么整笔先于本事务
///（current 与 baseline 一致地纳入）、要么晚于本事务提交（下次再报），不会静默吞掉新模型。
pub async fn get_model_changes(pool: &SqlitePool, provider_id: &str) -> anyhow::Result<Vec<ChannelDrift>> {
    let mut tx = pool.begin().await?;
    let current = sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(&mut *tx)
    .await?;
    let baseline = sqlx::query_as::<_, UpstreamModelRow>(
        "SELECT provider_id, channel_type, model_id, model_name FROM upstream_models_seen WHERE provider_id = ?",
    )
    .bind(provider_id)
    .fetch_all(&mut *tx)
    .await?;
    let drift = compute_drift(&current, &baseline);
    sqlx::query("DELETE FROM upstream_models_seen WHERE provider_id = ?")
        .bind(provider_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO upstream_models_seen (provider_id, channel_type, model_id, model_name)
         SELECT provider_id, channel_type, model_id, model_name FROM upstream_models WHERE provider_id = ?",
    )
    .bind(provider_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(drift)
}

/// 用本轮探测结果整体替换某 (provider, channel) 的上游快照（事务）。
async fn replace_upstream_snapshot(
    pool: &SqlitePool,
    provider_id: &str,
    channel_type: &str,
    models: &[(String, String)],
    expected_account_id: Option<&str>,
) -> anyhow::Result<()> {
    // Account validation and replacement share the write lock so a completed
    // login cannot be followed by an old account's delayed snapshot publication.
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if let Some(account_id) = expected_account_id {
        let authorized: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM provider_subscription_accounts
             WHERE provider_id = ? AND account_id = ? AND auth_status = 'authorized')",
        )
        .bind(provider_id)
        .bind(account_id)
        .fetch_one(&mut *tx)
        .await?;
        if !authorized {
            tx.rollback().await?;
            return Ok(());
        }
    }
    sqlx::query("DELETE FROM upstream_models WHERE provider_id = ? AND channel_type = ?")
        .bind(provider_id).bind(channel_type).execute(&mut *tx).await?;
    for (id, name) in models {
        sqlx::query(
            "INSERT INTO upstream_models (provider_id, channel_type, model_id, model_name) VALUES (?, ?, ?, ?)",
        )
        .bind(provider_id).bind(channel_type).bind(id).bind(name)
        .execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 后台探测上游 /v1/models，刷新 upstream_models 快照。失败保留旧快照（仅 warn）。
pub async fn probe_upstream_models(state: &Arc<AppState>) -> anyhow::Result<()> {
    #[derive(sqlx::FromRow)]
    struct CfgRow {
        provider_id: String,
        is_enabled: bool,
        api_key: String,
        workspace_id: String,
    }
    #[derive(sqlx::FromRow)]
    struct ChCfgRow { provider_id: String, channel_type: String, is_enabled: bool }

    let cfgs: Vec<CfgRow> = sqlx::query_as::<_, CfgRow>(
        "SELECT c.provider_id,c.is_enabled,COALESCE(k.api_key,'') AS api_key,COALESCE(k.workspace_id,'') AS workspace_id FROM provider_config c LEFT JOIN provider_api_key_credentials k USING(provider_id)",
    )
    .fetch_all(&state.db).await?;
    let ch_cfgs: Vec<ChCfgRow> = sqlx::query_as::<_, ChCfgRow>(
        "SELECT provider_id, channel_type, is_enabled FROM provider_channel_config",
    )
    .fetch_all(&state.db).await?;

    let mut provider_enabled: HashMap<String, bool> = HashMap::new();
    let mut provider_api_key: HashMap<String, String> = HashMap::new();
    for c in &cfgs {
        provider_enabled.insert(c.provider_id.clone(), c.is_enabled);
        provider_api_key.insert(c.provider_id.clone(), c.api_key.clone());
    }
    let mut channel_enabled: HashMap<(String, String), bool> = HashMap::new();
    for c in &ch_cfgs {
        channel_enabled.insert((c.provider_id.clone(), c.channel_type.clone()), c.is_enabled);
    }

    for def in state.provider_defs.iter().filter(|d| d.access_type == AccessType::Subscription && d.config_error.is_none()) {
        if !provider_enabled.get(&def.id).copied().unwrap_or(false) ||
            !channel_enabled.get(&(def.id.clone(), "openai_responses".into())).copied().unwrap_or(true) { continue; }
        let summary = state.subscription.summary(&def.id).await?;
        if summary.status != "authorized" { continue; }
        let Some(account_id) = summary.account_id else { continue; };
        match state.subscription.models(&def.id).await {
            Ok(models) => replace_upstream_snapshot(&state.db, &def.id, "openai_responses", &models, Some(&account_id)).await?,
            Err(_) => tracing::warn!(provider_id=%def.id, "subscription model discovery failed"),
        }
    }

    let targets = select_probe_targets(
        &state.provider_defs, &provider_enabled, &provider_api_key, &channel_enabled,
    );

    let workspaces: HashMap<_, _> = cfgs
        .iter()
        .map(|c| (c.provider_id.as_str(), c.workspace_id.as_str()))
        .collect();
    for t in targets {
        tracing::debug!(
            "probing upstream '{}' '{}' at {}",
            t.provider_id,
            t.channel_type,
            t.models_endpoint
        );
        match fetch_models_from_api(
            &state.client,
            &state.provider_defs,
            &t.provider_id,
            &t.channel_type,
            &t.api_key,
            workspaces
                .get(t.provider_id.as_str())
                .copied()
                .unwrap_or_default(),
        )
        .await
        {
            Ok(models) => {
                if let Err(e) = replace_upstream_snapshot(&state.db, &t.provider_id, &t.channel_type, &models, None).await {
                    tracing::warn!("upstream snapshot persist failed for '{}' '{}': {}", t.provider_id, t.channel_type, e);
                }
            }
            Err(e) => tracing::warn!("upstream probe failed for '{}' '{}': {}", t.provider_id, t.channel_type, e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod drift_tests {
    use super::*;
    use crate::config::{ChannelDef, ProviderDef};
    use crate::db::models::{ChannelDrift, ModelEntry, UpstreamModelRow};
    use sqlx::SqlitePool;

    fn row(ct: &str, id: &str, name: &str) -> UpstreamModelRow {
        UpstreamModelRow {
            provider_id: "p".into(),
            channel_type: ct.into(),
            model_id: id.into(),
            model_name: name.into(),
        }
    }

    #[test]
    fn added_when_current_has_baseline_missing() {
        let cur = vec![row("openai_chat", "glm-5.2-air", "GLM 5.2 Air")];
        let d = compute_drift(&cur, &[]);
        assert_eq!(
            d,
            vec![ChannelDrift {
                channel_type: "openai_chat".into(),
                added: vec![ModelEntry { model_id: "glm-5.2-air".into(), model_name: "GLM 5.2 Air".into() }],
                removed: vec![],
            }]
        );
    }

    #[test]
    fn removed_when_baseline_has_current_missing() {
        let base = vec![row("openai_chat", "glm-4-air", "GLM 4 Air")];
        let d = compute_drift(&[], &base);
        assert_eq!(
            d,
            vec![ChannelDrift {
                channel_type: "openai_chat".into(),
                added: vec![],
                removed: vec![ModelEntry { model_id: "glm-4-air".into(), model_name: "GLM 4 Air".into() }],
            }]
        );
    }

    #[test]
    fn case_insensitive_match_not_flagged() {
        // current GLM-5.2 / baseline glm-5.2 → 同一模型，不算新增也不算下架
        let cur = vec![row("openai_chat", "GLM-5.2", "x")];
        let base = vec![row("openai_chat", "glm-5.2", "y")];
        assert_eq!(compute_drift(&cur, &base), vec![]);
    }

    #[test]
    fn per_channel_isolation() {
        // 同 model_id 不同通道 → 互不抵消：anthropic 新增、openai_chat 下架
        let cur = vec![row("anthropic", "claude-x", "c")];
        let base = vec![row("openai_chat", "claude-x", "c")];
        let d = compute_drift(&cur, &base);
        let mut chans: Vec<(String, usize, usize)> = d
            .into_iter()
            .map(|c| (c.channel_type, c.added.len(), c.removed.len()))
            .collect();
        chans.sort();
        assert_eq!(
            chans,
            vec![("anthropic".to_string(), 1, 0), ("openai_chat".to_string(), 0, 1)]
        );
    }

    #[test]
    fn rename_only_is_ignored() {
        // 同 model_id、不同 model_name → 既不在 added 也不在 removed（跳过改名）
        let cur = vec![row("openai_chat", "glm-5.2", "new name")];
        let base = vec![row("openai_chat", "glm-5.2", "old name")];
        assert_eq!(compute_drift(&cur, &base), vec![]);
    }

    fn chan(ct: &str, base: &str, ep: Option<&str>) -> ChannelDef {
        ChannelDef { channel_type: ct.into(), base_url: base.into(), models_endpoint: ep.map(String::from) }
    }
    fn def(id: &str, chans: Vec<ChannelDef>) -> ProviderDef {
        ProviderDef { access_type: crate::config::AccessType::ApiKey, adapter: None, id: id.into(), name: id.into(), icon: None, console_url: None, channels: chans, usage: None, config_error: None }
    }

    #[test]
    fn api_key_probe_skips_subscription_and_invalid_definitions() {
        let mut subscription = def("sub", vec![chan("openai_responses", "https://x", Some("https://x/models"))]);
        subscription.access_type = AccessType::Subscription;
        let mut invalid = def("bad", subscription.channels.clone());
        invalid.config_error = Some("invalid".into());
        let enabled = [("sub".into(), true), ("bad".into(), true)].into();
        let keys = [("sub".into(), "residual".into()), ("bad".into(), "key".into())].into();
        assert!(select_probe_targets(&[subscription, invalid], &enabled, &keys, &Default::default()).is_empty());
    }

    #[test]
    fn probe_skips_disabled_provider_and_missing_key() {
        let defs = vec![def("a", vec![chan("openai_chat", "https://x/v1", Some("https://x/v1/models"))])];
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("a".to_string(), false);
        let mut keys = std::collections::HashMap::new();
        keys.insert("a".to_string(), "sk-1".to_string());
        assert!(select_probe_targets(&defs, &enabled, &keys, &Default::default()).is_empty());

        // 启用但无 key → 仍跳过
        enabled.insert("a".to_string(), true);
        keys.remove("a");
        assert!(select_probe_targets(&defs, &enabled, &keys, &Default::default()).is_empty());
    }

    #[test]
    fn probe_skips_disabled_channel_missing_endpoint_nonhttp() {
        let defs = vec![def("a", vec![
            chan("openai_chat", "https://x/v1", Some("https://x/v1/models")),       // 命中
            chan("openai_responses", "https://x/v1", Some("https://x/v1/models2")), // 通道关
            chan("anthropic", "https://x/v1", None),                                // 无 endpoint
            chan("openai_chat2", "https://x/v1", Some("file:///etc/passwd")),       // 非 http
        ])];
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("a".to_string(), true);
        let mut keys = std::collections::HashMap::new();
        keys.insert("a".to_string(), "sk-1".to_string());
        let mut ch_en = std::collections::HashMap::new();
        ch_en.insert(("a".to_string(), "openai_responses".to_string()), false);

        let t = select_probe_targets(&defs, &enabled, &keys, &ch_en);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].channel_type, "openai_chat");
        assert_eq!(t[0].models_endpoint, "https://x/v1/models");
    }

    async fn mempool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        pool
    }

    async fn insert_up(pool: &SqlitePool, pid: &str, ct: &str, rows: &[(&str, &str)]) {
        for (id, name) in rows {
            sqlx::query(
                "INSERT INTO upstream_models (provider_id, channel_type, model_id, model_name) VALUES (?, ?, ?, ?)",
            )
            .bind(pid).bind(ct).bind(id).bind(name)
            .execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn subscription_snapshot_discards_old_account_and_logged_out_results() {
        let pool = mempool().await;
        sqlx::query("INSERT INTO provider_subscription_accounts VALUES ('p','new-account',NULL,'encrypted','encrypted',9999999999,'version','authorized',0)")
            .execute(&pool).await.unwrap();
        let current = vec![("new-model".into(), "New model".into())];
        replace_upstream_snapshot(&pool, "p", "openai_responses", &current, Some("new-account")).await.unwrap();
        let stale = vec![("old-model".into(), "Old model".into())];
        replace_upstream_snapshot(&pool, "p", "openai_responses", &stale, Some("old-account")).await.unwrap();
        let models: Vec<String> = sqlx::query_scalar("SELECT model_id FROM upstream_models WHERE provider_id='p'").fetch_all(&pool).await.unwrap();
        assert_eq!(models, ["new-model"]);
        sqlx::query("UPDATE provider_subscription_accounts SET auth_status='reauth_required' WHERE provider_id='p'").execute(&pool).await.unwrap();
        replace_upstream_snapshot(&pool, "p", "openai_responses", &stale, Some("new-account")).await.unwrap();
        let models: Vec<String> = sqlx::query_scalar("SELECT model_id FROM upstream_models WHERE provider_id='p'").fetch_all(&pool).await.unwrap();
        assert_eq!(models, ["new-model"]);
        sqlx::query("DELETE FROM provider_subscription_accounts WHERE provider_id='p'").execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM upstream_models WHERE provider_id='p'").execute(&pool).await.unwrap();
        replace_upstream_snapshot(&pool, "p", "openai_responses", &stale, Some("new-account")).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upstream_models WHERE provider_id='p'").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn model_changes_returns_drift_then_lands_baseline() {
        let pool = mempool().await;
        // 当前快照：a, b（openai_chat 通道）
        insert_up(&pool, "p", "openai_chat", &[("a", "A"), ("b", "B")]).await;

        // 第一次：baseline 为空 → 所有 current 模型视为新增（首次提醒）
        let d1 = get_model_changes(&pool, "p").await.unwrap();
        let d1_added: Vec<String> = d1.iter().flat_map(|c| c.added.iter().map(|m| m.model_id.clone())).collect();
        assert_eq!(d1_added, vec!["a".to_string(), "b".to_string()]);
        assert!(d1.iter().flat_map(|c| c.removed.iter()).count() == 0);

        // 上游变化：下架 a、新增 c → current={b,c}
        sqlx::query("DELETE FROM upstream_models WHERE provider_id = 'p'")
            .execute(&pool).await.unwrap();
        insert_up(&pool, "p", "openai_chat", &[("b", "B"), ("c", "C")]).await;

        let d2 = get_model_changes(&pool, "p").await.unwrap();
        let added: Vec<String> = d2.iter().flat_map(|c| c.added.iter().map(|m| m.model_id.clone())).collect();
        let removed: Vec<String> = d2.iter().flat_map(|c| c.removed.iter().map(|m| m.model_id.clone())).collect();
        assert_eq!(added, vec!["c".to_string()]);
        assert_eq!(removed, vec!["a".to_string()]);

        // 第三次：baseline 已={b,c}、current={b,c} → 无变化
        let d3 = get_model_changes(&pool, "p").await.unwrap();
        assert!(d3.is_empty());
    }
}

#[cfg(test)]
mod config_error_tests {
    use super::*;
    use crate::config::{ChannelDef, ProviderDef};
    use crate::db::schema::run_migrations;
    use sqlx::SqlitePool;

    #[tokio::test]
    async fn subscription_routes_require_authorized_account_and_enabled_channel() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        update_subscription_provider(&pool, "sub", true,
            &[("openai_responses".into(), true)],
            &[("openai_responses".into(), "gpt-test".into(), "GPT".into())]).await.unwrap();
        let def: ProviderDef = serde_json::from_value(serde_json::json!({
            "id":"sub", "name":"Subscription", "access_type":"subscription", "adapter":"openai_chatgpt",
            "channels":[{"type":"openai_responses","base_url":"https://chatgpt.com/backend-api/codex"}]
        })).unwrap();
        let state = std::sync::Arc::new(crate::state::AppState {
            subscription: std::sync::Arc::new(crate::providers::openai_subscription::SubscriptionService::new(pool.clone(), reqwest::Client::new(), None)),
            admin_base_url: "http://localhost:10020".into(),
            updates: std::sync::Arc::new(crate::update::Manager::default()),
            usage_tasks: tokio_util::task::TaskTracker::new(),
            openai_chat_routes: Default::default(),
            openai_responses_routes: Default::default(),
            anthropic_routes: Default::default(),
            provider_defs: vec![def],
            db: pool,
            client: reqwest::Client::new(),
            api_key_cache: Default::default(),
            encryption_key: None,
            request_log_enabled: tokio::sync::RwLock::new(false),
            request_log_dir: std::env::temp_dir().join(format!("mb-request-log-{}", uuid::Uuid::new_v4())),
            proxy_base_url: "http://test".into(),
        });
        refresh_routes(&state).await.unwrap();
        assert!(state.openai_responses_routes.read().await.is_empty());
        sqlx::query("INSERT INTO provider_subscription_accounts VALUES ('sub', 'account', NULL, 'encrypted', 'encrypted', 1, 'version', 'authorized', 1)")
            .execute(&state.db).await.unwrap();
        refresh_routes(&state).await.unwrap();
        // Expiry alone leaves a refreshable account routable; no API Key row is needed.
        assert!(state.openai_responses_routes.read().await.contains_key("gpt-test"));
        sqlx::query("UPDATE provider_channel_config SET is_enabled=0 WHERE provider_id='sub'").execute(&state.db).await.unwrap();
        refresh_routes(&state).await.unwrap();
        assert!(state.openai_responses_routes.read().await.is_empty());
        sqlx::query("UPDATE provider_channel_config SET is_enabled=1 WHERE provider_id='sub'").execute(&state.db).await.unwrap();
        sqlx::query("UPDATE provider_subscription_accounts SET auth_status='reauth_required' WHERE provider_id='sub'").execute(&state.db).await.unwrap();
        refresh_routes(&state).await.unwrap();
        assert!(state.openai_responses_routes.read().await.is_empty());
        sqlx::query("DELETE FROM provider_subscription_accounts WHERE provider_id='sub'").execute(&state.db).await.unwrap();
        refresh_routes(&state).await.unwrap();
        assert!(state.openai_responses_routes.read().await.is_empty());
    }

    /// channel_type 重复的 provider 即使在 DB 里已启用且有 key，也不得建任何路由。
    #[tokio::test]
    async fn refresh_routes_skips_provider_with_config_error() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_config (provider_id, is_enabled) VALUES ('dup', 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_api_key_credentials(provider_id, api_key) VALUES ('dup', 'sk-x')").execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, channel_type, model_id, model_name)
             VALUES ('m1', 'dup', 'openai_chat', 'gpt-4o', 'gpt-4o')",
        ).execute(&pool).await.unwrap();

        let ch = |b: &str| ChannelDef {
            channel_type: "openai_chat".into(),
            base_url: b.into(),
            models_endpoint: None,
        };
        let mut def = ProviderDef { access_type: crate::config::AccessType::ApiKey, adapter: None,
            id: "dup".into(),
            name: "Dup".into(),
            icon: None,
            console_url: None,
            channels: vec![ch("https://a.example/v1"), ch("https://b.example/v1")],
            usage: None,
            config_error: None,
        };
        def.config_error = crate::config::validate_channel_types(&def);
        assert!(def.config_error.is_some(), "fixture must be invalid");

        let state = std::sync::Arc::new(crate::state::AppState {
            subscription: std::sync::Arc::new(crate::providers::openai_subscription::SubscriptionService::new(pool.clone(), reqwest::Client::new(), None)),
            admin_base_url: "http://localhost:10020".into(),
            updates: std::sync::Arc::new(crate::update::Manager::default()),
            usage_tasks: tokio_util::task::TaskTracker::new(),
            openai_chat_routes: Default::default(),
            openai_responses_routes: Default::default(),
            anthropic_routes: Default::default(),
            provider_defs: vec![def],
            db: pool,
            client: reqwest::Client::new(),
            api_key_cache: Default::default(),
            encryption_key: None,
            request_log_enabled: tokio::sync::RwLock::new(false),
            request_log_dir: std::env::temp_dir().join(format!("mb-request-log-{}", uuid::Uuid::new_v4())),
            proxy_base_url: "http://test".into(),
        });
        refresh_routes(&state).await.unwrap();

        assert!(state.openai_chat_routes.read().await.is_empty());
        assert!(state.openai_responses_routes.read().await.is_empty());
        assert!(state.anthropic_routes.read().await.is_empty());
    }
}

#[cfg(test)]
mod update_provider_tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn mempool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn subscription_save_preserves_residual_api_key_credentials() {
        let pool = mempool().await;
        update_provider(&pool, "p", "old-key", true, &[], &[], &ProviderSettings {
            workspace_id: Some("workspace".into()), cost_api_key: Some("cost".into()),
        }).await.unwrap();
        update_subscription_provider(&pool, "p", false,
            &[("openai_responses".into(), true)],
            &[("openai_responses".into(), "model".into(), "Model".into())]).await.unwrap();
        let saved = get_provider_config(&pool, "p").await.unwrap().unwrap();
        assert_eq!((saved.api_key.as_str(), saved.workspace_id.as_str(), saved.cost_api_key.as_str()), ("old-key", "workspace", "cost"));
        assert!(!saved.is_enabled);
        let model: String = sqlx::query_scalar("SELECT model_id FROM provider_models WHERE provider_id='p'").fetch_one(&pool).await.unwrap();
        assert_eq!(model, "model");
    }

    #[tokio::test]
    async fn failed_model_replace_rolls_back_entire_provider() {
        let pool = mempool().await;
        // 初始：p 已有模型 m1
        update_provider(
            &pool,
            "p",
            "k",
            true,
            &[("anthropic".into(), true)],
            &[("anthropic".into(), "m1".into(), "M1".into())],
            &Default::default(),
        )
        .await
        .unwrap();

        // 传入重复 (channel_type, model_id) → 第二条 INSERT 触发 UNIQUE 冲突，整体必须报错
        let dup = vec![
            ("anthropic".to_string(), "m2".to_string(), "M2".to_string()),
            ("anthropic".to_string(), "m2".to_string(), "dup".to_string()),
        ];
        let res = update_provider(
            &pool,
            "p",
            "new-key",
            false,
            &[("anthropic".into(), false)],
            &dup,
            &Default::default(),
        )
        .await;
        assert!(res.is_err());

        // 关键断言：DELETE 必须随事务回滚，原有 m1 不能丢
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT model_id FROM provider_models WHERE provider_id = 'p'")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, vec!["m1".to_string()]);
        let config: (String, bool) = sqlx::query_as(
            "SELECT api_key, is_enabled FROM provider_config JOIN provider_api_key_credentials USING(provider_id) WHERE provider_id = 'p'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(config, ("k".into(), true));
        let enabled: bool = sqlx::query_scalar(
            "SELECT is_enabled FROM provider_channel_config WHERE provider_id = 'p' AND channel_type = 'anthropic'",
        ).fetch_one(&pool).await.unwrap();
        assert!(enabled);
    }
}
