use sqlx::SqlitePool;

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    migrate_provider_credentials(pool).await?;
    // 删除旧表（Provider 从配置文件定义，DB 只存用户修改）
    sqlx::query("DROP TABLE IF EXISTS providers")
        .execute(pool)
        .await?;
    sqlx::query("DROP TABLE IF EXISTS provider_channels")
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

    // 每条统计/日志查询都按 created_at 过滤或排序（prune 清理亦然），无索引时是全表扫描
    // + 临时 B-tree 排序；保留期默认 730 天，表只增不减，故必须建索引。
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_usage_records_created_at ON usage_records(created_at)",
    )
    .execute(pool)
    .await?;

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

    // provider 余额最新快照：定时探测 UPSERT 单行。失败只覆写 status/error_msg/fetched_at，
    // 保留上次成功的 data（上游抖动不清空余额展示）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_balance (
            provider_id TEXT PRIMARY KEY,
            adapter     TEXT NOT NULL,
            status      TEXT NOT NULL,
            data        TEXT,
            error_msg   TEXT,
            fetched_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// The backup is taken before any destructive credential migration. Old binaries
/// require restoring it; running them directly against the split schema is unsupported.
async fn migrate_provider_credentials(pool: &SqlitePool) -> anyhow::Result<()> {
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('provider_config')")
            .fetch_all(pool)
            .await?;
    let legacy = columns.iter().any(|c| c == "api_key");
    if legacy {
        let databases: Vec<(i64, String, String)> = sqlx::query_as("PRAGMA database_list")
            .fetch_all(pool)
            .await?;
        if let Some((_, _, file)) = databases
            .iter()
            .find(|(_, name, file)| name == "main" && !file.is_empty())
        {
            let backup = std::path::PathBuf::from(format!(
                "{file}.pre-subscription-{}.backup",
                uuid::Uuid::new_v4()
            ));
            crate::update::backup::snapshot(std::path::Path::new(file), &backup).await?;
            tracing::info!(path = %backup.display(), "saved pre-subscription database backup");
        }
    }
    let mut tx = pool.begin().await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_migrations (name TEXT PRIMARY KEY)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS provider_config (provider_id TEXT PRIMARY KEY, is_enabled INTEGER DEFAULT 0)")
        .execute(&mut *tx).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS provider_api_key_credentials (
        provider_id TEXT PRIMARY KEY,
        api_key TEXT NOT NULL DEFAULT '',
        workspace_id TEXT NOT NULL DEFAULT '',
        cost_api_key TEXT NOT NULL DEFAULT ''
    )",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS provider_subscription_accounts (
        provider_id TEXT PRIMARY KEY,
        account_id TEXT NOT NULL,
        account_label TEXT,
        access_token_encrypted TEXT NOT NULL,
        refresh_token_encrypted TEXT NOT NULL,
        expires_at INTEGER NOT NULL,
        credential_version TEXT NOT NULL,
        auth_status TEXT NOT NULL CHECK(auth_status IN ('authorized', 'reauth_required')),
        updated_at INTEGER NOT NULL
    )",
    )
    .execute(&mut *tx)
    .await?;
    if legacy {
        for column in ["workspace_id", "cost_api_key"] {
            if !columns.iter().any(|name| name == column) {
                sqlx::query(&format!(
                    "ALTER TABLE provider_config ADD COLUMN {column} TEXT NOT NULL DEFAULT ''"
                ))
                .execute(&mut *tx)
                .await?;
            }
        }
        sqlx::query("INSERT INTO provider_api_key_credentials (provider_id, api_key, workspace_id, cost_api_key)
            SELECT provider_id, api_key, workspace_id, cost_api_key FROM provider_config
            WHERE provider_id NOT IN (SELECT provider_id FROM provider_api_key_credentials)")
            .execute(&mut *tx).await?;
        let mismatches: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_config p
            LEFT JOIN provider_api_key_credentials c ON p.provider_id = c.provider_id
            WHERE c.provider_id IS NULL OR p.api_key IS NOT c.api_key
            OR p.workspace_id IS NOT c.workspace_id OR p.cost_api_key IS NOT c.cost_api_key",
        )
        .fetch_one(&mut *tx)
        .await?;
        anyhow::ensure!(
            mismatches == 0,
            "credential migration conflicts with existing credentials"
        );
        for column in ["api_key", "workspace_id", "cost_api_key"] {
            sqlx::query(&format!("ALTER TABLE provider_config DROP COLUMN {column}"))
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query(
        "INSERT OR IGNORE INTO schema_migrations (name) VALUES ('split_provider_credentials')",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn credential_conflict_rolls_back_without_overwriting() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE provider_config (provider_id TEXT PRIMARY KEY, api_key TEXT NOT NULL, is_enabled INTEGER)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_config VALUES ('p', 'old', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE provider_api_key_credentials (provider_id TEXT PRIMARY KEY, api_key TEXT NOT NULL, workspace_id TEXT NOT NULL, cost_api_key TEXT NOT NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_api_key_credentials VALUES ('p', 'new', '', '')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(run_migrations(&pool).await.is_err());
        let old: String = sqlx::query_scalar("SELECT api_key FROM provider_config")
            .fetch_one(&pool)
            .await
            .unwrap();
        let new: String = sqlx::query_scalar("SELECT api_key FROM provider_api_key_credentials")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(old, "old");
        assert_eq!(new, "new");
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('provider_config')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!columns.contains(&"workspace_id".to_string()));
    }

    #[tokio::test]
    async fn file_migration_backs_up_original_credentials() {
        let dir = std::env::temp_dir().join(format!("mb-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("database.db");
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await.unwrap();
        sqlx::query("CREATE TABLE provider_config (provider_id TEXT PRIMARY KEY, api_key TEXT NOT NULL, is_enabled INTEGER, workspace_id TEXT NOT NULL, cost_api_key TEXT NOT NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_config VALUES ('p', 'key', 1, 'workspace', 'cost')")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "backup"))
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&backups[0])
                .read_only(true),
        )
        .await
        .unwrap();
        let saved: (String, String, String, i64) = sqlx::query_as(
            "SELECT api_key, workspace_id, cost_api_key, is_enabled FROM provider_config",
        )
        .fetch_one(&backup)
        .await
        .unwrap();
        assert_eq!(saved, ("key".into(), "workspace".into(), "cost".into(), 1));
        let current: (String, String, String) = sqlx::query_as(
            "SELECT api_key, workspace_id, cost_api_key FROM provider_api_key_credentials",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(current, ("key".into(), "workspace".into(), "cost".into()));
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('provider_config')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(columns, ["provider_id", "is_enabled"]);
        backup.close().await;
        pool.close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn migrations_idempotent_and_create_provider_balance() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap(); // 幂等：二次执行不报错
        let name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='provider_balance'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(name.as_deref(), Some("provider_balance"));
    }

    #[tokio::test]
    async fn migration_preserves_existing_provider_credentials() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE provider_config (provider_id TEXT PRIMARY KEY, api_key TEXT NOT NULL, is_enabled INTEGER)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO provider_config VALUES ('anthropic', 'existing-key', 1)")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        let row: (String, String, String) = sqlx::query_as(
            "SELECT api_key, workspace_id, cost_api_key FROM provider_api_key_credentials",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, ("existing-key".into(), "".into(), "".into()));
        sqlx::query("UPDATE provider_api_key_credentials SET workspace_id = 'wrkspc_saved', cost_api_key = 'admin-saved'")
            .execute(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();
        let row: (String, String) =
            sqlx::query_as("SELECT workspace_id, cost_api_key FROM provider_api_key_credentials")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row, ("wrkspc_saved".into(), "admin-saved".into()));
    }
}
