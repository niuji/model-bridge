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
    pub total_tokens: i64,
}

#[derive(Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
    pub api_key_preview: Option<String>,
    pub client: Option<String>,
    pub api_format: Option<String>,
    pub channel: Option<String>,
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

// 用命名字段的 FromRow 结构体而非大元组：sqlx 的 FromRow 仅实现到 16 元组，
// 加 channel 列后字段数达 17，超出元组上限；结构体无此限制且更易读。
#[derive(sqlx::FromRow)]
struct LogRow {
    id: i64,
    api_key_id: Option<String>,
    api_key_name: Option<String>,
    api_key: Option<String>,
    model_id: String,
    provider_id: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    latency_ms: i64,
    status: String,
    error_msg: Option<String>,
    client: Option<String>,
    api_format: Option<String>,
    channel: Option<String>,
    created_at: String,
}

pub async fn get_logs(
    pool: &SqlitePool,
    page: i64,
    page_size: i64,
    enc_key: Option<&[u8; 32]>,
) -> anyhow::Result<PaginatedLogs> {
    let page_size = page_size.clamp(1, 500);
    let page = page.max(1);

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_records")
        .fetch_one(pool)
        .await?;

    let offset = (page - 1) * page_size;
    let rows = sqlx::query_as::<_, LogRow>(
        r#"
        SELECT u.id, u.api_key_id, k.name AS api_key_name, k.api_key,
               u.model_id, u.provider_id,
               u.input_tokens, u.output_tokens, u.cache_read_tokens, u.cache_write_tokens,
               u.latency_ms, u.status, u.error_msg,
               u.client,
               u.api_format,
               u.channel,
               strftime('%Y-%m-%dT%H:%M:%SZ', u.created_at) as created_at
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
        .map(|r| {
            let api_key_preview = r
                .api_key
                .as_deref()
                .map(|k| crate::router::admin::mask_key(&crate::crypto::reveal(enc_key, k)));
            LogEntry {
                id: r.id,
                api_key_id: r.api_key_id,
                api_key_name: r.api_key_name,
                api_key_preview,
                client: r.client,
                api_format: r.api_format,
                channel: r.channel,
                model_id: r.model_id,
                provider_id: r.provider_id,
                input_tokens: r.input_tokens,
                output_tokens: r.output_tokens,
                cache_read_tokens: r.cache_read_tokens,
                cache_write_tokens: r.cache_write_tokens,
                latency_ms: r.latency_ms,
                status: r.status,
                error_msg: r.error_msg,
                created_at: r.created_at,
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
    // Generate the complete daily series over the window and LEFT JOIN the
    // aggregated usage onto it, so days with no requests still appear with zero
    // tokens (consistent with the hourly view).
    let days = days.clamp(1, 365);
    let bound = format!("-{}", days);
    let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"
        WITH RECURSIVE days(d) AS (
            SELECT DATE(datetime('now', ? || ' days'))
            UNION ALL
            SELECT DATE(d, '+1 day')
            FROM days
            WHERE d < DATE('now')
        ),
        agg AS (
            SELECT DATE(created_at) as d,
                   COUNT(*) as cnt,
                   SUM(input_tokens) as inp,
                   SUM(output_tokens) as outp,
                   SUM(input_tokens + output_tokens) as tot
            FROM usage_records
            WHERE created_at >= datetime('now', ? || ' days')
            GROUP BY DATE(created_at)
        )
        SELECT dd.d, COALESCE(a.cnt, 0), COALESCE(a.inp, 0), COALESCE(a.outp, 0), COALESCE(a.tot, 0)
        FROM days dd
        LEFT JOIN agg a ON a.d = dd.d
        ORDER BY dd.d ASC
        "#,
    )
    .bind(&bound)
    .bind(&bound)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(date, request_count, input_tokens, output_tokens, total_tokens)| DailyStats {
                date,
                request_count,
                input_tokens,
                output_tokens,
                total_tokens,
            },
        )
        .collect())
}

pub async fn get_hourly_stats(pool: &SqlitePool) -> anyhow::Result<Vec<HourlyStats>> {
    // Generate the complete hourly series over the last 7 days (UTC) and LEFT JOIN
    // the aggregated usage onto it, so hours with no requests still appear on the
    // chart with zero tokens instead of being silently dropped by the GROUP BY.
    let rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        WITH RECURSIVE hours(hour) AS (
            SELECT strftime('%Y-%m-%dT%H:00:00Z', datetime('now', '-7 days'))
            UNION ALL
            SELECT strftime('%Y-%m-%dT%H:00:00Z', datetime(hour, '+1 hour'))
            FROM hours
            WHERE hour < strftime('%Y-%m-%dT%H:00:00Z', datetime('now'))
        ),
        agg AS (
            SELECT strftime('%Y-%m-%dT%H:00:00Z', created_at) as hour,
                   SUM(input_tokens + output_tokens) as total_tokens
            FROM usage_records
            WHERE created_at >= datetime('now', '-7 days')
            GROUP BY strftime('%Y-%m-%dT%H:00:00Z', created_at)
        )
        SELECT h.hour, COALESCE(a.total_tokens, 0) as total_tokens
        FROM hours h
        LEFT JOIN agg a ON a.hour = h.hour
        ORDER BY h.hour ASC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(hour, total_tokens)| HourlyStats { hour, total_tokens })
        .collect())
}