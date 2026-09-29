use axum::http::HeaderMap;
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::AsyncWriteExt;

// 转发（proxy.rs 第 5 步）与诊断日志共用的排除名单：客户端头默认全部外发，只剔除网关自管
// 或会破坏链路的头。用排除而非白名单是承重设计——Claude Code 每个版本会新增 capability 头
// 与请求体字段（自动模式的服务端检查即依赖 `safeguards` 字段与 `safeguard_results` 原样
// 透传），白名单会在版本升级时静默剥掉它们，把功能坏在上游无法察觉的地方。
pub(super) const NON_FORWARDED_HEADERS: &[&str] = &[
    // 认证头：网关按路由注入上游凭证；reqwest 的 .header() 是 append 而非 insert，
    // 透传会与上游凭证并存为两个同名头，并泄漏客户端的网关 key。
    "authorization",
    "x-api-key",
    // 由 route.workspace_id 注入，客户端再带一个同样会 append 成两个。
    "anthropic-workspace-id",
    // 请求体被改写过（model 名长度可变），客户端的长度会与实际 body 不符。
    "content-length",
    // reqwest 未启用 gzip/brotli feature：上游一旦压缩，SSE 行解析与 usage 提取会在压缩
    // 字节上静默失败（响应本身仍能原样透传给客户端，不报错，故尤其难排查）。
    "accept-encoding",
    // 客户端 cookie 是发给网关的，转给 LLM 上游既无用又泄漏凭据。
    "cookie",
    // Host 必须由 reqwest 按目标 URL 生成，透传会让上游收到网关的 host。
    "host",
    // 逐跳头：不该跨代理转发。
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// HeaderMap 的 key 在解析时已归一化为小写，故可直接按字面量比较。
pub(super) fn is_forwardable(name: &str) -> bool {
    !NON_FORWARDED_HEADERS.contains(&name)
}

pub(super) async fn write_request(
    dir: &Path,
    metadata: Value,
    headers: &HeaderMap,
    body: &[u8],
    upstream_body: &[u8],
) -> anyhow::Result<String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    // 记「实际转发的头」：与转发共用 is_forwardable，认证头与 cookie 因此不会落盘。
    let headers: serde_json::Map<String, Value> = headers
        .iter()
        .filter(|(name, _)| is_forwardable(name.as_str()))
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), json!(value)))
        })
        .collect();
    let parse_body = |bytes: &[u8]| -> Value {
        serde_json::from_slice(bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(bytes)))
    };
    let record = json!({
        "request_id": request_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "metadata": metadata,
        "headers": headers,
        "body": parse_body(body),
        "upstream_body": parse_body(upstream_body),
    });
    tokio::fs::create_dir_all(dir).await?;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(dir.join(format!("{request_id}.json"))).await?;
    file.write_all(&serde_json::to_vec_pretty(&record)?).await?;
    file.flush().await?;
    Ok(request_id)
}

// Error capture is bounded so a diagnostic cannot buffer an arbitrary upstream body
// or hold up the original error indefinitely. Only allowlisted response headers persist.
pub(super) async fn write_upstream_error(
    dir: &Path,
    request_id: &str,
    mut response: reqwest::Response,
    secrets: &[&str],
) -> anyhow::Result<()> {
    const LIMIT: usize = 64 * 1024;
    let status = response.status().as_u16();
    let redact = |text: &str| {
        let mut text = text.to_owned();
        for secret in secrets.iter().filter(|s| !s.is_empty()) {
            text = text.replace(secret, "[REDACTED]");
        }
        text
    };
    let headers: serde_json::Map<String, Value> = response
        .headers()
        .iter()
        .filter(|(name, _)| {
            matches!(
                name.as_str(),
                "content-type" | "x-request-id" | "request-id" | "retry-after" | "cf-ray"
            )
        })
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.to_string(), json!(redact(v))))
        })
        .collect();
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut read_error = None;
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    let remaining = LIMIT - bytes.len();
                    bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    if chunk.len() > remaining {
                        truncated = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    read_error = Some("upstream body read failed");
                    break;
                }
            }
        }
    })
    .await;
    if read.is_err() {
        read_error = Some("upstream body read timed out");
    }
    let raw = String::from_utf8_lossy(&bytes);
    let parsed = serde_json::from_str::<Value>(&raw);
    // Partial JSON bypasses field redaction and can split a known token. Never
    // persist incomplete bodies or malformed JSON-shaped bodies as raw text.
    let body_omitted = truncated
        || read_error.is_some()
        || (parsed.is_err() && matches!(raw.trim_start().chars().next(), Some('{') | Some('[')));
    let body = if body_omitted {
        "[OMITTED: body could not be safely redacted]".to_owned()
    } else if let Ok(mut value) = parsed {
        redact_credential_fields(&mut value);
        redact(&serde_json::to_string(&value)?)
    } else {
        redact(&raw)
    };
    let record = json!({
        "request_id": request_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "status": status,
        "headers": headers,
        "body": body,
        "body_omitted": body_omitted,
        "truncated": truncated,
        "read_error": read_error,
    });
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(dir.join(format!("{request_id}.response.json")))
        .await?;
    file.write_all(&serde_json::to_vec_pretty(&record)?).await?;
    file.flush().await?;
    Ok(())
}

fn redact_credential_fields(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                match name.to_ascii_lowercase().replace('-', "_").as_str() {
                    "authorization" | "api_key" | "x_api_key" | "access_token"
                    | "refresh_token" | "id_token" | "cookie" | "set_cookie" => {
                        *value = json!("[REDACTED]");
                    }
                    _ => redact_credential_fields(value),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_credential_fields),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    /// 排除名单的边界：网关自管（认证、长度、workspace）与逐跳头必须挡掉。
    #[test]
    fn is_forwardable_blocks_gateway_owned_and_hop_by_hop_headers() {
        for name in [
            "authorization",
            "x-api-key",
            "anthropic-workspace-id",
            "content-length",
            "accept-encoding",
            "cookie",
            "host",
            "connection",
            "keep-alive",
            "proxy-authenticate",
            "proxy-authorization",
            "proxy-connection",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
        ] {
            assert!(!is_forwardable(name), "{name} must not be forwarded");
        }
    }

    /// 排除名单之外一律放行：Claude Code 随版本新增的 capability 头必须能透传，
    /// 这正是本列表从白名单改成排除名单的原因。
    #[test]
    fn is_forwardable_keeps_unknown_client_headers() {
        for name in [
            "content-type",
            "anthropic-version",
            "anthropic-beta",
            "user-agent",
            "idempotency-key",
            "accept",
            "x-claude-code-session-id",
            "x-claude-code-agent-id",
            "x-some-future-capability",
        ] {
            assert!(is_forwardable(name), "{name} must be forwarded");
        }
    }

    #[tokio::test]
    async fn request_log_preserves_schema_and_excludes_auth_headers() {
        let dir = std::env::temp_dir().join(format!("mb-request-log-{}", uuid::Uuid::new_v4()));
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer client-secret".parse().unwrap());
        headers.insert("x-api-key", "provider-secret".parse().unwrap());
        headers.insert("cookie", "session=secret".parse().unwrap());
        headers.insert("anthropic-beta", "test-beta".parse().unwrap());
        headers.insert("user-agent", "claude-cli/2.1.267".parse().unwrap());
        headers.insert("x-claude-code-session-id", "sess-1".parse().unwrap());
        let original = json!({"model":"claude-deepseek[1M]", "tools":[{
            "name":"Artifact", "input_schema":{"pattern":r"^(?!__.*__$)[^\p{Cc}]{1,200}$"}
        }]});
        let mut upstream = original.clone();
        upstream["model"] = json!("deepseek");
        let id = write_request(
            &dir,
            json!({"provider":"test"}),
            &headers,
            &serde_json::to_vec(&original).unwrap(),
            &serde_json::to_vec(&upstream).unwrap(),
        )
        .await
        .unwrap();
        let path = dir.join(format!("{id}.json"));
        let raw = tokio::fs::read_to_string(&path).await.unwrap();
        let record: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(record["body"], original);
        assert_eq!(record["upstream_body"], upstream);
        assert_eq!(record["headers"]["anthropic-beta"], "test-beta");
        assert_eq!(record["headers"]["user-agent"], "claude-cli/2.1.267");
        // 诊断文件记的是「实际转发的头」：未列入排除名单的头要留下，认证头与 cookie 不能留
        assert_eq!(record["headers"]["x-claude-code-session-id"], "sess-1");
        assert!(!raw.contains("secret"));
        assert_eq!(record["request_id"], id);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn request_log_reports_unwritable_directory() {
        let path = std::env::temp_dir().join(format!("mb-request-log-{}", uuid::Uuid::new_v4()));
        tokio::fs::write(&path, b"not a directory").await.unwrap();
        assert!(
            write_request(&path, json!({}), &HeaderMap::new(), b"{}", b"{}")
                .await
                .is_err()
        );
        tokio::fs::remove_file(path).await.unwrap();
    }
}
