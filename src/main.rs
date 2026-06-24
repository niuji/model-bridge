mod admin;
mod config;
mod crypto;
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
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = sqlx::SqlitePool::connect_with(options).await?;

    // 执行迁移
    db::schema::run_migrations(&pool).await?;

    // 创建共享 HTTP 客户端（复用连接池）。禁用重定向：上游或被篡改的
    // base_url 不得通过 3xx 把请求导向内网地址（SSF 防护）。
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(20)
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    // 解析客户端 API key 的静态加密密钥（可选）
    let enc_raw = app_config
        .database
        .encryption_key
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let encryption_key = match enc_raw {
        Some(raw) => match crypto::parse_key(raw) {
            Some(key) => Some(key),
            None => {
                tracing::warn!(
                    "database.encryption_key is set but invalid (expected base64 of 32 bytes); client API keys will be stored in plaintext"
                );
                None
            }
        },
        None => {
            tracing::warn!(
                "database.encryption_key not set; client API keys are stored in plaintext. Set it to encrypt at rest."
            );
            None
        }
    };

    // admin 服务默认应绑定 loopback（admin API 无应用层鉴权）。若被显式放开则告警。
    if !matches!(app_config.admin.host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        tracing::warn!(
            "admin server bound to '{}' — the admin API is unauthenticated. Bind to 127.0.0.1 unless you accept the risk.",
            app_config.admin.host
        );
    }

    // 构建应用状态
    let provider_defs = config::load_providers(&app_config.providers_file)?;
    let state = Arc::new(AppState {
        openai_routes: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        anthropic_routes: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        provider_defs,
        db: pool.clone(),
        client,
        api_key_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        encryption_key,
    });

    // 首次加载路由表
    if let Err(e) = admin::provider_svc::refresh_routes(&state).await {
        tracing::warn!("Initial route refresh failed (no providers configured yet): {}", e);
    }

    // 首次加载 API Key 缓存
    if let Err(e) = middleware::auth::refresh_api_key_cache(&state).await {
        tracing::warn!("Initial API key cache refresh failed: {}", e);
    }

    // 启动后台定时刷新
    let refresh_state = state.clone();
    // 至少 1 分钟，避免配置为 0 时退化为忙循环。
    let interval_min = app_config.bridge.refresh_interval_min.max(1);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            interval_min * 60,
        ));
        loop {
            interval.tick().await;
            if let Err(e) = admin::provider_svc::refresh_routes(&refresh_state).await {
                tracing::error!("Scheduled route refresh failed: {}", e);
            }
            if let Err(e) = middleware::auth::refresh_api_key_cache(&refresh_state).await {
                tracing::error!("Scheduled API key cache refresh failed: {}", e);
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