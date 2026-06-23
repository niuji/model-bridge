use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Serialize)]
pub struct StatsOverview {
    pub total_requests: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub avg_latency_ms: f64,
    pub error_count: i64,
}

#[derive(Serialize)]
pub struct ModelStats {
    pub model_id: String,
    pub request_count: i64,
    pub total_tokens: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub cache_hit_rate: f64,
}

#[derive(Serialize)]
pub struct DailyStats {
    pub date: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
    pub api_key_preview: Option<String>,
    pub model_id: String,
    pub provider_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub latency_ms: i64,
    pub status: String,
    pub error_msg: Option<String>,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct PaginatedLogs {
    pub logs: Vec<LogEntry>,
    pub total: i64,
}

pub async fn get_logs(pool: &SqlitePool, page: i64, page_size: i64) -> anyhow::Result<PaginatedLogs> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_records")
        .fetch_one(pool)
        .await?;

    let offset = (page - 1).max(0) * page_size;
    let rows = sqlx::query_as::<_, (i64, Option<String>, Option<String>, Option<String>, String, String, i64, i64, i64, i64, i64, String, Option<String>, String)>(
        r#"
        SELECT u.id, u.api_key_id, k.name, k.api_key,
               u.model_id, u.provider_id,
               u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_write_tokens,
               u.latency_ms, u.status, u.error_msg,
               strftime('%Y-%m-%d %H:%M:%S', u.created_at) as created_at
        FROM usage_records u
        LEFT JOIN api_keys k ON u.api_key_id = k.id
        ORDER BY u.id DESC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(page_size)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let logs = rows
        .into_iter()
        .map(|(id, api_key_id, api_key_name, api_key_raw, model_id, provider_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, status, error_msg, created_at)| {
            let api_key_preview = api_key_raw.as_deref().map(crate::router::admin::mask_key);
            LogEntry {
                id,
                api_key_id,
                api_key_name,
                api_key_preview,
                model_id,
                provider_id,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                latency_ms,
                status,
                error_msg,
                created_at,
            }
        })
        .collect();

    Ok(PaginatedLogs { logs, total: total.0 })
}

#[derive(Serialize)]
pub struct HourlyStats {
    pub hour: String,
    pub total_tokens: i64,
}

pub async fn get_overview(pool: &SqlitePool) -> anyhow::Result<StatsOverview> {
    let row = sqlx::query_as::<_, (i64, i64, i64, f64, i64)>(
        r#"
        SELECT
            COUNT(*) as total,
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(AVG(latency_ms), 0.0),
            COALESCE(SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END), 0)
        FROM usage_records
        WHERE created_at >= datetime('now', '-7 days')
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(StatsOverview {
        total_requests: row.0,
        total_input_tokens: row.1,
        total_output_tokens: row.2,
        avg_latency_ms: row.3,
        error_count: row.4,
    })
}

pub async fn get_model_stats(pool: &SqlitePool) -> anyhow::Result<Vec<ModelStats>> {
    let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
        r#"
        SELECT
            model_id,
            COUNT(*) as cnt,
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(SUM(cache_read_tokens), 0),
            COALESCE(SUM(cache_write_tokens), 0)
        FROM usage_records
        WHERE created_at >= datetime('now', '-7 days')
        GROUP BY model_id
        ORDER BY cnt DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(model_id, request_count, total_input_tokens, total_output_tokens, cache_read_tokens, cache_write_tokens)| {
            let total_tokens = total_input_tokens + total_output_tokens;
            let cache_hit_rate = if total_input_tokens > 0 {
                (cache_read_tokens as f64 / total_input_tokens as f64 * 100.0 * 100.0).round() / 100.0
            } else {
                0.0
            };
            ModelStats {
                model_id,
                request_count,
                total_tokens,
                total_input_tokens,
                total_output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cache_hit_rate,
            }
        })
        .collect())
}

pub async fn get_daily_stats(pool: &SqlitePool, days: i64) -> anyhow::Result<Vec<DailyStats>> {
    let rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"
        SELECT
            DATE(created_at) as date,
            COUNT(*) as cnt,
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0)
        FROM usage_records
        WHERE created_at >= datetime('now', ? || ' days')
        GROUP BY DATE(created_at)
        ORDER BY date ASC
        "#,
    )
    .bind(format!("-{}", days))
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(date, request_count, input_tokens, output_tokens)| DailyStats {
                date,
                request_count,
                input_tokens,
                output_tokens,
            },
        )
        .collect())
}

pub async fn get_hourly_stats(pool: &SqlitePool) -> anyhow::Result<Vec<HourlyStats>> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT
            strftime('%Y-%m-%d %H:00', created_at) as hour,
            COALESCE(SUM(input_tokens + output_tokens), 0) as total_tokens
        FROM usage_records
        WHERE created_at >= datetime('now', '-7 days')
        GROUP BY strftime('%Y-%m-%d %H:00', created_at)
        ORDER BY hour ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(hour, total_tokens)| HourlyStats { hour, total_tokens })
        .collect())
}