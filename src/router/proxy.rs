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

/// OpenAI Chat 入口: /openai-chat/v1/{*path}
pub async fn openai_chat_handler(
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
    handle_request(state, "openai_chat", method, path, headers, body_bytes, api_key_id).await
}

/// OpenAI Responses 入口: /openai-responses/v1/{*path}
pub async fn openai_responses_handler(
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
    handle_request(
        state,
        "openai_responses",
        method,
        path,
        headers,
        body_bytes,
        api_key_id,
    )
    .await
}

/// Anthropic 格式入口: /anthropic/v1/{*path}
pub async fn anthropic_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    let api_key_id = request.extensions().get::<AuthenticatedKey>().map(|k| k.0.clone());
    tracing::info!("Anthropic request: method={}, path={}, api_key_id={}", method, path, api_key_id.as_deref().unwrap_or("none"));
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
    channel: &str,
    method: Method,
    path: String,
    headers: HeaderMap,
    body: axum::body::Bytes,
    api_key_id: Option<String>,
) -> Response {
    // api_format（协议层）：openai_chat / openai_responses 均走 openai 协议，anthropic 走 anthropic 协议。
    let api_format: &str = if channel == "anthropic" {
        "anthropic"
    } else {
        "openai"
    };

    // GET /v1/models → 各端点返回各自路由表的模型列表
    if method == Method::GET && path == "models" {
        return match channel {
            "openai_chat" => models_list::get_openai_chat_models(state).await,
            "openai_responses" => models_list::get_openai_responses_models(state).await,
            _ => models_list::get_anthropic_models(state).await,
        };
    }

    // 各端点仅服务其所属接口的 path，其余记日志并 404；anthropic 入口透传任意 path。
    let routes_lock: Arc<tokio::sync::RwLock<HashMap<String, crate::state::ProviderRoute>>> =
        match channel {
            "openai_chat" => {
                if path != "chat/completions" {
                    tracing::warn!("unsupported openai-chat path: /openai-chat/v1/{}", path);
                    return (
                        StatusCode::NOT_FOUND,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        serde_json::json!({ "error": format!("unsupported path '/openai-chat/v1/{}'", path) })
                            .to_string(),
                    )
                        .into_response();
                }
                state.openai_chat_routes.clone()
            }
            "openai_responses" => {
                if path != "responses" {
                    tracing::warn!("unsupported openai-responses path: /openai-responses/v1/{}", path);
                    return (
                        StatusCode::NOT_FOUND,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        serde_json::json!({ "error": format!("unsupported path '/openai-responses/v1/{}'", path) })
                            .to_string(),
                    )
                        .into_response();
                }
                state.openai_responses_routes.clone()
            }
            _ => state.anthropic_routes.clone(),
        };

    let client = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    proxy_to_provider(
        state,
        api_format,
        channel,
        method,
        &path,
        headers,
        &body,
        api_key_id,
        client,
        routes_lock,
    )
    .await
}

async fn proxy_to_provider(
    state: Arc<AppState>,
    api_format: &str,
    channel: &str,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: &[u8],
    api_key_id: Option<String>,
    client: Option<String>,
    routes_lock: Arc<tokio::sync::RwLock<HashMap<String, crate::state::ProviderRoute>>>,
) -> Response {
    // channel（usage_records 的归属通道）由入口端点唯一决定：
    // /openai-chat→openai_chat、/openai-responses→openai_responses、/anthropic→anthropic，
    // 与建表时的 channel type 1:1 对应，比 api_format 更细。
    // 1. 从请求体提取 model 名（转小写用于路由查找）
    let model = extract_model_from_body(body).unwrap_or_default();
    let model_lower = model.to_lowercase();

    // 2. 查传入的路由表
    let routes = routes_lock.read().await;

    let route = match routes.get(&model_lower) {
        Some(r) => r.clone(),
        None => {
            let available_models: Vec<String> = routes.keys().cloned().collect();
            tracing::warn!(
                "Model not found: requested_model='{}' (lowercased='{}'), endpoint={}, available_models={:?}",
                model,
                model_lower,
                api_format,
                available_models
            );
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
    // anthropic 链路：路由表 key 是给客户端的检索名（已剥 [1m]，非 claude 模型还补了 claude- 前缀），
    // 不能直接发上游。统一用 route.model_id 剥掉 [1m]/[1M] 变体后缀后的原始名转发：
    // claude 原生模型无后缀（剥除为 no-op），非 claude 模型的 model_id 不含补的 claude- 前缀，
    // 剥后即真实上游名。openai 链路按既有逻辑用完整 route.model_id。
    let body = if api_format == "anthropic" {
        // [1m]/[1M] 为末 4 个 ASCII 字节，ends_with 已保证按字节切安全
        let upstream_model = if route.model_id.to_lowercase().ends_with("[1m]") {
            route.model_id[..route.model_id.len() - 4].to_string()
        } else {
            route.model_id.clone()
        };
        replace_model_in_body(body, &upstream_model)
    } else {
        replace_model_in_body(body, &route.model_id)
    };
    let request_body = inject_stream_options(api_format, path, &body);

    // 7. 发送请求（带超时）
    let start = std::time::Instant::now();
    match req
        .timeout(std::time::Duration::from_secs(480))
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
                    state, resp, &route.model_id, &route.provider_id, start, api_key_id, client, api_format, channel, &target_url,
                )
                .await
            } else {
                proxy_buffered_response(
                    state, resp, &route.model_id, &route.provider_id, start, api_key_id, client, api_format, channel, &target_url,
                )
                .await
            }
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as i64;
            let err_msg = error_chain(&e);
            tracing::warn!(
                "upstream send error: model={}, provider={}, url={}, latency_ms={}, err={}",
                route.model_id, route.provider_id, target_url, latency_ms, err_msg
            );
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
                Some(err_msg),
                api_key_id,
                client,
                api_format.to_string(),
                channel.to_string(),
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
#[allow(clippy::too_many_arguments)]
async fn proxy_buffered_response(
    state: Arc<AppState>,
    resp: reqwest::Response,
    model: &str,
    provider_id: &str,
    start: std::time::Instant,
    api_key_id: Option<String>,
    client: Option<String>,
    api_format: &str,
    channel: &str,
    target_url: &str,
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
                client,
                api_format.to_string(),
                channel.to_string(),
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
            let err_msg = error_chain(&e);
            tracing::warn!(
                "upstream body error: model={}, provider={}, url={}, latency_ms={}, err={}",
                model, provider_id, target_url, latency_ms, err_msg
            );
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
                Some(err_msg),
                api_key_id,
                client,
                api_format.to_string(),
                channel.to_string(),
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
#[allow(clippy::too_many_arguments)]
async fn proxy_streaming_response(
    state: Arc<AppState>,
    resp: reqwest::Response,
    model: &str,
    provider_id: &str,
    start: std::time::Instant,
    api_key_id: Option<String>,
    client: Option<String>,
    api_format: &str,
    channel: &str,
    target_url: &str,
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
    let client_final = client.clone();
    let api_format_final = api_format.to_string();
    let channel_final = channel.to_string();
    let target_url_final = target_url.to_string();
    let start_clone = start;

    tokio::spawn(async move {
        let mut has_error = false;
        // bytes_stream 的分块边界与 SSE 事件不对齐：一个事件可能横跨多个分块，
        // 一个分块也可能包含多个事件。因此按完整行（以 \n 切分）解析 data: 负载，
        // 不完整的行留在缓冲区等下一块，避免漏提或解析半截 JSON。
        let mut line_buf: Vec<u8> = Vec::new();
        // usage 各字段（input/output/cache_read/cache_write）在 SSE 事件中是累计终值而非
        // 增量，取「最后一个非零值」而非求和：message_delta 携带最终值、覆盖 message_start
        // 的初始/stub 值；标准 Anthropic 的 message_delta 无 input/cache 字段时则保留
        // message_start 的值。求和会让 glm-5.2 这类「start 与 delta 都带非平凡
        // input_tokens」的模型把 input 算两遍。
        let mut last_usage = (0i64, 0i64, 0i64, 0i64); // (input, output, cache_read, cache_write)
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    line_buf.extend_from_slice(&bytes);
                    while let Some(nl) = line_buf.iter().position(|&b| b == b'\n') {
                        // drain(..=nl) 移除该行内容连同结尾的 \n
                        let drained: Vec<u8> = line_buf.drain(..=nl).collect();
                        if let Some(payload) = parse_data_line(&drained) {
                            let u = extract_usage_from_sse_event(&payload, &api_format_final);
                            // 逐字段覆盖：仅当新值非零时更新（0 表示该事件未提供此字段）
                            if u.0 > 0 { last_usage.0 = u.0; }
                            if u.1 > 0 { last_usage.1 = u.1; }
                            if u.2 > 0 { last_usage.2 = u.2; }
                            if u.3 > 0 { last_usage.3 = u.3; }
                        }
                    }
                    if tx.send(Ok(bytes)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    has_error = true;
                    let latency_ms = start_clone.elapsed().as_millis() as i64;
                    let err_msg = error_chain(&e);
                    tracing::warn!(
                        "upstream stream error: model={}, provider={}, url={}, latency_ms={}, err={}",
                        model_final, provider_final, target_url_final, latency_ms, err_msg
                    );
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
                        Some(err_msg),
                        api_key_id_final.clone(),
                        client_final.clone(),
                        api_format_final.clone(),
                        channel_final.clone(),
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
                last_usage.0,
                last_usage.1,
                last_usage.2,
                last_usage.3,
                latency_ms,
                "success",
                None,
                api_key_id_final,
                client_final,
                api_format_final,
                channel_final,
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

    // OpenAI Responses API: usage 在 response.usage 下
    // (response.completed / response.in_progress 等事件)
    if let Some(resp_usage) = v.get("response").and_then(|r| r.get("usage")) {
        return extract_usage(resp_usage, api_format);
    }

    // 通用: usage 在顶层（OpenAI Chat 末尾 chunk / Anthropic message_delta）
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
/// 协议由 api_format 决定（/openai-chat/ 与 /openai-responses/ 入口→openai 协议 channel，/anthropic/ 入口→anthropic 协议 channel，
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
    // openai 协议: 兼容 Chat Completions (prompt_tokens/completion_tokens)
    // 和 Responses API (input_tokens/output_tokens) 两种字段名
    let input = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let output = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let cache_read = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .or_else(|| usage.get("input_tokens_details").and_then(|d| d.get("cached_tokens")))
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

/// 将 reqwest 错误的完整 source 链拼成一行，便于在日志/DB 里看到底层真因
/// （reqwest 的 Display 只输出 kind，如 "error decoding response body"，不含 source）。
fn error_chain(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut cur = std::error::Error::source(e);
    while let Some(src) = cur {
        msg.push_str(": ");
        msg.push_str(&src.to_string());
        cur = std::error::Error::source(src);
    }
    msg
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
    client: Option<String>,
    api_format: String,
    channel: String,
) {
    tracing::debug!(
        "WRITE usage: provider={}, model={}, input={}, output={}, cache_read={}, cache_write={}, latency_ms={}, status={}",
        provider_id, model_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, status
    );
    if let Err(e) = sqlx::query(
        "INSERT INTO usage_records (api_key_id, model_id, provider_id, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, latency_ms, status, error_msg, client, api_format, channel) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .bind(&client)
    .bind(&api_format)
    .bind(&channel)
    .execute(&state.db)
    .await
    {
        tracing::error!("Failed to write usage record: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== parse_data_line =====

    #[test]
    fn parse_data_line_standard() {
        let line = b"data: {\"key\":\"value\"}\n";
        assert_eq!(
            parse_data_line(line),
            Some(b"{\"key\":\"value\"}".to_vec())
        );
    }

    #[test]
    fn parse_data_line_no_space_after_colon() {
        let line = b"data:{\"x\":1}\n";
        assert_eq!(parse_data_line(line), Some(b"{\"x\":1}".to_vec()));
    }

    #[test]
    fn parse_data_line_done_marker() {
        assert_eq!(parse_data_line(b"data: [DONE]\n"), None);
        assert_eq!(parse_data_line(b"data:[DONE]\r\n"), None);
    }

    #[test]
    fn parse_data_line_non_data_line() {
        assert_eq!(parse_data_line(b"event: ping\n"), None);
        assert_eq!(parse_data_line(b"\n"), None);
    }

    #[test]
    fn parse_data_line_trailing_crlf() {
        let line = b"data: {\"a\":1}\r\n";
        assert_eq!(parse_data_line(line), Some(b"{\"a\":1}".to_vec()));
    }

    #[test]
    fn parse_data_line_empty_payload() {
        // 空 data 行不算错误，返回空 vec
        assert_eq!(parse_data_line(b"data:\n"), Some(vec![]));
    }

    // ===== extract_model_from_body =====

    #[test]
    fn extract_model_present() {
        let body = br#"{"model":"gpt-4","messages":[]}"#;
        assert_eq!(extract_model_from_body(body).as_deref(), Some("gpt-4"));
    }

    #[test]
    fn extract_model_preserves_case() {
        let body = br#"{"model":"Claude-3.5-Sonnet","stream":true}"#;
        assert_eq!(
            extract_model_from_body(body).as_deref(),
            Some("Claude-3.5-Sonnet")
        );
    }

    #[test]
    fn extract_model_missing() {
        let body = br#"{"messages":[]}"#;
        assert_eq!(extract_model_from_body(body), None);
    }

    #[test]
    fn extract_model_empty_body() {
        assert_eq!(extract_model_from_body(b""), None);
    }

    #[test]
    fn extract_model_invalid_json() {
        assert_eq!(extract_model_from_body(b"not json"), None);
    }

    // ===== replace_model_in_body =====

    #[test]
    fn replace_model_basic() {
        let body = br#"{"model":"claude-3","stream":true}"#;
        let result = replace_model_in_body(body, "claude-3-5-sonnet-latest");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["model"], "claude-3-5-sonnet-latest");
    }

    #[test]
    fn replace_model_empty_body() {
        let result = replace_model_in_body(b"", "gpt-4");
        assert_eq!(result, b"");
    }

    #[test]
    fn replace_model_invalid_json_passthrough() {
        let broken = b"not valid json";
        assert_eq!(replace_model_in_body(broken, "gpt-4"), broken);
    }

    #[test]
    fn replace_model_adds_field_if_missing() {
        let body = br#"{"stream":true}"#;
        let result = replace_model_in_body(body, "o1");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["model"], "o1");
    }

    #[test]
    fn replace_model_keeps_other_fields() {
        let body = br#"{"model":"old","messages":[{"role":"user","content":"hi"}],"temperature":0.7}"#;
        let result = replace_model_in_body(body, "new-model");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["model"], "new-model");
        assert_eq!(v["temperature"], 0.7);
        assert!(v["messages"].is_array());
    }

    // ===== inject_stream_options =====

    #[test]
    fn inject_stream_options_adds_when_missing() {
        let body = br#"{"model":"gpt-4","stream":true}"#;
        let result = inject_stream_options("openai", "chat/completions", body);
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(
            v["stream_options"],
            serde_json::json!({"include_usage": true})
        );
    }

    #[test]
    fn inject_stream_options_skips_when_not_streaming() {
        let body = br#"{"model":"gpt-4","stream":false}"#;
        let result = inject_stream_options("openai", "chat/completions", body);
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert!(v.get("stream_options").is_none());
    }

    #[test]
    fn inject_stream_options_skips_when_already_present() {
        let body = br#"{"model":"gpt-4","stream":true,"stream_options":{"include_usage":false}}"#;
        let result = inject_stream_options("openai", "chat/completions", body);
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["stream_options"]["include_usage"], false);
    }

    #[test]
    fn inject_stream_options_skips_anthropic() {
        let body = br#"{"model":"claude-3","stream":true}"#;
        let result = inject_stream_options("anthropic", "messages", body);
        assert_eq!(result, body);
    }

    #[test]
    fn inject_stream_options_skips_responses_path() {
        let body = br#"{"model":"gpt-4","stream":true}"#;
        let result = inject_stream_options("openai", "responses", body);
        assert_eq!(result, body);
    }

    #[test]
    fn inject_stream_options_empty_body() {
        let result = inject_stream_options("openai", "chat/completions", b"");
        assert_eq!(result, b"");
    }

    #[test]
    fn inject_stream_options_invalid_json() {
        let broken = b"not json";
        assert_eq!(
            inject_stream_options("openai", "chat/completions", broken),
            broken
        );
    }

    // ===== extract_usage =====

    #[test]
    fn extract_usage_openai_basic() {
        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "prompt_tokens_details": {"cached_tokens": 20}
        });
        let (input, output, cache_read, cache_write) = extract_usage(&usage, "openai");
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(cache_read, 20);
        assert_eq!(cache_write, 0);
    }

    #[test]
    fn extract_usage_openai_cached_tokens_fallback() {
        // 没有 prompt_tokens_details 时回退顶层 cached_tokens
        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "cached_tokens": 15
        });
        let (input, _output, cache_read, _cache_write) = extract_usage(&usage, "openai");
        assert_eq!(input, 100);
        assert_eq!(cache_read, 15);
    }

    #[test]
    fn extract_usage_openai_prompt_cache_hit_tokens_fallback() {
        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "prompt_cache_hit_tokens": 10
        });
        let (_input, _output, cache_read, _cache_write) = extract_usage(&usage, "openai");
        assert_eq!(cache_read, 10);
    }

    #[test]
    fn extract_usage_openai_details_takes_priority() {
        // prompt_tokens_details.cached_tokens 优先于顶层 fallback
        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "cached_tokens": 999,
            "prompt_tokens_details": {"cached_tokens": 30}
        });
        let (_input, _output, cache_read, _cache_write) = extract_usage(&usage, "openai");
        assert_eq!(cache_read, 30);
    }

    #[test]
    fn extract_usage_openai_all_zero() {
        let usage = serde_json::json!({});
        let (input, output, cache_read, cache_write) = extract_usage(&usage, "openai");
        assert_eq!((input, output, cache_read, cache_write), (0, 0, 0, 0));
    }

    #[test]
    fn extract_usage_anthropic_normalizes_input() {
        let usage = serde_json::json!({
            "input_tokens": 80,
            "output_tokens": 40,
            "cache_read_input_tokens": 20,
            "cache_creation_input_tokens": 10
        });
        let (input, output, cache_read, cache_write) = extract_usage(&usage, "anthropic");
        // input = 80 + 20 + 10 = 110（归一化为含缓存的总额）
        assert_eq!(input, 110);
        assert_eq!(output, 40);
        assert_eq!(cache_read, 20);
        assert_eq!(cache_write, 10);
    }

    #[test]
    fn extract_usage_anthropic_no_cache() {
        let usage = serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 50
        });
        let (input, output, cache_read, cache_write) = extract_usage(&usage, "anthropic");
        assert_eq!(input, 100);
        assert_eq!(output, 50);
        assert_eq!(cache_read, 0);
        assert_eq!(cache_write, 0);
    }

    // ===== extract_usage_from_sse_event =====

    #[test]
    fn sse_event_openai_final_chunk() {
        // OpenAI 流式末尾 chunk：usage 在顶层
        let data = br#"{"choices":[{"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":50}}"#;
        let (input, output, _, _) = extract_usage_from_sse_event(data, "openai");
        assert_eq!(input, 100);
        assert_eq!(output, 50);
    }

    #[test]
    fn sse_event_anthropic_message_start() {
        let data = br#"{"type":"message_start","message":{"usage":{"input_tokens":80,"output_tokens":0,"cache_read_input_tokens":20}}}"#;
        let (input, output, cache_read, _) = extract_usage_from_sse_event(data, "anthropic");
        // anthropic 归一化: 80 + 20 + 0 = 100
        assert_eq!(input, 100);
        assert_eq!(output, 0);
        assert_eq!(cache_read, 20);
    }

    #[test]
    fn sse_event_anthropic_message_delta() {
        // message_delta 的 usage 在顶层
        let data = br#"{"type":"message_delta","usage":{"output_tokens":42}}"#;
        let (input, output, _, _) = extract_usage_from_sse_event(data, "anthropic");
        assert_eq!(input, 0);
        assert_eq!(output, 42);
    }

    #[test]
    fn sse_event_no_usage() {
        let data = br#"{"type":"content_block_delta","delta":{"text":"hello"}}"#;
        let (input, output, cache_read, cache_write) =
            extract_usage_from_sse_event(data, "openai");
        assert_eq!((input, output, cache_read, cache_write), (0, 0, 0, 0));
    }

    #[test]
    fn sse_event_invalid_json() {
        let (input, output, cache_read, cache_write) =
            extract_usage_from_sse_event(b"not json", "openai");
        assert_eq!((input, output, cache_read, cache_write), (0, 0, 0, 0));
    }

    // ===== mask_key (from admin.rs) =====

    #[test]
    fn mask_key_standard() {
        let key = "mb-12345678-1234-1234-1234-1234567890ab";
        let masked = crate::router::admin::mask_key(key);
        assert!(masked.starts_with("mb-123"));
        assert!(masked.ends_with("90ab"));
        assert!(masked.contains("..."));
    }

    #[test]
    fn mask_key_short_passthrough() {
        assert_eq!(crate::router::admin::mask_key("mb-abcd"), "mb-abcd");
        assert_eq!(crate::router::admin::mask_key("abc"), "abc");
    }

    // ===== is_safe_base_url (from provider_svc.rs) =====

    #[test]
    fn is_safe_base_url_accepts_https() {
        assert!(crate::admin::provider_svc::is_safe_base_url(
            "https://api.openai.com/v1"
        ));
    }

    #[test]
    fn is_safe_base_url_accepts_http() {
        assert!(crate::admin::provider_svc::is_safe_base_url(
            "http://localhost:8080"
        ));
    }

    #[test]
    fn is_safe_base_url_rejects_file() {
        assert!(!crate::admin::provider_svc::is_safe_base_url(
            "file:///etc/passwd"
        ));
    }

    #[test]
    fn is_safe_base_url_rejects_garbage() {
        assert!(!crate::admin::provider_svc::is_safe_base_url("not a url"));
        assert!(!crate::admin::provider_svc::is_safe_base_url(""));
    }
}