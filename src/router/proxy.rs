use axum::{
    body::Body,
    extract::{Path, Request, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use tokio_stream::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    middleware::auth::AuthenticatedKey,
    router::models_list,
    state::AppState,
};

/// 单个代理请求体上限（字节）。超出返回 413，避免超大请求耗尽内存。
const MAX_PROXY_BODY: usize = 64 * 1024 * 1024; // 64 MiB

/// OpenAI 格式入口: /openai/v1/{*path}
pub async fn openai_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    let api_key_id = request.extensions().get::<AuthenticatedKey>().map(|k| k.0.clone());
    let headers = request.headers().clone();
    let body_bytes = match axum::body::to_bytes(request.into_body(), MAX_PROXY_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"request body too large"}"#,
            )
                .into_response();
        }
    };
    handle_request(state, "openai", method, path, headers, body_bytes, api_key_id).await
}

/// Anthropic 格式入口: /anthropic/v1/{*path}
pub async fn anthropic_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    let api_key_id = request.extensions().get::<AuthenticatedKey>().map(|k| k.0.clone());
    let headers = request.headers().clone();
    let body_bytes = match axum::body::to_bytes(request.into_body(), MAX_PROXY_BODY).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                r#"{"error":"request body too large"}"#,
            )
                .into_response();
        }
    };
    handle_request(state, "anthropic", method, path, headers, body_bytes, api_key_id).await
}

async fn handle_request(
    state: Arc<AppState>,
    api_format: &str,
    method: Method,
    path: String,
    headers: HeaderMap,
    body: axum::body::Bytes,
    api_key_id: Option<String>,
) -> Response {
    // GET /v1/models → 返回内存中缓存的模型列表
    if method == Method::GET && path == "models" {
        return match api_format {
            "openai" => models_list::get_openai_models(state).await,
            _ => models_list::get_anthropic_models(state).await,
        };
    }

    // openai 入口仅识别 chat/completions、responses、models 三个子 path（共用同一张路由表）；其余记日志并 404。
    // required_channel：该 path 对应的 channel type，命中 route 后校验 route.channels 是否包含它，
    // 不含则视为无可用 provider 返回 404（即 provider 未声明该 channel 时不转发上游）。
    let (routes_lock, required_channel): (
        Arc<tokio::sync::RwLock<HashMap<String, crate::state::ProviderRoute>>>,
        Option<&str>,
    ) = match api_format {
        "openai" => match path.as_str() {
            "chat/completions" => (state.openai_routes.clone(), Some("openai_chat")),
            "responses" => (state.openai_routes.clone(), Some("openai_responses")),
            other => {
                tracing::warn!("unsupported openai path: /openai/v1/{}", other);
                return (
                    StatusCode::NOT_FOUND,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    serde_json::json!({ "error": format!("unsupported path '/openai/v1/{}'", other) }).to_string(),
                )
                    .into_response();
            }
        },
        _ => (state.anthropic_routes.clone(), None),
    };

    proxy_to_provider(
        state,
        api_format,
        method,
        &path,
        headers,
        &body,
        api_key_id,
        routes_lock,
        required_channel,
    )
    .await
}

async fn proxy_to_provider(
    state: Arc<AppState>,
    api_format: &str,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: &[u8],
    api_key_id: Option<String>,
    routes_lock: Arc<tokio::sync::RwLock<HashMap<String, crate::state::ProviderRoute>>>,
    required_channel: Option<&str>,
) -> Response {
    // 1. 从请求体提取 model 名（转小写用于路由查找）
    let model = extract_model_from_body(body).unwrap_or_default();
    let model_lower = model.to_lowercase();

    // 2. 查传入的路由表
    let routes = routes_lock.read().await;

    let route = match routes.get(&model_lower) {
        Some(r) => r.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({
                    "error": format!("model '{}' not found on /{} endpoint", model, api_format)
                })
                .to_string(),
            )
                .into_response();
        }
    };

    drop(routes);

    // 2.1 openai 请求按 path 校验 route 是否支持对应 channel：
    // provider 未声明该 channel 时直接 404，不转发上游。
    if let Some(ch) = required_channel {
        if !route.channels.iter().any(|c| c == ch) {
            tracing::info!(
                "model '{}' on /{} endpoint: provider '{}' has no '{}' channel, 404",
                model, api_format, route.provider_id, ch
            );
            return (
                StatusCode::NOT_FOUND,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({
                    "error": format!(
                        "model '{}' not available on /{} endpoint (provider '{}' has no '{}' channel)",
                        model, api_format, route.provider_id, ch
                    )
                })
                .to_string(),
            )
                .into_response();
        }
    }

    // 3. 构造目标 URL：base_url 自带完整版本前缀（如 .../v1 或 .../api/paas/v4），
    // 仅拼接 path，并去除两侧多余斜杠，避免出现 // 或重复 /v1。
    let target_url = format!(
        "{}/{}",
        route.base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );

    // 4. 构造转发请求
    let mut req = state
        .client
        .request(method.clone(), &target_url);

    // 根据 API 格式设置认证头
    match api_format {
        "anthropic" => {
            req = req.header("x-api-key", &route.api_key);
        }
        _ => {
            req = req.header("Authorization", format!("Bearer {}", route.api_key));
        }
    }

    // 5. 透传客户端关键请求头
    for header_name in &["content-type", "anthropic-version"] {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(v) = value.to_str() {
                req = req.header(*header_name, v);
            }
        }
    }

    // 6. 注入 stream_options（确保上游在流式响应中返回 usage）
    let body = replace_model_in_body(body, &route.model_id);
    let request_body = inject_stream_options(api_format, path, &body);

    // 7. 发送请求（带超时）
    let start = std::time::Instant::now();
    match req
        .timeout(std::time::Duration::from_secs(120))
        .body(request_body)
        .send()
        .await
    {
        Ok(resp) => {
            let is_stream = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.contains("text/event-stream"))
                .unwrap_or(false);

            if is_stream {
                proxy_streaming_response(
                    state, resp, &route.model_id, &route.provider_id, start, api_key_id, api_format,
                )
                .await
            } else {
                proxy_buffered_response(
                    state, resp, &route.model_id, &route.provider_id, start, api_key_id, api_format,
                )
                .await
            }
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as i64;
            tokio::spawn(write_usage(
                state.clone(),
                route.model_id.clone(),
                route.provider_id,
                0,
                0,
                0,
                0,
                latency_ms,
                "error",
                Some(e.to_string()),
                api_key_id,
            ));

            // 根据错误类型返回更具体的信息
            let (status, detail) = if e.is_timeout() {
                (StatusCode::GATEWAY_TIMEOUT, "upstream timeout")
            } else if e.is_connect() {
                (StatusCode::BAD_GATEWAY, "upstream connection failed")
            } else {
                (StatusCode::BAD_GATEWAY, "upstream error")
            };

            (
                status,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({"error": detail, "detail": e.to_string()}).to_string(),
            )
                .into_response()
        }
    }
}

/// 处理非流式响应：缓冲后返回
async fn proxy_buffered_response(
    state: Arc<AppState>,
    resp: reqwest::Response,
    model: &str,
    provider_id: &str,
    start: std::time::Instant,
    api_key_id: Option<String>,
    api_format: &str,
) -> Response {
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let latency_ms = start.elapsed().as_millis() as i64;

    match resp.bytes().await {
        Ok(body_bytes) => {
            // 异步写入 usage
            let usage = extract_usage_from_response(&body_bytes, api_format);
            tokio::spawn(write_usage(
                state.clone(),
                model.to_string(),
                provider_id.to_string(),
                usage.0,
                usage.1,
                usage.2,
                usage.3,
                latency_ms,
                if status.is_success() { "success" } else { "error" },
                None,
                api_key_id,
            ));

            // 构造响应，透传上游响应头（仅过滤 hop-by-hop 头）
            let mut response = Response::builder().status(status);
            for (key, value) in resp_headers.iter() {
                // 只过滤 transfer-encoding（axum 会自行处理），保留 content-encoding
                if key != "transfer-encoding" {
                    response = response.header(key, value);
                }
            }
            response
                .body(Body::from(body_bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            tokio::spawn(write_usage(
                state.clone(),
                model.to_string(),
                provider_id.to_string(),
                0,
                0,
                0,
                0,
                latency_ms,
                "error",
                Some(e.to_string()),
                api_key_id,
            ));

            (
                StatusCode::BAD_GATEWAY,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({"error": "failed to read response", "detail": e.to_string()}).to_string(),
            )
                .into_response()
        }
    }
}

/// 处理 SSE 流式响应：逐块透传，流结束时从最后 chunk 提取 usage
async fn proxy_streaming_response(
    state: Arc<AppState>,
    resp: reqwest::Response,
    model: &str,
    provider_id: &str,
    start: std::time::Instant,
    api_key_id: Option<String>,
    api_format: &str,
) -> Response {
    let status = resp.status();
    let resp_headers = resp.headers().clone();

    // 构造流式 body
    let mut stream = resp.bytes_stream();
    let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<bytes::Bytes, axum::Error>>(64);

    let state_final = state.clone();
    let model_final = model.to_string();
    let provider_final = provider_id.to_string();
    let api_key_id_final = api_key_id.clone();
    let api_format_final = api_format.to_string();
    let start_clone = start;

    tokio::spawn(async move {
        let mut has_error = false;
        // bytes_stream 的分块边界与 SSE 事件不对齐：一个事件可能横跨多个分块，
        // 一个分块也可能包含多个事件。因此按完整行（以 \n 切分）解析 data: 负载，
        // 不完整的行留在缓冲区等下一块，避免漏提或解析半截 JSON。
        let mut line_buf: Vec<u8> = Vec::new();
        // 累积所有 SSE 事件中的 usage（不能只取最后一个，Anthropic 的
        // input_tokens 在 message_start，output_tokens 在 message_delta）
        let mut acc_usage = (0i64, 0i64, 0i64, 0i64); // (input, output, cache_read, cache_write)
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    line_buf.extend_from_slice(&bytes);
                    while let Some(nl) = line_buf.iter().position(|&b| b == b'\n') {
                        // drain(..=nl) 移除该行内容连同结尾的 \n
                        let drained: Vec<u8> = line_buf.drain(..=nl).collect();
                        if let Some(payload) = parse_data_line(&drained) {
                            let u = extract_usage_from_sse_event(&payload, &api_format_final);
                            acc_usage.0 += u.0;
                            acc_usage.1 += u.1;
                            acc_usage.2 += u.2;
                            acc_usage.3 += u.3;
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    has_error = true;
                    let latency_ms = start_clone.elapsed().as_millis() as i64;
                    tokio::spawn(write_usage(
                        state_final.clone(),
                        model_final.clone(),
                        provider_final.clone(),
                        0,
                        0,
                        0,
                        0,
                        latency_ms,
                        "error",
                        Some(e.to_string()),
                        api_key_id_final.clone(),
                    ));
                    break;
                }
            }
        }
        // 流结束时写入累积的 usage
        if !has_error {
            let latency_ms = start_clone.elapsed().as_millis() as i64;
            tokio::spawn(write_usage(
                state_final,
                model_final,
                provider_final,
                acc_usage.0,
                acc_usage.1,
                acc_usage.2,
                acc_usage.3,
                latency_ms,
                "success",
                None,
                api_key_id_final,
            ));
        }
    });

    let stream_body = Body::from_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    );

    // 构造响应，透传上游响应头
    let mut response = Response::builder().status(status);
    for (key, value) in resp_headers.iter() {
        if key != "transfer-encoding" {
            response = response.header(key, value);
        }
    }
    response
        .body(stream_body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// 从一条完整 SSE 行（可能含尾部 \r/\n）提取 data 负载。
/// 支持 "data:" 与 "data: " 两种前缀；跳过 [DONE] 及非 data 行。返回 JSON 字节。
fn parse_data_line(line: &[u8]) -> Option<Vec<u8>> {
    // 去掉行尾的 \r / \n
    let mut end = line.len();
    while end > 0 && matches!(line[end - 1], b'\r' | b'\n') {
        end -= 1;
    }
    let rest = line[..end].strip_prefix(b"data:")?;
    let rest = rest.strip_prefix(b" ").unwrap_or(rest);
    if rest == b"[DONE]" {
        return None;
    }
    Some(rest.to_vec())
}

/// 从单个 SSE 事件 JSON 中提取 usage，支持：
/// - OpenAI SSE: {"usage": {"prompt_tokens":..., "completion_tokens":...}}
/// - Anthropic message_start: {"message": {"usage": {...}}}
/// - Anthropic message_delta: {"usage": {"output_tokens":...}}
///
/// 按 api_format 走对应的提取/归一化逻辑（见 extract_usage）。
fn extract_usage_from_sse_event(data: &[u8], api_format: &str) -> (i64, i64, i64, i64) {
    let v: serde_json::Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => return (0, 0, 0, 0),
    };

    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    // Anthropic message_start: usage 在 message.usage 下
    if event_type == "message_start" {
        if let Some(msg_usage) = v.get("message").and_then(|m| m.get("usage")) {
            return extract_usage(msg_usage, api_format);
        }
    }

    // 通用: usage 在顶层（OpenAI 末尾 chunk / Anthropic message_delta）
    if let Some(usage) = v.get("usage") {
        return extract_usage(usage, api_format);
    }

    (0, 0, 0, 0)
}

fn extract_model_from_body(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("model")?.as_str().map(|s| s.to_string()))
}

/// 将请求体中的 model 字段替换为路由表中存储的原始大小写
fn replace_model_in_body(body: &[u8], canonical_model: &str) -> Vec<u8> {
    if body.is_empty() {
        return body.to_vec();
    }
    let mut v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return body.to_vec(),
    };
    if let Some(obj) = v.as_object_mut() {
        obj.insert("model".to_string(), serde_json::Value::String(canonical_model.to_string()));
    }
    serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec())
}

/// 对于 OpenAI /v1/chat/completions 流式请求，注入 stream_options 确保上游返回完整 usage。
/// （Responses API 不使用 stream_options，故仅对 chat/completions 注入。）
fn inject_stream_options(api_format: &str, path: &str, body: &[u8]) -> Vec<u8> {
    if api_format == "anthropic" || body.is_empty() || path != "chat/completions" {
        return body.to_vec();
    }

    let mut v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return body.to_vec(),
    };

    // 只在 stream=true 时注入
    let is_stream = v.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    if !is_stream {
        return body.to_vec();
    }

    // 如果已有 stream_options 则不动
    if v.get("stream_options").is_some() {
        return body.to_vec();
    }

    v["stream_options"] = serde_json::json!({"include_usage": true});
    serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec())
}

/// 按请求所属 channel 协议提取并归一化 usage 为 (input, output, cache_read, cache_write)。
/// 协议由 api_format 决定（/openai/ 入口→openai 协议 channel，/anthropic/ 入口→anthropic 协议 channel，
/// 二者与 refresh_routes 建表时的 channel type 1:1 对应），不靠响应字段猜测格式。
/// - openai 协议: prompt_tokens / completion_tokens 已含缓存；cache_read 取
///   prompt_tokens_details.cached_tokens，回退顶层 cached_tokens / prompt_cache_hit_tokens。
/// - anthropic 协议: input_tokens 不含缓存，归一化为 input_tokens + cache_read + cache_write，
///   使 input 列跨协议一致（均含缓存），cache_hit_rate = cache_read / input 也因此有界。
fn extract_usage(usage: &serde_json::Value, api_format: &str) -> (i64, i64, i64, i64) {
    if api_format == "anthropic" {
        let input = usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let output = usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        let cache_write = usage.get("cache_creation_input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
        return (input + cache_read + cache_write, output, cache_read, cache_write);
    }
    // openai 协议
    let input = usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let output = usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
    let cache_read = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_i64())
        .or_else(|| usage.get("cached_tokens").and_then(|v| v.as_i64()))
        .or_else(|| usage.get("prompt_cache_hit_tokens").and_then(|v| v.as_i64()))
        .unwrap_or(0);
    (input, output, cache_read, 0)
}

fn extract_usage_from_response(body: &[u8], api_format: &str) -> (i64, i64, i64, i64) {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (0, 0, 0, 0),
    };

    if let Some(usage) = v.get("usage") {
        tracing::debug!("BUFFERED raw usage: {}", usage);
        return extract_usage(usage, api_format);
    }

    tracing::debug!("BUFFERED response without usage: {}", String::from_utf8_lossy(body).chars().take(300).collect::<String>());
    (0, 0, 0, 0)
}

#[allow(clippy::too_many_arguments)]
async fn write_usage(
    state: Arc<AppState>,
    model_id: String,
    provider_id: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    latency_ms: i64,
    status: &str,
    error_msg: Option<String>,
    api_key_id: Option<String>,
) {
    tracing::debug!(
        "WRITE usage: provider={}, model={}, input={}, output={}, cache_read={}, cache_write={}, latency_ms={}, status={}",
        provider_id, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, status
    );
    if let Err(e) = sqlx::query(
        "INSERT INTO usage_records (api_key_id, model_id, provider_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, status, error_msg) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&api_key_id)
    .bind(&model_id)
    .bind(&provider_id)
    .bind(input_tokens)
    .bind(output_tokens)
    .bind(cache_read_tokens)
    .bind(cache_write_tokens)
    .bind(latency_ms)
    .bind(status)
    .bind(&error_msg)
    .execute(&state.db)
    .await
    {
        tracing::error!("Failed to write usage record: {}", e);
    }
}