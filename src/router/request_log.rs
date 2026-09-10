use axum::http::HeaderMap;
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::AsyncWriteExt;

// 与实际转发共用白名单；认证头由网关另行注入，绝不能写入诊断文件。
pub(super) const FORWARDED_HEADERS: &[&str] = &[
    "content-type",
    "anthropic-version",
    "anthropic-beta",
    "user-agent",
    "idempotency-key",
];

pub(super) async fn write_request(
    dir: &Path,
    metadata: Value,
    headers: &HeaderMap,
    body: &[u8],
    upstream_body: &[u8],
) -> anyhow::Result<String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let headers: serde_json::Map<String, Value> = FORWARDED_HEADERS
        .iter()
        .filter_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .map(|value| ((*name).to_string(), json!(value)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    #[tokio::test]
    async fn request_log_preserves_schema_and_excludes_auth_headers() {
        let dir = std::env::temp_dir().join(format!("mb-request-log-{}", uuid::Uuid::new_v4()));
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer client-secret".parse().unwrap());
        headers.insert("x-api-key", "provider-secret".parse().unwrap());
        headers.insert("cookie", "session=secret".parse().unwrap());
        headers.insert("anthropic-beta", "test-beta".parse().unwrap());
        headers.insert("user-agent", "claude-cli/2.1.267".parse().unwrap());
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
