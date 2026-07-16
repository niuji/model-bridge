use sqlx::SqlitePool;

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    // 删除旧表（Provider 从配置文件定义，DB 只存用户修改）
    sqlx::query("DROP TABLE IF EXISTS providers")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS provider_channels")
        .execute(pool)
        .await?;

    // Provider 用户配置（只存覆盖值）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_config (
            provider_id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL DEFAULT '',
            is_enabled INTEGER DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Channel 用户配置：base_url 一律以配置文件为准、不入库（仅存 channel 启用状态）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_channel_config (
            provider_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            is_enabled INTEGER DEFAULT 1,
            PRIMARY KEY (provider_id, channel_type)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 旧库可能仍残留 base_url 列，迁移时删掉（列已不存在则忽略错误）
    sqlx::query("ALTER TABLE provider_channel_config DROP COLUMN base_url")
        .execute(pool)
        .await
        .ok();

    // 模型列表（用户配置的数据）。channel_type 把模型按通道隔离——同一 provider 的
    // anthropic / openai 通道各有独立模型清单，UNIQUE(provider_id, channel_type, model_id)
    // 允许同一 model_id 存于不同通道（如 bigmodel 的 glm-4.7 同时走 openai_chat 与 anthropic）。
    // 旧表 UNIQUE(provider_id, model_id) 需迁移：先补 channel_type 列，再在单连接事务内
    // 重建表换约束——任一步失败整体回滚，绝不留搁浅表、绝不丢数据。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_models (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            channel_type TEXT NOT NULL DEFAULT '',
            model_id TEXT NOT NULL,
            model_name TEXT NOT NULL DEFAULT '',
            UNIQUE(provider_id, channel_type, model_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 为旧表添加 channel_type / model_name 列（列已存在则忽略）。必须在重建前补上，
    // 否则重建的 INSERT 引用 channel_type 会因列不存在而失败。
    sqlx::query("ALTER TABLE provider_models ADD COLUMN channel_type TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE provider_models ADD COLUMN model_name TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();

    // 旧约束 UNIQUE(provider_id, model_id) → 重建为 UNIQUE(provider_id, channel_type, model_id)。
    // 幂等：检测到表定义里尚无新约束才重建。单连接事务：DROP IF EXISTS _new → CREATE _new
    // → 拷贝 → DROP 旧 → RENAME；失败则 tx drop 自动回滚。
    let create_sql: Option<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='provider_models'",
    )
    .fetch_optional(pool)
    .await?;
    let need_rebuild = create_sql
        .map(|s| {
            !s.split_whitespace()
                .collect::<String>()
                .contains("UNIQUE(provider_id,channel_type,model_id)")
        })
        .unwrap_or(false);
    if need_rebuild {
        let mut tx = pool.begin().await?;
        sqlx::query("DROP TABLE IF EXISTS provider_models_new")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"CREATE TABLE provider_models_new (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                channel_type TEXT NOT NULL DEFAULT '',
                model_id TEXT NOT NULL,
                model_name TEXT NOT NULL DEFAULT '',
                UNIQUE(provider_id, channel_type, model_id)
            )"#,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO provider_models_new (id, provider_id, channel_type, model_id, model_name)
             SELECT id, provider_id, channel_type, model_id, model_name FROM provider_models",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE provider_models")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE provider_models_new RENAME TO provider_models")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!("provider_models unique migrated to (provider_id, channel_type, model_id)");
    }

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            key_hash TEXT UNIQUE NOT NULL,
            api_key TEXT NOT NULL DEFAULT '',
            name TEXT,
            is_enabled INTEGER DEFAULT 1,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("ALTER TABLE api_keys ADD COLUMN api_key TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await
        .ok();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS usage_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            model_id TEXT NOT NULL,
            provider_id TEXT NOT NULL,
            input_tokens INTEGER DEFAULT 0,
            output_tokens INTEGER DEFAULT 0,
            cache_read_tokens INTEGER DEFAULT 0,
            cache_write_tokens INTEGER DEFAULT 0,
            latency_ms INTEGER DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'success',
            error_msg TEXT,
            client TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 确保旧表有 cache 列
    sqlx::query("ALTER TABLE usage_records ADD COLUMN cache_read_tokens INTEGER DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE usage_records ADD COLUMN cache_write_tokens INTEGER DEFAULT 0")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE usage_records ADD COLUMN client TEXT")
        .execute(pool)
        .await
        .ok();
    sqlx::query("ALTER TABLE usage_records ADD COLUMN api_format TEXT")
        .execute(pool)
        .await
        .ok();
    // 通道类型（openai_chat / openai_responses / anthropic）：比 api_format 更细，
    // 区分同一 openai 入口下的 chat/completions 与 responses 两条上游通道。
    sqlx::query("ALTER TABLE usage_records ADD COLUMN channel TEXT")
        .execute(pool)
        .await
        .ok();

    // 上游模型快照：探测成功后按 (provider, channel) 整体替换；失败时保留旧值。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upstream_models (
            provider_id   TEXT NOT NULL,
            channel_type  TEXT NOT NULL,
            model_id      TEXT NOT NULL,
            model_name    TEXT NOT NULL DEFAULT '',
            last_seen_at  TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (provider_id, channel_type, model_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // baseline：上次打开"变更"弹窗时落地的上游快照（结构与 upstream_models 一致，无 last_seen_at）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS upstream_models_seen (
            provider_id   TEXT NOT NULL,
            channel_type  TEXT NOT NULL,
            model_id      TEXT NOT NULL,
            model_name    TEXT NOT NULL DEFAULT '',
            PRIMARY KEY (provider_id, channel_type, model_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}