//! 火山引擎（火山方舟）余额 adapter：账户现金余额 + Agent/Coding Plan 配额复合载荷。
//!
//! 三路探测全走火山引擎 OpenAPI、**强制 V4 签名（AK/SK）**——方舟推理 key（`ark-` 前缀
//! Bearer）调不了费用中心与控制面网关（实测 400 InvalidAuthorization），凭证从
//! `usage.params.{access_key, secret_key}` 取（火山引擎控制台 IAM 的 AK/SK，与推理 key
//! 是两套体系）。
//!
//! HTTP status 不可靠：网关对签名/凭据错误常返 4xx（多为 400）且带与 200 路径相同的
//! `ResponseMetadata.Error` 信封，业务错误也可能 200+Error——每路都解析信封（bigmodel
//! 同款教训，只看 status 的 http adapter 会误判成功）。

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::{check_params, REQUEST_TIMEOUT};

const CONTENT_TYPE: &str = "application/json; charset=utf-8";
const BILLING_ENDPOINT: &str = "https://billing.volcengineapi.com";
const OPENAPI_ENDPOINT: &str = "https://open.volcengineapi.com";

/// 一次 OpenAPI 调用的差异项（三路探测的公共形状都是 POST / + V4 签名）。
/// billing 服务签名区域用 cn-north-1（Go SDK 口径），ark 控制面用 cn-beijing——
/// 两个服务各自实测验证过的 scope，不是笔误。body：GetAFPUsage/GetCodingPlanUsage 实测
/// 回空、QueryBalanceAcct 回 `{}`（各自对照已验证的社区实现）。
struct VolcApi {
    service: &'static str,
    region: &'static str,
    query: &'static str,
    body: &'static [u8],
}

const BALANCE_API: VolcApi = VolcApi {
    service: "billing",
    region: "cn-north-1",
    query: "Action=QueryBalanceAcct&Version=2022-01-01",
    body: b"{}",
};
const AFP_API: VolcApi = VolcApi {
    service: "ark",
    region: "cn-beijing",
    query: "Action=GetAFPUsage&Region=cn-beijing&Version=2024-01-01",
    body: b"",
};
const CODING_PLAN_API: VolcApi = VolcApi {
    service: "ark",
    region: "cn-beijing",
    query: "Action=GetCodingPlanUsage&Region=cn-beijing&Version=2024-01-01",
    body: b"",
};

/// 入口：三路并发探测 + 合并。载荷契约（adapter ↔ 前端 Providers.vue）：
/// `{balance: {available, cash, frozen, arrears, credit_limit, currency} | null,
///   plan_source: "agent_plan" | "coding_plan" | null,
///   plan: [{label: 5H|7D|M, used_pct, resets_at}] | null}`
/// `plan` 直接是数组，与 bigmodel 的 plan 段同形——前端 planChips() 与 adapter 名解耦、零改动复用。
/// AK/SK 只在 params 里，不消费 provider 的推理 key（但 probe_one 仍要求推理 key 已配置——
/// 没有 key 的 provider 本就不参与路由）。
pub(super) async fn volcengine_usage(
    client: &reqwest::Client,
    _api_key: &str,
    params: &Map<String, Value>,
) -> anyhow::Result<Value> {
    check_params(params, &["access_key", "secret_key"])?;
    let param_str = |key: &str| -> anyhow::Result<String> {
        params
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "volcengine adapter 需在 usage.params 配置 access_key/secret_key \
                     （火山引擎 IAM AK/SK；方舟推理 key 调不了 billing/控制面网关）：missing '{key}'"
                )
            })
    };
    let access_key = param_str("access_key")?;
    let secret_key = param_str("secret_key")?;
    let (balance, afp, coding) = tokio::join!(
        query_balance_acct(client, BILLING_ENDPOINT, &access_key, &secret_key),
        get_afp_usage(client, OPENAPI_ENDPOINT, &access_key, &secret_key),
        get_coding_plan_usage(client, OPENAPI_ENDPOINT, &access_key, &secret_key),
    );
    volcengine_merge(balance, afp, coding)
}

/// 三路合并（纯函数，便于不触网络测象限）：任一成功即 ok（失败段置 null + debug log），
/// plan 段 AFP 窗口优先、空则 Coding Plan、再空则 null（未订阅不算失败），三路全 Err 才 Err。
fn volcengine_merge(
    balance: anyhow::Result<Value>,
    afp: anyhow::Result<Vec<Value>>,
    coding: anyhow::Result<Vec<Value>>,
) -> anyhow::Result<Value> {
    let plan = afp
        .as_ref()
        .ok()
        .filter(|w| !w.is_empty())
        .map(|w| ("agent_plan", w))
        .or_else(|| coding.as_ref().ok().filter(|w| !w.is_empty()).map(|w| ("coding_plan", w)));
    if let (Err(b), Err(a), Err(c)) = (&balance, &afp, &coding) {
        anyhow::bail!("balance: {b}; agent_plan: {a}; coding_plan: {c}");
    }
    if let Err(e) = &balance {
        tracing::debug!("volcengine balance probe failed: {e}");
    }
    if let Err(e) = &afp {
        tracing::debug!("volcengine agent-plan probe failed: {e}");
    }
    if let Err(e) = &coding {
        tracing::debug!("volcengine coding-plan probe failed: {e}");
    }
    Ok(json!({
        "balance": balance.unwrap_or(Value::Null),
        "plan_source": plan.map(|(s, _)| s),
        "plan": plan.map(|(_, w)| Value::Array(w.clone())),
    }))
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// 火山引擎 OpenAPI 签名 V4（HMAC-SHA256，AWS SigV4 的火山变体）。三处火山特有差异，
/// 照搬标准 SigV4 会签名失败：无 `AWS4` 前缀（algorithm 串 `HMAC-SHA256`、派生密钥首层
/// 直接 HMAC(SK, date)）、credential scope 结尾 `request`（非 `aws4_request`）。本 adapter
/// 三个请求都是 POST / + 固定四头，method/path 与 SignedHeaders（字母序，官方 Go SDK 口径）
/// 直接内联。`now` 参数化便于 golden vector 单测。返回 (Authorization, X-Date, X-Content-Sha256)。
fn volc_sign(
    api: &VolcApi,
    access_key: &str,
    secret_key: &str,
    host: &str,
    now: DateTime<Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = &x_date[..8];
    let body_hash = sha256_hex(api.body);
    let signed_headers = "content-type;host;x-content-sha256;x-date";
    let canonical_headers = format!(
        "content-type:{CONTENT_TYPE}\nhost:{host}\nx-content-sha256:{body_hash}\nx-date:{x_date}\n"
    );
    // canonical headers 块自带收尾 \n，其后显式 \n 形成 SigV4 规定的空行（对照 Go SDK）
    let canonical_request = format!("POST\n/\n{}\n{canonical_headers}\n{signed_headers}\n{body_hash}", api.query);
    let credential_scope = format!("{short_date}/{}/{}/request", api.region, api.service);
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );
    let k_date = hmac_sha256(secret_key.as_bytes(), short_date.as_bytes());
    let k_region = hmac_sha256(&k_date, api.region.as_bytes());
    let k_service = hmac_sha256(&k_region, api.service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"request");
    let signature = hmac_sha256(&k_signing, string_to_sign.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let authorization = format!(
        "HMAC-SHA256 Credential={access_key}/{credential_scope}, \
         SignedHeaders={signed_headers}, Signature={signature}"
    );
    (authorization, x_date, body_hash)
}

/// 火山 OpenAPI 调用：POST {endpoint}/?{query} + V4 签名，解析响应信封后返回 `Result` 节点。
/// endpoint 参数化（默认常量由入口传入）便于 wiremock 测试；host 从 endpoint 提取参与签名。
async fn openapi_call(
    client: &reqwest::Client,
    endpoint: &str,
    api: &VolcApi,
    access_key: &str,
    secret_key: &str,
) -> anyhow::Result<Value> {
    let host = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .unwrap_or(endpoint);
    let (authorization, x_date, x_content_sha256) =
        volc_sign(api, access_key, secret_key, host, Utc::now());
    let resp = client
        .post(format!("{endpoint}/?{}", api.query))
        .header("Authorization", authorization)
        .header("X-Date", x_date)
        .header("X-Content-Sha256", x_content_sha256)
        .header("Content-Type", CONTENT_TYPE)
        .body(api.body.to_vec())
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await?;
    let status = resp.status();
    let raw = resp.text().await?;
    let body: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => anyhow::bail!("HTTP {status}{}", if status.is_success() { " (non-JSON body)" } else { "" }),
    };
    // 信封优先于 status：网关凭据类错误常返 4xx（多为 400）且带与 200 路径相同的
    // ResponseMetadata.Error，业务错误也可能 200+Error——先报具体 code，无信封才回落 HTTP {status}
    if let Some((code, msg)) = envelope_error(&body) {
        anyhow::bail!("{code} {msg}");
    }
    if !status.is_success() {
        anyhow::bail!("HTTP {status}");
    }
    Ok(body.get("Result").cloned().unwrap_or(Value::Null))
}

/// 提取 OpenAPI 错误信封 `ResponseMetadata.Error.{Code, Message}`（兼容顶层 Error）。
fn envelope_error(body: &Value) -> Option<(String, String)> {
    let err = body
        .get("ResponseMetadata")
        .and_then(|m| m.get("Error"))
        .or_else(|| body.get("Error"))?;
    let code = err.get("Code").and_then(Value::as_str).unwrap_or("").trim();
    let msg = err.get("Message").and_then(Value::as_str).unwrap_or("").trim();
    if code.is_empty() && msg.is_empty() {
        return None;
    }
    Some((code.to_string(), msg.to_string()))
}

/// 费用中心账户现金余额（service=billing，scope 区域 cn-north-1，Go SDK 口径）。
async fn query_balance_acct(
    client: &reqwest::Client,
    endpoint: &str,
    access_key: &str,
    secret_key: &str,
) -> anyhow::Result<Value> {
    Ok(parse_balance(&openapi_call(client, endpoint, &BALANCE_API, access_key, secret_key).await?))
}

/// Agent Plan（GetAFPUsage，service=ark，scope 区域 cn-beijing）：绝对额度窗口。
async fn get_afp_usage(
    client: &reqwest::Client,
    endpoint: &str,
    access_key: &str,
    secret_key: &str,
) -> anyhow::Result<Vec<Value>> {
    Ok(parse_afp(&openapi_call(client, endpoint, &AFP_API, access_key, secret_key).await?))
}

/// Coding Plan（GetCodingPlanUsage，service=ark，scope 区域 cn-beijing）：百分比窗口。
async fn get_coding_plan_usage(
    client: &reqwest::Client,
    endpoint: &str,
    access_key: &str,
    secret_key: &str,
) -> anyhow::Result<Vec<Value>> {
    Ok(parse_coding_plan(&openapi_call(client, endpoint, &CODING_PLAN_API, access_key, secret_key).await?))
}

/// 数字或数字字符串（金额字段 ark-cli 记录为原样字符串）。
fn value_to_f64(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// 重置时间归一化到 epoch 秒：数字或数字字符串，>=1e12 判毫秒；<=0 视为无数据。
fn epoch_to_secs(v: &Value) -> Option<i64> {
    let n = value_to_f64(v)?;
    if n <= 0.0 {
        return None;
    }
    Some(if n >= 1e12 { (n / 1000.0) as i64 } else { n as i64 })
}

/// 余额段：字段缺失置 null（前端跳过该段），与 bigmodel wallet 段同策略。
fn parse_balance(result: &Value) -> Value {
    let amount = |k: &str| result.get(k).and_then(value_to_f64);
    json!({
        "available": amount("AvailableBalance"),
        "cash": amount("CashBalance"),
        "frozen": amount("FreezeAmount"),
        "arrears": amount("ArrearsBalance"),
        "credit_limit": amount("CreditLimit"),
        "currency": result.get("Currency").and_then(Value::as_str).unwrap_or("CNY"),
    })
}

/// Agent Plan 窗口：`Quota<=0` 视为该窗口未订阅/未启用（也用于识别「已鉴权但无
/// Agent Plan」，触发 Coding Plan 回落）；`AFPDaily` 被官方控制台隐藏（其 Quota 常高于
/// 周上限，历史默认值），跳过。已用% = Used/Quota×100，不裁剪范围。
fn parse_afp(result: &Value) -> Vec<Value> {
    [("AFPFiveHour", "5H"), ("AFPWeekly", "7D"), ("AFPMonthly", "M")]
        .into_iter()
        .filter_map(|(key, label)| {
            let win = result.get(key)?;
            let quota = win.get("Quota").and_then(value_to_f64)?;
            if quota <= 0.0 {
                return None;
            }
            let used = win.get("Used").and_then(value_to_f64).unwrap_or(0.0);
            Some(json!({
                "label": label,
                "used_pct": used / quota * 100.0,
                "resets_at": win.get("ResetTime").and_then(epoch_to_secs),
            }))
        })
        .collect()
}

/// Coding Plan 窗口：`Level`（实测为 session/weekly/monthly，大小写不敏感）映射到
/// 5H/7D/M 固定输出顺序（与 AFP 段一致）；`Percent` 即已用%；`QuotaUsage` 缺失 → 空（未订阅）。
fn parse_coding_plan(result: &Value) -> Vec<Value> {
    let Some(items) = result.get("QuotaUsage").and_then(Value::as_array) else {
        return Vec::new();
    };
    [("session", "5H"), ("weekly", "7D"), ("monthly", "M")]
        .into_iter()
        .filter_map(|(level, label)| {
            let item = items.iter().find(|i| {
                i.get("Level")
                    .and_then(Value::as_str)
                    .is_some_and(|l| l.eq_ignore_ascii_case(level))
            })?;
            Some(json!({
                "label": label,
                "used_pct": item.get("Percent").and_then(value_to_f64),
                "resets_at": item.get("ResetTime").and_then(epoch_to_secs),
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    fn win(label: &str, pct: f64) -> Value {
        json!({ "label": label, "used_pct": pct, "resets_at": null })
    }

    // ---- 签名 golden vector ----
    // 由逐行对照官方 Go SDK（volc-sdk-golang base/sign.go）的 Python 参考实现生成；
    // body hash 与 `{}` / 空串的公开 SHA-256 值吻合，双重交叉验证。
    #[test]
    fn volc_sign_matches_reference_vectors() {
        let now = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
        let (auth, x_date, hash) = volc_sign(
            &BALANCE_API, "AKLTtest-key-id", "test-secret", "billing.volcengineapi.com", now,
        );
        assert_eq!(x_date, "20260829T120000Z");
        assert_eq!(hash, "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
        assert_eq!(
            auth,
            "HMAC-SHA256 Credential=AKLTtest-key-id/20260829/cn-north-1/billing/request, \
             SignedHeaders=content-type;host;x-content-sha256;x-date, \
             Signature=599faf505e988a6ee3759bab1c8d2c6915fc95785a990b41960a0b2f1a5897eb"
        );
        let (auth, _, hash) = volc_sign(
            &AFP_API, "AKLTtest-key-id", "test-secret", "open.volcengineapi.com", now,
        );
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(
            auth,
            "HMAC-SHA256 Credential=AKLTtest-key-id/20260829/cn-beijing/ark/request, \
             SignedHeaders=content-type;host;x-content-sha256;x-date, \
             Signature=81ed9a19ff0fe609cbd4f3c8984aaca388ce3c0e2197b4b186b9e1231ae28f19"
        );
    }

    // ---- merge 象限 ----

    #[test]
    fn merge_prefers_afp_when_both_plans_return_windows() {
        let v = volcengine_merge(
            Ok(json!({"available": 1.0})),
            Ok(vec![win("5H", 10.0)]),
            Ok(vec![win("7D", 20.0)]),
        )
        .unwrap();
        assert_eq!(v["balance"]["available"], 1.0);
        assert_eq!(v["plan_source"], "agent_plan");
        assert_eq!(v["plan"][0]["label"], "5H");
    }

    #[test]
    fn merge_falls_back_to_coding_plan_when_afp_empty() {
        let v = volcengine_merge(
            Ok(json!({"available": 1.0})),
            Ok(vec![]),
            Ok(vec![win("7D", 20.0)]),
        )
        .unwrap();
        assert_eq!(v["plan_source"], "coding_plan");
        assert_eq!(v["plan"][0]["label"], "7D");
    }

    #[test]
    fn merge_yields_null_plan_when_unsubscribed() {
        let v = volcengine_merge(Ok(json!({"available": 1.0})), Ok(vec![]), Ok(vec![])).unwrap();
        assert_eq!(v["plan_source"], Value::Null);
        assert_eq!(v["plan"], Value::Null);
    }

    #[test]
    fn merge_balance_error_degrades_to_null_but_ok() {
        let v = volcengine_merge(
            Err(anyhow::anyhow!("HTTP 403")),
            Ok(vec![win("5H", 1.0)]),
            Ok(vec![]),
        )
        .unwrap();
        assert_eq!(v["balance"], Value::Null);
        assert_eq!(v["plan_source"], "agent_plan");
    }

    #[test]
    fn merge_single_success_even_empty_keeps_ok() {
        // afp 成功但未订阅（空窗口）+ 其余失败 → 仍 ok（任一子项成功即 ok，bigmodel 同语义）
        let v = volcengine_merge(Err(anyhow::anyhow!("x")), Ok(vec![]), Err(anyhow::anyhow!("y"))).unwrap();
        assert_eq!(v["balance"], Value::Null);
        assert_eq!(v["plan"], Value::Null);
    }

    #[test]
    fn merge_all_probes_failed_is_error() {
        let e = volcengine_merge(
            Err(anyhow::anyhow!("a")),
            Err(anyhow::anyhow!("b")),
            Err(anyhow::anyhow!("c")),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("a") && e.contains("b") && e.contains("c"), "{e}");
    }

    // ---- Result 解析（纯函数） ----

    #[test]
    fn parse_balance_maps_string_amounts_to_snake_case() {
        let result = json!({
            "AccountID": "21000",
            "AvailableBalance": "12.34", "CashBalance": "20.00",
            "FreezeAmount": "0.00", "ArrearsBalance": "0.00", "CreditLimit": "0.00",
            "Currency": "CNY"
        });
        let v = parse_balance(&result);
        assert_eq!(v, json!({
            "available": 12.34, "cash": 20.0, "frozen": 0.0, "arrears": 0.0,
            "credit_limit": 0.0, "currency": "CNY"
        }));
    }

    #[test]
    fn parse_afp_skips_zero_quota_and_computes_pct() {
        let result = json!({
            "AFPFiveHour": { "Quota": 100, "Used": 42, "ResetTime": 1770000000 },
            "AFPWeekly": { "Quota": 1000, "Used": 100, "ResetTime": 1770000000000i64 },
            "AFPMonthly": { "Quota": 0, "Used": 0, "ResetTime": 0 },
            "AFPDaily": { "Quota": 5000, "Used": 10 }
        });
        // Quota<=0 = 未订阅跳过；AFPDaily 被官方控制台隐藏（Quota 常高于周上限），跳过；
        // ResetTime 兼容秒/毫秒（>=1e12 判毫秒）
        assert_eq!(
            Value::Array(parse_afp(&result)),
            json!([
                { "label": "5H", "used_pct": 42.0, "resets_at": 1770000000 },
                { "label": "7D", "used_pct": 10.0, "resets_at": 1770000000 },
            ])
        );
    }

    #[test]
    fn parse_coding_plan_maps_levels_in_canonical_order() {
        let result = json!({ "PlanTier": "PLAN_TIER_LITE", "QuotaUsage": [
            { "Level": "weekly", "Percent": 30, "ResetTime": 1770000000 },
            { "Level": "session", "Percent": 42.5, "ResetTime": 1769990000 },
            { "Level": "monthly", "Percent": 5, "ResetTime": 0 }
        ]});
        // 输出按 5H/7D/M 固定顺序（与 AFP 段一致）；ResetTime 0 = 无数据 → null
        assert_eq!(
            Value::Array(parse_coding_plan(&result)),
            json!([
                { "label": "5H", "used_pct": 42.5, "resets_at": 1769990000 },
                { "label": "7D", "used_pct": 30.0, "resets_at": 1770000000 },
                { "label": "M", "used_pct": 5.0, "resets_at": null }
            ])
        );
    }

    // ---- 三路探测（wiremock） ----

    fn balance_ok_response() -> serde_json::Value {
        json!({
            "ResponseMetadata": { "RequestId": "r", "Action": "QueryBalanceAcct", "Version": "2022-01-01" },
            "Result": {
                "AccountID": "21000", "AvailableBalance": "12.34", "CashBalance": "20.00",
                "FreezeAmount": "0.00", "ArrearsBalance": "0.00", "CreditLimit": "0.00", "Currency": "CNY"
            }
        })
    }

    #[tokio::test]
    async fn query_balance_acct_sends_signed_post_and_parses_result() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(query_param("Action", "QueryBalanceAcct"))
            .and(query_param("Version", "2022-01-01"))
            .and(wiremock::matchers::header_regex("Authorization", r"^HMAC-SHA256 Credential=AKLT/"))
            .and(wiremock::matchers::header_regex("X-Date", r"^\d{8}T\d{6}Z$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(balance_ok_response()))
            .expect(1)
            .mount(&server)
            .await;
        let v = query_balance_acct(&client(), &server.uri(), "AKLT", "SK").await.unwrap();
        assert_eq!(v["available"], 12.34);
        assert_eq!(v["currency"], "CNY");
    }

    #[tokio::test]
    async fn openapi_envelope_error_on_200_is_err() {
        // 业务错误 200 + ResponseMetadata.Error：只看 status 会误判成功
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ResponseMetadata": { "Error": { "Code": "MissingAuthentication", "Message": "Signature does not match" } }
            })))
            .mount(&server)
            .await;
        let err = query_balance_acct(&client(), &server.uri(), "AKLT", "SK").await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("MissingAuthentication"), "{msg}");
        assert!(msg.contains("Signature"), "{msg}");
    }

    #[tokio::test]
    async fn openapi_envelope_error_on_400_beats_bare_status() {
        // 网关对签名/凭据错误常返 400（非 401）且带同一信封——错误信息须带 code 而非只报 HTTP 400
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "ResponseMetadata": { "Error": { "Code": "InvalidAuthorization", "Message": "bad credential" } }
            })))
            .mount(&server)
            .await;
        let err = query_balance_acct(&client(), &server.uri(), "AKLT", "SK").await.unwrap_err();
        assert!(err.to_string().contains("InvalidAuthorization"));
    }

    #[tokio::test]
    async fn openapi_non_2xx_without_envelope_is_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let err = query_balance_acct(&client(), &server.uri(), "AKLT", "SK").await.unwrap_err();
        assert!(err.to_string().contains("HTTP 502"));
    }

    #[tokio::test]
    async fn get_afp_usage_probes_and_parses_windows() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(query_param("Action", "GetAFPUsage"))
            .and(query_param("Region", "cn-beijing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ResponseMetadata": { "RequestId": "r" },
                "Result": {
                    "AFPFiveHour": { "Quota": 100, "Used": 42, "ResetTime": 1770000000 },
                    "AFPWeekly": { "Quota": 1000, "Used": 10, "ResetTime": 1770000000 },
                    "AFPMonthly": { "Quota": 5000, "Used": 250, "ResetTime": 1770000000 }
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let w = get_afp_usage(&client(), &server.uri(), "AKLT", "SK").await.unwrap();
        assert_eq!(w.len(), 3);
        assert_eq!(w[0]["label"], "5H");
        assert_eq!(w[2]["label"], "M");
    }

    #[tokio::test]
    async fn get_coding_plan_usage_probes_and_parses_windows() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/"))
            .and(query_param("Action", "GetCodingPlanUsage"))
            .and(query_param("Region", "cn-beijing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "ResponseMetadata": { "RequestId": "r" },
                "Result": { "PlanTier": "PLAN_TIER_LITE", "QuotaUsage": [
                    { "Level": "session", "Percent": 42, "ResetTime": 1770000000 },
                    { "Level": "weekly", "Percent": 10, "ResetTime": 1770000000 },
                    { "Level": "monthly", "Percent": 2, "ResetTime": 1770000000 }
                ] }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let w = get_coding_plan_usage(&client(), &server.uri(), "AKLT", "SK").await.unwrap();
        assert_eq!(w.len(), 3);
        assert_eq!(w[0]["label"], "5H");
        assert_eq!(w[1]["used_pct"], 10.0);
    }

    #[tokio::test]
    async fn volcengine_usage_requires_aksk_params() {
        let usage = crate::config::UsageDef {
            adapter: "volcengine".into(),
            params: Map::new(),
            result: None,
            display: None,
        };
        let err = crate::admin::balance_svc::fetch_balance(&client(), &usage, "ark-ee-key").await.unwrap_err();
        assert!(err.to_string().contains("access_key"), "{}", err);
    }
}
