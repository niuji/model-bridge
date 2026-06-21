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
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

#[derive(Serialize)]
pub struct DailyStats {
    pub date: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
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
    let rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"
        SELECT
            model_id,
            COUNT(*) as cnt,
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0)
        FROM usage_records
        GROUP BY model_id
        ORDER BY cnt DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(model_id, request_count, total_input_tokens, total_output_tokens)| {
            ModelStats {
                model_id,
                request_count,
                total_input_tokens,
                total_output_tokens,
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
