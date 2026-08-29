//! 智谱 bigmodel 余额 adapter：钱包 + Coding Plan 配额复合载荷。

use serde_json::{json, Value};

use super::{check_params, REQUEST_TIMEOUT};

const BIGMODEL_WALLET_URL: &str = "https://open.bigmodel.cn/api/biz/account/query-customer-account-report";
const BIGMODEL_QUOTA_URL: &str = "https://open.bigmodel.cn/api/monitor/usage/quota/limit";

/// 智谱：单快照复合载荷 `{wallet, plan}`。两个未公开端点均 `Authorization: Bearer <明文 key>`。
/// **鉴权失败上游返回 HTTP 200 + body `{code:1000|1001|401, msg}`**，故必须校验 body code，
/// 不能只看 status（http adapter 的「非 2xx 才失败」语义在这里会误判成功）。
/// 账号常只持有按量钱包或 Coding Plan 之一：任一子项成功即落 ok 快照（失败子项置 null），
/// 双失败才写 error 行。载荷形状是 adapter ↔ 前端 Providers.vue 的渲染契约。
pub(super) async fn bigmodel_usage(
    client: &reqwest::Client,
    api_key: &str,
    params: &serde_json::Map<String, Value>,
) -> anyhow::Result<Value> {
    check_params(params, &[])?;
    let (wallet, plan) = tokio::join!(
        bigmodel_wallet(client, BIGMODEL_WALLET_URL, api_key),
        bigmodel_plan(client, BIGMODEL_QUOTA_URL, api_key),
    );
    bigmodel_merge(wallet, plan)
}

/// 两路子探测的合并规则（纯函数，便于不触网络测四象限）。
fn bigmodel_merge(wallet: anyhow::Result<Value>, plan: anyhow::Result<Value>) -> anyhow::Result<Value> {
    match (wallet, plan) {
        (Ok(w), Ok(p)) => Ok(json!({ "wallet": w, "plan": p })),
        (Ok(w), Err(e)) => {
            tracing::debug!("bigmodel plan probe failed: {e}");
            Ok(json!({ "wallet": w, "plan": Value::Null }))
        }
        (Err(e), Ok(p)) => {
            tracing::debug!("bigmodel wallet probe failed: {e}");
            Ok(json!({ "wallet": Value::Null, "plan": p }))
        }
        (Err(we), Err(pe)) => anyhow::bail!("wallet: {}; plan: {}", we, pe),
    }
}

/// GET + Bearer，校验上游业务信封（HTTP 2xx 且 body `code == 200`），返回 `data` 节点。
async fn bigmodel_envelope(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> anyhow::Result<Value> {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {}", status);
    }
    let body: Value = resp.json().await.map_err(|_| anyhow::anyhow!("HTTP {} (non-JSON body)", status))?;
    let code = body.get("code").and_then(Value::as_i64);
    if code != Some(200) {
        let msg = body.get("msg").and_then(Value::as_str).unwrap_or("");
        anyhow::bail!("code {} {}", code.unwrap_or(-1), msg.trim());
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

/// 按量钱包余额。金额是 JSON number（可能科学计数法，如 `0E-9`）；成功但无钱包时 `data`
/// 可能是 `{}`（flowlet 实测）——字段缺失置 null，前端跳过该段。
async fn bigmodel_wallet(client: &reqwest::Client, url: &str, api_key: &str) -> anyhow::Result<Value> {
    let data = bigmodel_envelope(client, url, api_key).await?;
    let num = |k: &str| data.get(k).and_then(Value::as_f64);
    Ok(json!({
        "available_balance": num("availableBalance"),
        "recharge_amount": num("rechargeAmount"),
        "give_amount": num("giveAmount"),
        "total_spend_amount": num("totalSpendAmount"),
    }))
}

/// Coding Plan 配额：`limits[]` 只取 `TOKENS_LIMIT`（`TIME_LIMIT` 是 MCP 工具额度，不展示），
/// 按 `nextResetTime` 升序 → 5H / 7D（社区实测约定，statusline 同口径）；`percentage` = 已用%。
async fn bigmodel_plan(client: &reqwest::Client, url: &str, api_key: &str) -> anyhow::Result<Value> {
    let data = bigmodel_envelope(client, url, api_key).await?;
    let mut windows: Vec<(i64, &Value)> = data
        .get("limits")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|l| l.get("type").and_then(Value::as_str) == Some("TOKENS_LIMIT"))
                .map(|l| (l.get("nextResetTime").and_then(Value::as_i64).unwrap_or(0), l))
                .collect()
        })
        .unwrap_or_default();
    windows.sort_by_key(|(t, _)| *t);
    let plan: Vec<Value> = windows
        .iter()
        .zip(["5H", "7D"])
        .map(|((_, l), label)| {
            json!({
                "label": label,
                "used_pct": l.get("percentage").and_then(Value::as_f64),
                "resets_at": l.get("nextResetTime").and_then(Value::as_i64).map(|ms| ms / 1000),
            })
        })
        .collect();
    Ok(Value::Array(plan))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[tokio::test]
    async fn bigmodel_wallet_maps_camel_to_snake_with_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wallet"))
            .and(wiremock::matchers::header("Authorization", "Bearer id.secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200, "success": true, "msg": "操作成功",
                "data": { "availableBalance": 12.34, "rechargeAmount": 100.0, "giveAmount": 1.0, "totalSpendAmount": 88.66 }
            })))
            .expect(1)
            .mount(&server).await;
        let w = bigmodel_wallet(&client(), &format!("{}/wallet", server.uri()), "id.secret").await.unwrap();
        assert_eq!(w, json!({ "available_balance": 12.34, "recharge_amount": 100.0, "give_amount": 1.0, "total_spend_amount": 88.66 }));
    }

    #[tokio::test]
    async fn bigmodel_wallet_empty_data_yields_null_fields() {
        // 成功信封但 data={}（无钱包产品，flowlet 实测）→ 字段全 null，不算失败
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wallet"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 200, "success": true, "data": {} })))
            .mount(&server).await;
        let w = bigmodel_wallet(&client(), &format!("{}/wallet", server.uri()), "k").await.unwrap();
        assert_eq!(w, json!({ "available_balance": null, "recharge_amount": null, "give_amount": null, "total_spend_amount": null }));
    }

    #[tokio::test]
    async fn bigmodel_envelope_auth_fail_is_err_despite_http_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wallet"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 1000, "msg": "身份验证失败。", "success": false
            })))
            .mount(&server).await;
        let err = bigmodel_wallet(&client(), &format!("{}/wallet", server.uri()), "bad.key").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("code 1000"), "{msg}");
        assert!(msg.contains("身份验证失败"), "{msg}");
    }

    #[tokio::test]
    async fn bigmodel_envelope_non_2xx_is_err() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/wallet"))
            .respond_with(ResponseTemplate::new(500)).mount(&server).await;
        let err = bigmodel_wallet(&client(), &format!("{}/wallet", server.uri()), "k").await.unwrap_err();
        assert!(err.to_string().contains("HTTP 500"));
    }

    #[tokio::test]
    async fn bigmodel_plan_filters_tokens_limit_and_sorts_by_reset() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/quota"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 200, "success": true,
                "data": { "limits": [
                    { "type": "TIME_LIMIT", "percentage": 99, "nextResetTime": 1000 },
                    { "type": "TOKENS_LIMIT", "percentage": 88, "nextResetTime": 2000000 },
                    { "type": "TOKENS_LIMIT", "percentage": 42, "nextResetTime": 1000000 }
                ] }
            })))
            .mount(&server).await;
        let p = bigmodel_plan(&client(), &format!("{}/quota", server.uri()), "k").await.unwrap();
        // TIME_LIMIT 过滤；TOKENS_LIMIT 按 nextResetTime 升序 → 5H 在前；ms → 秒
        assert_eq!(p, json!([
            { "label": "5H", "used_pct": 42.0, "resets_at": 1000 },
            { "label": "7D", "used_pct": 88.0, "resets_at": 2000 }
        ]));
    }

    #[tokio::test]
    async fn bigmodel_plan_empty_limits_is_empty_array() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/quota"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "code": 200, "data": { "limits": [] } })))
            .mount(&server).await;
        let p = bigmodel_plan(&client(), &format!("{}/quota", server.uri()), "k").await.unwrap();
        assert_eq!(p, json!([]));
    }

    #[test]
    fn bigmodel_merge_quadrants() {
        // 双成功 → 两段齐全
        let v = bigmodel_merge(Ok(json!({"w": 1})), Ok(json!([{"p": 1}]))).unwrap();
        assert_eq!(v, json!({ "wallet": { "w": 1 }, "plan": [{"p": 1}] }));
        // 单成功 → 失败段 null，整体 ok
        let v = bigmodel_merge(Ok(json!({"w": 1})), Err(anyhow::anyhow!("boom"))).unwrap();
        assert_eq!(v, json!({ "wallet": { "w": 1 }, "plan": null }));
        let v = bigmodel_merge(Err(anyhow::anyhow!("boom")), Ok(json!([]))).unwrap();
        assert_eq!(v, json!({ "wallet": null, "plan": [] }));
        // 双失败 → Err，两边原因都在
        let e = bigmodel_merge(Err(anyhow::anyhow!("auth")), Err(anyhow::anyhow!("timeout"))).unwrap_err().to_string();
        assert_eq!(e, "wallet: auth; plan: timeout");
    }

    #[tokio::test]
    async fn bigmodel_rejects_params_without_network() {
        use serde_json::Map;
        use crate::config::UsageDef;
        let mut p = Map::new();
        p.insert("endpoint".into(), json!("https://evil.example.com"));
        let usage = UsageDef { adapter: "bigmodel".into(), params: p, result: None, display: None };
        let err = crate::admin::balance_svc::fetch_balance(&client(), &usage, "k").await.unwrap_err();
        assert!(err.to_string().contains("unknown usage param"));
    }
}
