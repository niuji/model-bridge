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

    // Channel 用户配置（base_url 为 NULL 表示使用配置文件默认值）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_channel_config (
            provider_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            base_url TEXT,
            is_enabled INTEGER DEFAULT 1,
            PRIMARY KEY (provider_id, channel_type)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 模型列表（用户配置的数据）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS provider_models (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            model_id TEXT NOT NULL,
            UNIQUE(provider_id, model_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

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

    Ok(())
}