use serde::Serialize;
use sqlx::SqlitePool;

/// 删除超过 retention_days 天的 usage_records。
/// 返回被删除的行数。
pub async fn prune_old_records(pool: &SqlitePool, retention_days: u64) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "DELETE FROM usage_records WHERE created_at < datetime('now', ? || ' days')",
    )
    .bind(format!("-{}", retention_days))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

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
    pub provider_id: String,
    /// 历史未记录通道时为空字符串。
    pub channel: String,
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
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64, i64, i64, i64)>(
        r#"
        SELECT
            provider_id,
            COALESCE(channel, '') as channel,
            model_id,
            COUNT(*) as cnt,
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(SUM(cache_read_tokens), 0),
            COALESCE(SUM(cache_write_tokens), 0)
        FROM usage_records
        WHERE created_at >= datetime('now', '-30 days')
        GROUP BY provider_id, COALESCE(channel, ''), model_id
        ORDER BY cnt DESC, provider_id, channel, model_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(provider_id, channel, model_id, request_count, total_input_tokens, total_output_tokens, cache_read_tokens, cache_write_tokens)| {
            let total_tokens = total_input_tokens + total_output_tokens;
            let cache_hit_rate = if total_input_tokens > 0 {
                (cache_read_tokens as f64 / total_input_tokens as f64 * 100.0 * 100.0).round() / 100.0
            } else {
                0.0
            };
            ModelStats {
                provider_id,
                channel,
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

#[derive(Serialize, sqlx::FromRow)]
pub struct ChannelUsageRow {
    pub date: String,
    pub channel: String,
    pub client: String,
    pub model_id: String,
    pub provider_id: String,
    pub request_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Serialize)]
pub struct ChannelUsage {
    pub dates: Vec<String>,
    pub rows: Vec<ChannelUsageRow>,
}

fn client_family(user_agent: &str) -> String {
    let product = user_agent.split('(').next().unwrap_or("").trim();
    let mut name = Vec::new();
    for token in product.split_whitespace() {
        let is_version = |value: &str| {
            value.trim_start_matches(['v', 'V']).starts_with(|ch: char| ch.is_ascii_digit())
        };
        if is_version(token) {
            break;
        }
        if let Some((client, version)) = token.rsplit_once('/') {
            if is_version(version) {
                name.push(client);
                break;
            }
        }
        name.push(token);
    }
    name.join(" ")
}

pub async fn get_channel_usage(pool: &SqlitePool) -> anyhow::Result<ChannelUsage> {
    // 同一条 SQL 固定窗口边界，跨 UTC 午夜查询时日期轴与用量仍保持一致。
    let rows = sqlx::query_as::<_, ChannelUsageRow>(
        r#"
        WITH RECURSIVE dates(d) AS (
            SELECT DATE('now', '-6 days')
            UNION ALL SELECT DATE(d, '+1 day') FROM dates WHERE d < DATE('now')
        )
        SELECT dates.d AS date, COALESCE(u.channel, '') AS channel,
               COALESCE(u.client, '') AS client, COALESCE(u.model_id, '') AS model_id,
               COALESCE(u.provider_id, '') AS provider_id,
               COUNT(u.id) AS request_count,
               COALESCE(SUM(u.input_tokens), 0) AS input_tokens,
               COALESCE(SUM(u.output_tokens), 0) AS output_tokens
        FROM dates
        LEFT JOIN usage_records u ON u.created_at >= dates.d
            AND u.created_at < datetime(dates.d, '+1 day')
        GROUP BY dates.d, COALESCE(u.channel, ''), COALESCE(u.client, ''), u.model_id, u.provider_id
        ORDER BY dates.d, channel, client, model_id, provider_id
        "#,
    ).fetch_all(pool).await?;
    let mut dates = Vec::new();
    for row in &rows {
        if dates.last() != Some(&row.date) {
            dates.push(row.date.clone());
        }
    }
    // 在聚合层归一客户端，让历史版本立即合并，同时保留日志的原始 User-Agent。
    let mut grouped = std::collections::BTreeMap::new();
    for mut row in rows.into_iter().filter(|row| row.request_count > 0) {
        row.client = client_family(&row.client);
        let key = (
            row.date.clone(), row.channel.clone(), row.client.clone(),
            row.model_id.clone(), row.provider_id.clone(),
        );
        let group = grouped.entry(key).or_insert_with(|| ChannelUsageRow {
            request_count: 0, input_tokens: 0, output_tokens: 0,
            date: row.date.clone(), channel: row.channel.clone(), client: row.client.clone(),
            model_id: row.model_id.clone(), provider_id: row.provider_id.clone(),
        });
        group.request_count += row.request_count;
        group.input_tokens += row.input_tokens;
        group.output_tokens += row.output_tokens;
    }
    Ok(ChannelUsage { dates, rows: grouped.into_values().collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn channel_usage_merges_client_versions_without_rewriting_logs() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        for (client, provider, model, channel, age, input, output) in [
            ("claude-cli/2.1.302 (external, cli)", "p1", "m1", "anthropic", "-1 days", 100, 10),
            ("claude-cli/2.1.303 (external, sdk-cli)", "p1", "m1", "anthropic", "-1 days", 200, 20),
            ("claude-cli/2.1.303", "p2", "m1", "anthropic", "-1 days", 300, 30),
            ("claude-cli/2.1.303", "p1", "m2", "anthropic", "-1 days", 400, 40),
            ("claude-cli/2.1.303", "p1", "m1", "openai_chat", "-1 days", 500, 50),
            ("claude-cli/2.1.303", "p1", "m1", "anthropic", "-2 days", 600, 60),
            ("OpenAI/JS 7.20.0", "p1", "m1", "openai_responses", "-1 days", 10, 1),
            ("OpenAI/JS 7.27.0", "p1", "m1", "openai_responses", "-1 days", 20, 2),
            ("deepseek-harness/0.2.0-rc.2 (+https://github.com/deepseek-ai/deepseek-harness)", "p1", "m1", "openai_responses", "-1 days", 30, 3),
            ("deepseek-harness/0.2.1-rc.2 (+https://github.com/deepseek-ai/deepseek-harness)", "p1", "m1", "openai_responses", "-1 days", 40, 4),
        ] {
            sqlx::query("INSERT INTO usage_records (client, provider_id, model_id, channel, created_at, input_tokens, output_tokens) VALUES (?, ?, ?, ?, datetime('now', ?), ?, ?)")
                .bind(client).bind(provider).bind(model).bind(channel).bind(age).bind(input).bind(output)
                .execute(&pool).await.unwrap();
        }
        let stats = get_channel_usage(&pool).await.unwrap();
        assert_eq!(stats.rows.len(), 7);
        let merged = stats.rows.iter().find(|r| r.client == "claude-cli" && r.provider_id == "p1" && r.model_id == "m1" && r.channel == "anthropic" && r.request_count == 2).unwrap();
        assert_eq!((merged.input_tokens, merged.output_tokens), (300, 30));
        let js = stats.rows.iter().find(|r| r.client == "OpenAI/JS").unwrap();
        assert_eq!((js.request_count, js.input_tokens, js.output_tokens), (2, 30, 3));
        let harness = stats.rows.iter().find(|r| r.client == "deepseek-harness").unwrap();
        assert_eq!((harness.request_count, harness.input_tokens, harness.output_tokens), (2, 70, 7));
        assert_eq!(stats.rows.iter().map(|r| r.request_count).sum::<i64>(), 10);
        let raw: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_records WHERE client = 'claude-cli/2.1.302 (external, cli)'").fetch_one(&pool).await.unwrap();
        assert_eq!(raw.0, 1);
    }

    #[tokio::test]
    async fn channel_usage_separates_dimensions_and_limits_to_seven_dates() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        for (client, model, provider, channel, input, output, age) in [
            (Some("cli"), "m1", "p1", Some("openai_chat"), 10, 2, "-6 days"),
            (Some("cli"), "m1", "p1", Some("openai_chat"), 20, 3, "-6 days"),
            (Some("other"), "m1", "p1", Some("openai_chat"), 30, 4, "-6 days"),
            (Some("cli"), "m2", "p1", Some("openai_chat"), 40, 5, "-6 days"),
            (Some("cli"), "m1", "p2", Some("openai_chat"), 50, 6, "-6 days"),
            (Some("cli"), "m1", "p1", Some("anthropic"), 60, 7, "-6 days"),
            (None, "m1", "p1", None, 70, 8, "-1 days"),
            (Some(""), "m1", "p1", None, 80, 9, "-1 days"),
            (Some("cli"), "m1", "p1", Some("openai_chat"), 999, 999, "-7 days"),
            (Some("cli"), "m1", "p1", Some("openai_chat"), 999, 999, "+1 days"),
        ] {
            sqlx::query("INSERT INTO usage_records (client, model_id, provider_id, channel, input_tokens, output_tokens, created_at) VALUES (?, ?, ?, ?, ?, ?, datetime('now', ?))")
                .bind(client).bind(model).bind(provider).bind(channel).bind(input).bind(output).bind(age)
                .execute(&pool).await.unwrap();
        }
        let stats = get_channel_usage(&pool).await.unwrap();
        assert_eq!(stats.dates.len(), 7);
        assert_eq!(stats.rows.len(), 6);
        let row = stats.rows.iter().find(|r| r.client == "cli" && r.model_id == "m1" && r.provider_id == "p1" && r.channel == "openai_chat").unwrap();
        assert_eq!(row.date, stats.dates[0]);
        assert_eq!((row.request_count, row.input_tokens, row.output_tokens), (2, 30, 5));
        let unknown = stats.rows.iter().find(|r| r.client.is_empty()).unwrap();
        assert_eq!(unknown.channel, "");
        assert_eq!((unknown.request_count, unknown.input_tokens, unknown.output_tokens), (2, 150, 17));
        sqlx::query("DELETE FROM usage_records").execute(&pool).await.unwrap();
        let empty = get_channel_usage(&pool).await.unwrap();
        assert_eq!(empty.dates.len(), 7);
        assert!(empty.rows.is_empty());
    }

    #[tokio::test]
    async fn model_stats_separate_provider_and_channel_in_same_window() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        for (provider, channel, input, output, cached, age) in [
            ("p1", Some("openai_chat"), 100, 10, 50, "-2 days"),
            ("p1", Some("openai_chat"), 20, 2, 10, "-1 days"),
            ("p1", Some("openai_responses"), 40, 4, 0, "-2 days"),
            ("p2", Some("openai_chat"), 60, 6, 0, "-2 days"),
            ("p1", None, 8, 1, 0, "-2 days"),
            ("p1", Some(""), 2, 1, 0, "-2 days"),
            ("p1", Some("openai_chat"), 9999, 9999, 0, "-40 days"),
        ] {
            sqlx::query("INSERT INTO usage_records (provider_id, channel, model_id, input_tokens, output_tokens, cache_read_tokens, created_at) VALUES (?, ?, 'same-model', ?, ?, ?, datetime('now', ?))")
                .bind(provider).bind(channel).bind(input).bind(output).bind(cached).bind(age)
                .execute(&pool).await.unwrap();
        }
        let rows = serde_json::to_value(get_model_stats(&pool).await.unwrap()).unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(
            rows.len(),
            4,
            "same-named models must be split by provider and channel"
        );
        let chat = rows
            .iter()
            .find(|r| r["provider_id"] == "p1" && r["channel"] == "openai_chat")
            .unwrap();
        assert_eq!(chat["request_count"], 2);
        assert_eq!(chat["total_input_tokens"], 120);
        assert_eq!(chat["total_output_tokens"], 12);
        assert_eq!(chat["total_tokens"], 132);
        assert_eq!(chat["cache_read_tokens"], 60);
        assert_eq!(chat["cache_hit_rate"], 50.0);
        let legacy = rows.iter().find(|r| r["channel"] == "").unwrap();
        assert_eq!(legacy["request_count"], 2);
        assert_eq!(legacy["total_tokens"], 12);
        let other = rows.iter().find(|r| r["provider_id"] == "p2").unwrap();
        assert_eq!(other["total_tokens"], 66);
    }

    #[tokio::test]
    async fn model_stats_empty_database_returns_empty_rows() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        crate::db::schema::run_migrations(&pool).await.unwrap();
        assert!(get_model_stats(&pool).await.unwrap().is_empty());
    }
}
