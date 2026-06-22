mod admin;
mod config;
mod db;
mod middleware;
mod router;
mod state;

use clap::Parser;
use std::sync::Arc;

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

    // 创建共享 HTTP 客户端（复用连接池）
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(20)
        .build()?;

    // 构建应用状态
    let provider_defs = config::load_providers(&app_config.providers_file)?;
    let state = Arc::new(AppState {
        openai_routes: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        anthropic_routes: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        provider_defs,
        db: pool.clone(),
        client,
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
    let proxy_router = router::create_proxy_router(state.clone());
    let admin_router = router::create_admin_router(state);

    // 启动代理服务
    let proxy_addr = format!("{}:{}", app_config.proxy.host, app_config.proxy.port);
    let proxy_listener = tokio::net::TcpListener::bind(&proxy_addr).await?;
    tracing::info!("proxy service starting on {}", proxy_addr);

    // 启动管理服务
    let admin_addr = format!("{}:{}", app_config.admin.host, app_config.admin.port);
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr).await?;
    tracing::info!("admin service starting on {}", admin_addr);

    // 并行运行两个服务，任一退出则整体退出
    let proxy_svc = axum::serve(proxy_listener, proxy_router)
        .with_graceful_shutdown(shutdown_signal());
    let admin_svc = axum::serve(admin_listener, admin_router)
        .with_graceful_shutdown(shutdown_signal());

    tokio::select! {
        result = proxy_svc => {
            if let Err(e) = result {
                tracing::error!("proxy service error: {}", e);
            }
        }
        result = admin_svc => {
            if let Err(e) = result {
                tracing::error!("admin service error: {}", e);
            }
        }
    }

    tracing::info!("model-bridge shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received SIGINT, shutting down...");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, shutting down...");
        }
    }
}