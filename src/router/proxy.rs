use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use tokio_stream::StreamExt;
use std::sync::Arc;

use crate::{
    router::models_list,
    state::AppState,
};

/// OpenAI 格式入口: /openai/v1/{*path}
pub async fn openai_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle_request(state, "openai", method, path, headers, body).await
}

/// Anthropic 格式入口: /anthropic/v1/{*path}
pub async fn anthropic_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle_request(state, "anthropic", method, path, headers, body).await
}

async fn handle_request(
    state: Arc<AppState>,
    api_format: &str,
    method: Method,
    path: String,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // GET /v1/models → 返回内存中缓存的模型列表
    if method == Method::GET && path == "models" {
        return match api_format {
            "openai" => models_list::get_openai_models(state).await,
            _ => models_list::get_anthropic_models(state).await,
        };
    }

    // 提取 model，查路由，转发
    proxy_to_provider(state, api_format, method, &path, headers, &body).await
}

async fn proxy_to_provider(
    state: Arc<AppState>,
    api_format: &str,
    method: Method,
    path: &str,
    headers: HeaderMap,
    body: &[u8],
) -> Response {
    // 1. 从请求体提取 model 名
    let model = extract_model_from_body(body).unwrap_or_default();

    // 2. 查对应路由表
    let routes = match api_format {
        "openai" => state.openai_routes.read().await,
        _ => state.anthropic_routes.read().await,
    };

    let route = match routes.get(&model) {
        Some(r) => r.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                format!(
                    r#"{{"error":"model '{}' not found on /{} endpoint"}}"#,
                    model, api_format
                ),
            )
                .into_response();
        }
    };

    drop(routes);

    // 3. 构造目标 URL
    let target_url = format!("{}/v1/{}", route.base_url.trim_end_matches('/'), path);

    // 4. 构造转发请求
    let client = reqwest::Client::new();
    let mut req = client
        .request(method.clone(), &target_url)
        .header("Authorization", format!("Bearer {}", route.api_key));

    // 5. 透传 Content-Type
    if let Some(content_type) = headers.get("content-type") {
        if let Ok(ct) = content_type.to_str() {
            req = req.header("Content-Type", ct);
        }
    }

    // 6. 发送请求
    let start = std::time::Instant::now();
    match req.body(body.to_vec()).send().await {
        Ok(resp) => {
            let is_stream = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.contains("text/event-stream"))
                .unwrap_or(false);

            if is_stream {
                proxy_streaming_response(
                    state, resp, model, route.provider_id, start,
                )
                .await
            } else {
                proxy_buffered_response(
                    state, resp, model, route.provider_id, start,
                )
                .await
            }
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as i64;
            tokio::spawn(write_usage(
                state.clone(),
                model,
                route.provider_id,
                0,
                0,
                latency_ms,
                "error",
                Some(e.to_string()),
            ));

            (
                StatusCode::BAD_GATEWAY,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({"error": format!("upstream error: {}", e)}).to_string(),
            )
                .into_response()
        }
    }
}

/// 处理非流式响应：缓冲后返回
async fn proxy_buffered_response(
    state: Arc<AppState>,
    resp: reqwest::Response,
    model: String,
    provider_id: String,
    start: std::time::Instant,
) -> Response {
    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let latency_ms = start.elapsed().as_millis() as i64;

    match resp.bytes().await {
        Ok(body_bytes) => {
            // 异步写入 usage
            let usage = extract_usage_from_response(&body_bytes);
            tokio::spawn(write_usage(
                state.clone(),
                model,
                provider_id,
                usage.0,
                usage.1,
                latency_ms,
                if status.is_success() { "success" } else { "error" },
                None,
            ));

            // 构造响应
            let mut response = Response::builder().status(status);
            for (key, value) in resp_headers.iter() {
                if key != "transfer-encoding" && key != "content-encoding" {
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
                model,
                provider_id,
                0,
                0,
                latency_ms,
                "error",
                Some(e.to_string()),
            ));

            (
                StatusCode::BAD_GATEWAY,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                serde_json::json!({"error": format!("failed to read response: {}", e)}).to_string(),
            )
                .into_response()
        }
    }
}

/// 处理 SSE 流式响应：逐块透传
async fn proxy_streaming_response(
    state: Arc<AppState>,
    resp: reqwest::Response,
    model: String,
    provider_id: String,
    start: std::time::Instant,
) -> Response {
    let status = resp.status();
    let resp_headers = resp.headers().clone();

    // 构造流式 body
    let mut stream = resp.bytes_stream();
    let (tx, rx) = tokio::sync::mpsc::channel::<std::result::Result<bytes::Bytes, axum::Error>>(32);

    let state_final = state.clone();
    let model_final = model.clone();
    let provider_final = provider_id.clone();
    let start_clone = start;

    tokio::spawn(async move {
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if tx.send(Ok(bytes)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    let latency_ms = start_clone.elapsed().as_millis() as i64;
                    tokio::spawn(write_usage(
                        state_final.clone(),
                        model_final.clone(),
                        provider_final.clone(),
                        0,
                        0,
                        latency_ms,
                        "error",
                        Some(e.to_string()),
                    ));
                    break;
                }
            }
        }
        // 流结束时写入 usage
        let latency_ms = start_clone.elapsed().as_millis() as i64;
        tokio::spawn(write_usage(
            state_final,
            model_final,
            provider_final,
            0,
            0,
            latency_ms,
            "success",
            None,
        ));
    });

    let stream_body = Body::from_stream(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    );

    // 构造响应
    let mut response = Response::builder().status(status);
    for (key, value) in resp_headers.iter() {
        if key != "transfer-encoding" && key != "content-encoding" {
            response = response.header(key, value);
        }
    }
    response
        .body(stream_body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn extract_model_from_body(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("model")?.as_str().map(|s| s.to_string()))
}

fn extract_usage_from_response(body: &[u8]) -> (i64, i64) {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (0, 0),
    };

    if let Some(usage) = v.get("usage") {
        // OpenAI 格式: prompt_tokens, completion_tokens
        let o_input = usage.get("prompt_tokens").and_then(|v| v.as_i64());
        let o_output = usage.get("completion_tokens").and_then(|v| v.as_i64());
        if o_input.is_some() || o_output.is_some() {
            return (o_input.unwrap_or(0), o_output.unwrap_or(0));
        }
        // Anthropic 格式: input_tokens, output_tokens
        let a_input = usage.get("input_tokens").and_then(|v| v.as_i64());
        let a_output = usage.get("output_tokens").and_then(|v| v.as_i64());
        return (a_input.unwrap_or(0), a_output.unwrap_or(0));
    }

    (0, 0)
}

async fn write_usage(
    state: Arc<AppState>,
    model_id: String,
    provider_id: String,
    input_tokens: i64,
    output_tokens: i64,
    latency_ms: i64,
    status: &str,
    error_msg: Option<String>,
) {
    let _ = sqlx::query(
        "INSERT INTO usage_records (model_id, provider_id, input_tokens, output_tokens, latency_ms, status, error_msg) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&model_id)
    .bind(&provider_id)
    .bind(input_tokens)
    .bind(output_tokens)
    .bind(latency_ms)
    .bind(status)
    .bind(&error_msg)
    .execute(&state.db)
    .await;
}
