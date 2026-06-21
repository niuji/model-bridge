mod admin;
mod config;
mod db;
mod middleware;
mod router;
mod state;

use clap::Parser;
use std::sync::Arc;
use tracing_subscriber;

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 解析命令行参数
    let cli = config::Cli::parse();
    let app_config = config::load_config(&cli)?;

    // 初始化数据库
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;
    let options = SqliteConnectOptions::from_str(&app_config.database.path)?
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await?;

    // 执行迁移
    db::schema::run_migrations(&pool).await?;

    // 构建应用状态
    let state = Arc::new(AppState {
        openai_routes: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        anthropic_routes: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        db: pool.clone(),
    });

    // 首次加载路由表
    if let Err(e) = admin::provider_svc::refresh_routes(&state).await {
        tracing::warn!("Initial route refresh failed (no providers configured yet): {}", e);
    }

    // 启动后台定时刷新
    let refresh_state = state.clone();
    let interval_min = app_config.bridge.refresh_interval_min;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            interval_min * 60,
        ));
        loop {
            interval.tick().await;
            if let Err(e) = admin::provider_svc::refresh_routes(&refresh_state).await {
                tracing::error!("Scheduled route refresh failed: {}", e);
            }
        }
    });

    // 构建路由
    let app = router::create_router(state);

    // 启动服务器
    let addr = format!("{}:{}", app_config.server.host, app_config.server.port);
    tracing::info!("model-bridge starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
