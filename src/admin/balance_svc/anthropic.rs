//! Anthropic Cost API 返回已消费金额（美分），不是充值余额。

use chrono::{Datelike, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;

use super::{check_params, endpoint_param, REQUEST_TIMEOUT};

pub(super) async fn monthly_cost(
    client: &reqwest::Client,
    api_key: &str,
    params: &serde_json::Map<String, Value>,
) -> anyhow::Result<Value> {
    check_params(params, &["endpoint", "workspace_id"])?;
    let endpoint = endpoint_param(
        params,
        "https://api.anthropic.com/v1/organizations/cost_report",
    )?;
    let workspace_id = match params.get("workspace_id") {
        Some(value) => value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("workspace_id must be a string"))?,
        None => "",
    };
    let now = Utc::now();
    let starting_at = format!("{}-{:02}-01T00:00:00Z", now.year(), now.month());
    // 日桶用次日零点作上界，包含今天已上报的费用；快照另存实际查询时间。
    let ending_at = format!("{}T00:00:00Z", now.date_naive().succ_opt().unwrap());
    let mut page: Option<String> = None;
    let mut cursors = HashSet::new();
    let mut cents = 0.0_f64;
    loop {
        let mut req = client
            .get(&endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header(
                "user-agent",
                concat!("model-bridge/", env!("CARGO_PKG_VERSION")),
            )
            .query(&[
                ("starting_at", starting_at.as_str()),
                ("ending_at", ending_at.as_str()),
                ("bucket_width", "1d"),
                ("limit", "31"),
                ("group_by[]", "workspace_id"),
            ])
            .timeout(REQUEST_TIMEOUT);
        if let Some(page) = &page {
            req = req.query(&[("page", page)]);
        }
        let response = req.send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Cost API HTTP {}", response.status());
        }
        let body: Value = response.json().await?;
        let buckets = body["data"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("missing cost data array"))?;
        for bucket in buckets {
            let results = bucket["results"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("missing cost results"))?;
            for result in results {
                // Cost API 只支持 group_by，按 workspace 分组后在本地筛选，避免误报整个组织的费用。
                if !workspace_id.is_empty() {
                    if !result
                        .get("workspace_id")
                        .is_some_and(|id| id.is_string() || id.is_null())
                    {
                        anyhow::bail!("missing workspace_id in grouped cost result");
                    }
                    if result["workspace_id"].as_str() != Some(workspace_id) {
                        continue;
                    }
                }
                if result["currency"] != "USD" {
                    anyhow::bail!("unsupported cost currency");
                }
                let amount: f64 = result["amount"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing cost amount"))?
                    .parse()?;
                if !amount.is_finite() {
                    anyhow::bail!("invalid cost amount");
                }
                cents += amount;
            }
        }
        let has_more = body["has_more"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("missing cost has_more"))?;
        if !has_more {
            break;
        }
        let next = body["next_page"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing cost next_page"))?;
        if !cursors.insert(next.to_string()) {
            anyhow::bail!("cost pagination cursor did not advance");
        }
        page = Some(next.to_string());
    }
    if !cents.is_finite() {
        anyhow::bail!("invalid cost total");
    }
    Ok(json!({
        "kind": "cost", "cost_usd": cents / 100.0, "currency": "USD",
        "period": now.format("%Y-%m").to_string(), "starting_at": starting_at,
        "ending_at": ending_at, "scope": if workspace_id.is_empty() { "organization" } else { "workspace" },
        "workspace_id": workspace_id
    }))
}
