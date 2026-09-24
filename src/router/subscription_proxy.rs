use crate::state::{AppState, ProviderRoute};
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc, time::Instant};

const MAX_BUFFER: usize = 64 * 1024 * 1024;

fn prepare_request(body: &[u8], model: &str) -> anyhow::Result<(Value, bool)> {
    let mut value: Value = serde_json::from_slice(body)?;
    anyhow::ensure!(value.is_object(), "Responses request must be a JSON object");
    for field in ["store", "background"] {
        if let Some(v) = value.get(field) {
            anyhow::ensure!(v.is_boolean(), "{field} must be boolean");
            anyhow::ensure!(
                v != &json!(true),
                "subscription does not support {field}=true"
            );
        }
    }
    anyhow::ensure!(
        value.get("previous_response_id").is_none_or(Value::is_null),
        "subscription requires full input history; previous_response_id is unsupported"
    );
    let streaming = match value.get("stream") {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("stream must be boolean"))?,
    };
    if let Some(input) = value["input"].as_str() {
        value["input"] = json!([{"role":"user","content":[{"type":"input_text","text":input}]}]);
    }
    anyhow::ensure!(value["input"].is_array(), "input must be a string or array");
    match value.get("instructions") {
        None | Some(Value::Null) => value["instructions"] = json!("You are a helpful assistant."),
        Some(v) => {
            anyhow::ensure!(v.is_string(), "instructions must be a string");
            if v.as_str().is_some_and(|s| s.is_empty()) {
                value["instructions"] = json!("You are a helpful assistant.");
            }
        }
    }
    value["model"] = json!(model);
    value["store"] = json!(false);
    value["stream"] = json!(true);
    Ok((value, streaming))
}

#[derive(Default)]
struct ResponseEvents {
    buffer: Vec<u8>,
    terminal: Option<Value>,
    output_items: BTreeMap<u64, Value>,
    output_bytes: usize,
    failed: bool,
    response_terminal: bool,
    usage: (i64, i64, i64, i64),
}
impl ResponseEvents {
    fn push(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.buffer.extend_from_slice(bytes);
        anyhow::ensure!(
            self.buffer.len() <= MAX_BUFFER,
            "subscription SSE event exceeds size limit"
        );
        loop {
            let boundary = self
                .buffer
                .windows(2)
                .position(|w| w == b"\n\n")
                .map(|p| (p, 2))
                .into_iter()
                .chain(
                    self.buffer
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|p| (p, 4)),
                )
                .min_by_key(|v| v.0);
            let Some((pos, len)) = boundary else {
                break;
            };
            let frame: Vec<u8> = self.buffer.drain(..pos + len).collect();
            let text = std::str::from_utf8(&frame)?;
            let payload = text
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|v| v.strip_prefix(' ').unwrap_or(v))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let event: Value = serde_json::from_str(&payload)?;
            let kind = event["type"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing Responses event type"))?;
            if let Some(usage) = event.get("response").and_then(|v| v.get("usage")) {
                self.usage = super::proxy::extract_usage(usage, "openai");
            }
            match kind {
                "response.output_item.done" => {
                    let index = event["output_index"]
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("missing output item index"))?;
                    anyhow::ensure!(event["item"].is_object(), "invalid output item");
                    self.output_bytes += payload.len();
                    anyhow::ensure!(
                        self.output_bytes <= MAX_BUFFER,
                        "subscription output exceeds size limit"
                    );
                    self.output_items.insert(index, event["item"].clone());
                }
                "response.completed" | "response.failed" | "response.incomplete" => {
                    let expected = kind.strip_prefix("response.").unwrap();
                    anyhow::ensure!(
                        event["response"].is_object()
                            && event["response"]["status"].as_str() == Some(expected),
                        "invalid terminal response"
                    );
                    self.failed |= expected != "completed";
                    self.response_terminal = true;
                    let mut response = event["response"].clone();
                    // Codex can send the final output only in output_item.done, leaving
                    // the terminal output empty. Keep populated terminal output authoritative.
                    if response
                        .get("output")
                        .and_then(Value::as_array)
                        .is_none_or(Vec::is_empty)
                        && !self.output_items.is_empty()
                    {
                        response["output"] = Value::Array(
                            std::mem::take(&mut self.output_items)
                                .into_values()
                                .collect(),
                        );
                    }
                    self.terminal = Some(response);
                }
                "error" => {
                    self.failed = true;
                    self.terminal = Some(json!({"status":"failed","error":event["error"]}));
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn status(&self) -> &'static str {
        if self.terminal.is_some() && !self.failed {
            "success"
        } else {
            "error"
        }
    }
}

fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(json!({"error":{"message":message,"type":"subscription_error"}})),
    )
        .into_response()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn proxy(
    state: Arc<AppState>,
    route: ProviderRoute,
    method: Method,
    headers: HeaderMap,
    body: &[u8],
    api_key_id: Option<String>,
    client: Option<String>,
) -> Response {
    if method != Method::POST {
        return error(StatusCode::METHOD_NOT_ALLOWED, "Responses requires POST");
    }
    let (payload, streaming) = match prepare_request(body, &route.model_id) {
        Ok(v) => v,
        Err(e) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let started = Instant::now();
    let mut credentials = match state.subscription.credentials(&route.provider_id).await {
        Ok(v) => v,
        Err(e) => {
            let message = crate::crypto::credential_error_message(&e).unwrap_or_else(|| {
                "subscription credentials unavailable; check login status".into()
            });
            record(
                state,
                route,
                api_key_id,
                client,
                started,
                (0, 0, 0, 0),
                "error",
                Some(&message),
            )
            .await;
            return error(StatusCode::BAD_GATEWAY, &message);
        }
    };
    if *state.request_log_enabled.read().await && super::request_log::write_request(&state.request_log_dir,
            json!({"method":"POST","path":"responses","channel":"openai_responses","provider_id":route.provider_id,"route_model":route.model_id,"api_key_id":api_key_id}),
            &headers, body, &serde_json::to_vec(&payload).unwrap()).await.is_err() {
        tracing::warn!("failed to save subscription request diagnostic");
    }
    let original_account = credentials.account_id.clone();
    let url = format!("{}/responses", route.base_url.trim_end_matches('/'));
    let mut response = None;
    for attempt in 0..2 {
        if credentials.account_id != original_account {
            return error(
                StatusCode::CONFLICT,
                "subscription account changed during request; retry explicitly",
            );
        }
        // Login can replace the account while a periodic route snapshot still exists.
        // Check selected models against the same credential generation before sending.
        let eligible: Result<i64, _> = sqlx::query_scalar("SELECT COUNT(*) FROM provider_models m JOIN provider_config p USING(provider_id) JOIN provider_subscription_accounts a USING(provider_id) LEFT JOIN provider_channel_config c ON c.provider_id=m.provider_id AND c.channel_type=m.channel_type WHERE m.provider_id=? AND m.model_id=? AND m.channel_type='openai_responses' AND p.is_enabled=1 AND COALESCE(c.is_enabled,1)=1 AND a.credential_version=? AND a.auth_status='authorized'")
        .bind(&route.provider_id).bind(&route.model_id).bind(&credentials.credential_version).fetch_one(&state.db).await;
        match eligible {
            Ok(count) if count > 0 => {}
            Ok(_) => {
                return error(
                    StatusCode::NOT_FOUND,
                    "subscription model selection or account changed; refresh models",
                )
            }
            Err(_) => {
                return error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "failed to verify subscription model selection",
                )
            }
        }
        let mut outgoing = HeaderMap::new();
        for (name, value) in &headers {
            if super::request_log::is_forwardable(name.as_str())
                && !matches!(
                    name.as_str(),
                    "chatgpt-account-id"
                        | "originator"
                        | "openai-beta"
                        | "user-agent"
                        | "content-type"
                        | "accept"
                )
            {
                outgoing.insert(name.clone(), value.clone());
            }
        }
        let request = state
            .client
            .post(&url)
            .headers(outgoing)
            .bearer_auth(&credentials.access_token)
            .header("chatgpt-account-id", &credentials.account_id)
            .header("originator", "model-bridge")
            .header(
                "User-Agent",
                concat!("model-bridge/", env!("CARGO_PKG_VERSION")),
            )
            .header("OpenAI-Beta", "responses=experimental")
            .header("Accept", "text/event-stream")
            .timeout(std::time::Duration::from_secs(720))
            .json(&payload);
        match request.send().await {
            Ok(resp) if resp.status() == StatusCode::UNAUTHORIZED && attempt == 0 => {
                drop(resp);
                credentials = match state
                    .subscription
                    .force_refresh(&route.provider_id, &credentials.credential_version)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        let message =
                            crate::crypto::credential_error_message(&e).unwrap_or_else(|| {
                                "subscription authentication failed; sign in again".into()
                            });
                        record(
                            state,
                            route,
                            api_key_id,
                            client,
                            started,
                            (0, 0, 0, 0),
                            "error",
                            Some(&message),
                        )
                        .await;
                        return error(StatusCode::BAD_GATEWAY, &message);
                    }
                };
            }
            Ok(resp) => {
                response = Some(resp);
                break;
            }
            Err(_) => {
                record(
                    state,
                    route,
                    api_key_id,
                    client,
                    started,
                    (0, 0, 0, 0),
                    "error",
                    Some("subscription upstream connection failed"),
                )
                .await;
                return error(
                    StatusCode::BAD_GATEWAY,
                    "subscription upstream connection failed",
                );
            }
        }
    }
    let Some(resp) = response else {
        return error(
            StatusCode::BAD_GATEWAY,
            "subscription authentication failed",
        );
    };
    let status = resp.status();
    if !status.is_success() {
        let retry = resp.headers().get("retry-after").cloned();
        record(
            state,
            route,
            api_key_id,
            client,
            started,
            (0, 0, 0, 0),
            "error",
            Some(&format!("subscription upstream HTTP {status}")),
        )
        .await;
        let mut response = error(status, &format!("subscription upstream HTTP {status}"));
        if let Some(retry) = retry {
            response.headers_mut().insert("retry-after", retry);
        }
        return response;
    }
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.is_empty() && !content_type.starts_with("text/event-stream") {
        record(
            state,
            route,
            api_key_id,
            client,
            started,
            (0, 0, 0, 0),
            "error",
            Some("invalid subscription response content type"),
        )
        .await;
        return error(
            StatusCode::BAD_GATEWAY,
            "invalid subscription response content type",
        );
    }
    // Validate the first SSE field even when the upstream omitted Content-Type.
    // Preserve every byte for streaming clients once the prefix is established.
    let mut stream = resp.bytes_stream();
    let mut prefix = Vec::new();
    while !prefix.contains(&b'\n') {
        match stream.next().await {
            Some(Ok(bytes)) => {
                prefix.extend_from_slice(&bytes);
                if prefix.len() > MAX_BUFFER {
                    break;
                }
            }
            _ => break,
        }
    }
    let trim = prefix
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|p| &prefix[p..])
        .unwrap_or(&[]);
    if prefix.len() > MAX_BUFFER
        || !(trim.starts_with(b"event:") || trim.starts_with(b"data:") || trim.starts_with(b":"))
    {
        record(
            state,
            route,
            api_key_id,
            client,
            started,
            (0, 0, 0, 0),
            "error",
            Some("invalid subscription SSE response"),
        )
        .await;
        return error(StatusCode::BAD_GATEWAY, "invalid subscription SSE response");
    }
    let mut events = ResponseEvents::default();
    if events.push(&prefix).is_err() {
        record(
            state,
            route,
            api_key_id,
            client,
            started,
            events.usage,
            "error",
            Some("invalid subscription SSE event"),
        )
        .await;
        return error(StatusCode::BAD_GATEWAY, "invalid subscription SSE event");
    }
    if !streaming {
        let mut total = prefix.len();
        let mut stream_error = false;
        while events.terminal.is_none() {
            match stream.next().await {
                Some(Ok(bytes)) => {
                    total += bytes.len();
                    if total > MAX_BUFFER || events.push(&bytes).is_err() {
                        stream_error = true;
                        break;
                    }
                }
                Some(Err(_)) => {
                    stream_error = true;
                    break;
                }
                None => break,
            }
        }
        let success = !stream_error && events.status() == "success";
        record(
            state,
            route,
            api_key_id,
            client,
            started,
            events.usage,
            if success { "success" } else { "error" },
            if success {
                None
            } else {
                Some("subscription response did not complete")
            },
        )
        .await;
        if !stream_error && events.response_terminal {
            return Json(events.terminal.unwrap()).into_response();
        }
        return error(
            StatusCode::BAD_GATEWAY,
            "subscription response failed, was incomplete, or exceeded size limit",
        );
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(16);
    let task_state = state.clone();
    state.usage_tasks.spawn(async move {
        let mut failure = None;
        let mut cancelled = false;
        if tx.send(Ok(bytes::Bytes::from(prefix))).await.is_err() {
            cancelled = true;
        }
        while !cancelled && events.terminal.is_none() {
            let next = tokio::select! {
                biased;
                _=tx.closed()=> {cancelled=true; break;}
                chunk=stream.next()=>chunk,
            };
            match next {
                Some(Ok(bytes)) => {
                    if events.push(&bytes).is_err() {
                        failure = Some("invalid subscription SSE event");
                        break;
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        cancelled = true;
                        break;
                    }
                }
                Some(Err(_)) => {
                    failure = Some("subscription stream interrupted");
                    break;
                }
                None => break,
            }
        }
        if !cancelled && (failure.is_some() || events.terminal.is_none()) {
            failure = Some(failure.unwrap_or("subscription stream ended without a terminal event"));
            let _ = tx.send(Err(std::io::Error::other(failure.unwrap()))).await;
        }
        let status = if cancelled {
            "cancelled"
        } else if failure.is_some() {
            "error"
        } else {
            events.status()
        };
        if events.failed {
            failure = Some("subscription returned failed or incomplete response");
        }
        drop(stream);
        drop(tx);
        record(
            task_state,
            route,
            api_key_id,
            client,
            started,
            events.usage,
            status,
            failure,
        )
        .await;
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(Body::from_stream(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))
        .unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn record(
    state: Arc<AppState>,
    route: ProviderRoute,
    key: Option<String>,
    client: Option<String>,
    start: Instant,
    usage: (i64, i64, i64, i64),
    status: &str,
    error: Option<&str>,
) {
    super::proxy::write_usage(
        state,
        route.model_id,
        route.provider_id,
        usage.0,
        usage.1,
        usage.2,
        usage.3,
        start.elapsed().as_millis() as i64,
        status,
        error.map(str::to_owned),
        key,
        client,
        "openai".into(),
        "openai_responses".into(),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_request_keeps_responses_semantics() {
        let (body, streaming) = prepare_request(br#"{"model":"local/name","input":"hello","tools":[{"type":"function","name":"lookup"}]}"#, "real-model").unwrap();
        assert!(!streaming);
        assert_eq!(body["model"], "real-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["input"][0]["content"][0]["text"], "hello");
        assert_eq!(body["tools"][0]["name"], "lookup");
    }

    #[test]
    fn subscription_request_rejects_unsupported_stateful_features() {
        for body in [
            r#"{"input":[],"store":true}"#,
            r#"{"input":[],"background":true}"#,
            r#"{"input":[],"previous_response_id":"resp_old"}"#,
        ] {
            assert!(prepare_request(body.as_bytes(), "model").is_err());
        }
    }

    #[test]
    fn sse_parser_handles_split_events_and_requires_terminal() {
        let mut parser = ResponseEvents::default();
        parser
            .push(b"event: response.completed\r\ndata: {\"type\":\"response.com")
            .unwrap();
        assert!(parser.terminal.is_none());
        parser.push(b"pleted\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":12}}}\r\n\r\n").unwrap();
        assert_eq!(
            parser.terminal.as_ref().unwrap()["usage"]["input_tokens"],
            12
        );
        assert_eq!(parser.status(), "success");
        assert_eq!(ResponseEvents::default().status(), "error");
    }

    #[test]
    fn failed_response_is_not_success() {
        let mut parser = ResponseEvents::default();
        parser
            .push(b"data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\"}}\n\n")
            .unwrap();
        assert_eq!(parser.status(), "error");
    }
    #[test]
    fn empty_terminal_output_is_rebuilt_from_completed_items_in_index_order() {
        let message = json!({"id":"msg_test","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK"}]});
        let tool = json!({"id":"fc_test","type":"function_call","call_id":"call_test","name":"lookup","arguments":"{}","status":"completed"});
        for status in ["completed", "incomplete", "failed"] {
            let mut parser = ResponseEvents::default();
            for event in [
                json!({"type":"response.output_item.done","output_index":1,"item":tool}),
                json!({"type":"response.output_item.done","output_index":0,"item":message}),
                json!({"type":format!("response.{status}"),"response":{"status":status,"output":[],"usage":{"output_tokens":5}}}),
            ] {
                parser
                    .push(format!("data: {event}\n\n").as_bytes())
                    .unwrap();
            }
            let response = parser.terminal.unwrap();
            assert_eq!(response["output"], json!([message, tool]));
            assert_eq!(response["usage"]["output_tokens"], 5);
        }
    }

    #[test]
    fn populated_terminal_output_remains_authoritative() {
        let mut parser = ResponseEvents::default();
        for event in [
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"message","content":[]}}),
            json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"final"}]}]}}),
        ] {
            parser
                .push(format!("data: {event}\n\n").as_bytes())
                .unwrap();
        }
        assert_eq!(
            parser.terminal.unwrap()["output"][0]["content"][0]["text"],
            "final"
        );
    }

    #[test]
    fn malformed_terminal_response_is_rejected() {
        for payload in [
            r#"{"type":"response.incomplete"}"#,
            r#"{"type":"response.failed","response":null}"#,
            r#"{"type":"response.completed","response":{"status":"failed"}}"#,
        ] {
            let mut events = ResponseEvents::default();
            assert!(events
                .push(format!("data: {payload}\n\n").as_bytes())
                .is_err());
        }
    }
}
