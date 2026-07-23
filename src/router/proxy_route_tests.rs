// 路由级行为测试（finding #2）：覆盖三个入口端点的 HTTP 层行为。
// 目的是在纯函数单测之外，守住最容易线上回归、又最难靠纯函数覆盖的入口行为：
//   - 认证 401（无 key / 非法 key）
//   - 路径不支持 404（chat 端点不服务 /responses、responses 端点不服务 /chat/completions）
//   - 模型未找到 404（路由表里没有该 model）
//   - /v1/models 按端点隔离下发
//   - 413 超限（body 超 64MiB）
//   - 502 上游连接失败（base_url 指向已释放端口 → is_connect）
//   - 200 转发：openai_chat（model 大小写规范化 + stream_options 注入）、
//     openai_responses（model 规范化）、anthropic（SSE 透传 + 上游用 x-api-key 头）
//
// 504（上游超时）未覆盖：超时 720s 硬编码在 proxy.rs:277，等待不可行；
// 该分支为单行 `is_timeout()→504`，回归风险低于路由/鉴权/413，故此处略过。

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use bytes::Bytes;
use sha2::Digest;
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::db::schema::run_migrations;
use crate::router::create_proxy_router;
use crate::state::{AppState, ProviderRoute};

use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 客户端侧网关 key（auth_middleware 会对它做 SHA-256 后查内存缓存）。
const TEST_KEY: &str = "mb-test-key";
/// 注入到 ProviderRoute.api_key 的上游伪凭证；assert 上游收到的鉴权值即此。
const UPSTREAM_KEY: &str = "sk-upstream";

fn hex_sha256(s: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(s.as_bytes()))
}

/// 构造一个 ProviderRoute：model_id/model_name 同名，base_url 指向给定上游。
fn route(model_id: &str, base_url: &str) -> ProviderRoute {
    ProviderRoute {
        provider_id: "prov".into(),
        provider_name: "Prov".into(),
        model_id: model_id.into(),
        model_name: model_id.into(),
        base_url: base_url.into(),
        api_key: UPSTREAM_KEY.into(),
    }
}

/// 构建带内存 SQLite（已建表）+ 一个已缓存测试 key 的 AppState。
async fn build_state(
    chat: HashMap<String, ProviderRoute>,
    responses: HashMap<String, ProviderRoute>,
    anthropic: HashMap<String, ProviderRoute>,
) -> Arc<AppState> {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut cache = HashMap::new();
    cache.insert(hex_sha256(TEST_KEY), "key-1".to_string());
    Arc::new(AppState {
        openai_chat_routes: Arc::new(RwLock::new(chat)),
        openai_responses_routes: Arc::new(RwLock::new(responses)),
        anthropic_routes: Arc::new(RwLock::new(anthropic)),
        provider_defs: vec![],
        db: pool,
        client,
        api_key_cache: Arc::new(RwLock::new(cache)),
        encryption_key: None,
        proxy_base_url: "http://test".into(),
    })
}

/// 在 127.0.0.1 随机端口上 serve 代理路由，返回 base URL。
async fn spawn_proxy(state: Arc<AppState>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app: Router = create_proxy_router(state);
    tokio::spawn(async move {
        // server 生命周期随测试结束而终止；bind 已完成即开始接受连接（OS backlog）。
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    format!("http://{addr}")
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

/// 客户端侧鉴权头（Bearer mb-xxx；与上游头格式无关，三种端点通用）。
fn auth_headers() -> reqwest::header::HeaderMap {
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        "authorization",
        reqwest::header::HeaderValue::from_str(&format!("Bearer {TEST_KEY}")).unwrap(),
    );
    h
}

#[tokio::test(flavor = "multi_thread")]
async fn no_api_key_returns_401() {
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), HashMap::new()).await).await;
    let resp = http()
        .get(format!("{base}/openai-chat/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_api_key_returns_401() {
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), HashMap::new()).await).await;
    let resp = http()
        .get(format!("{base}/openai-chat/v1/models"))
        .header("authorization", "Bearer mb-wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_chat_path_returns_404() {
    // chat 端点只服务 /chat/completions：/responses 命中 chat 处理器但路径不匹配 → 404
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), HashMap::new()).await).await;
    let resp = http()
        .post(format!("{base}/openai-chat/v1/responses"))
        .headers(auth_headers())
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_responses_path_returns_404() {
    // responses 端点只服务 /responses：/chat/completions 命中 responses 处理器但路径不匹配 → 404
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), HashMap::new()).await).await;
    let resp = http()
        .post(format!("{base}/openai-responses/v1/chat/completions"))
        .headers(auth_headers())
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn model_not_in_route_table_returns_404() {
    // 有效 key、路径正确，但 model 不在路由表 → 模型未找到 404（命中处理器、不触达上游）
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), HashMap::new()).await).await;
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({"model":"gpt-4o","messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("gpt-4o"));
}

#[tokio::test(flavor = "multi_thread")]
async fn models_list_isolated_per_endpoint() {
    let mut chat = HashMap::new();
    chat.insert("gpt-4o".to_string(), route("GPT-4o", "http://unused"));
    let base = spawn_proxy(build_state(chat, HashMap::new(), HashMap::new()).await).await;

    // chat 端点下发自己的模型
    let r = http()
        .get(format!("{base}/openai-chat/v1/models"))
        .headers(auth_headers())
        .send()
        .await
        .unwrap();
    let b: serde_json::Value = r.json().await.unwrap();
    let ids: Vec<&str> = b["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["gpt-4o"]);

    // responses 端点路由表为空 → 不应看到 chat 的模型
    let r2 = http()
        .get(format!("{base}/openai-responses/v1/models"))
        .headers(auth_headers())
        .send()
        .await
        .unwrap();
    let b2: serde_json::Value = r2.json().await.unwrap();
    assert!(b2["data"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn oversized_body_returns_413() {
    // body 刚好超过 64MiB 上限：to_bytes 在上限处失败 → 413（发生在模型查找之前）。
    let big: Bytes = Bytes::from(vec![0u8; 64 * 1024 * 1024 + 1]);
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), HashMap::new()).await).await;
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .body(big)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 413);
}

#[tokio::test(flavor = "multi_thread")]
async fn upstream_connect_failure_returns_502() {
    // base_url 指向一个刚释放的端口 → reqwest 连接被拒 → is_connect → 502（立即返回，不等待 720s）
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    let mut chat = HashMap::new();
    chat.insert("gpt-4o".to_string(), route("GPT-4o", &dead_url));
    let base = spawn_proxy(build_state(chat, HashMap::new(), HashMap::new()).await).await;

    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({"model":"gpt-4o","stream":true,"messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 502);
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_chat_forwards_canonical_model_and_injects_stream_options() {
    let server = MockServer::start().await;
    // 上游收到的请求体应含：规范大小写的 model（client 发小写）+ 注入的 stream_options（client 未传）
    // body_partial_json 既验证又决定响应：匹配失败时 wiremock 默认 404，proxy 透传 404 → 断言失败。
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({
            "model": "GPT-4o",
            "stream_options": {"include_usage": true}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "ok"}}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1}
        })))
        .mount(&server)
        .await;

    let mut chat = HashMap::new();
    chat.insert("gpt-4o".to_string(), route("GPT-4o", &server.uri()));
    let base = spawn_proxy(build_state(chat, HashMap::new(), HashMap::new()).await).await;

    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["choices"][0]["message"]["content"].as_str(), Some("ok"));
}

#[tokio::test(flavor = "multi_thread")]
async fn openai_responses_forwards_canonical_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(body_partial_json(serde_json::json!({"model": "GPT-4o"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id":"resp_1","output":[]})),
        )
        .mount(&server)
        .await;

    let mut responses = HashMap::new();
    responses.insert("gpt-4o".to_string(), route("GPT-4o", &server.uri()));
    let base = spawn_proxy(build_state(HashMap::new(), responses, HashMap::new()).await).await;

    let resp = http()
        .post(format!("{base}/openai-responses/v1/responses"))
        .headers(auth_headers())
        .json(&serde_json::json!({"model":"gpt-4o","input":"hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str(), Some("resp_1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn anthropic_forwards_sse_and_uses_x_api_key_header() {
    let server = MockServer::start().await;
    let sse = "data: {\"type\":\"message_start\"}\n\n\
               data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n\
               data: [DONE]\n\n";
    // 上游应收到 x-api-key（anthropic 鉴权头格式）+ 规范 model（无 [1m] 后缀，无需剥离）
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(header("x-api-key", UPSTREAM_KEY))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(sse.as_bytes(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let mut anthropic = HashMap::new();
    anthropic.insert(
        "claude-sonnet-4".to_string(),
        route("claude-sonnet-4", &server.uri()),
    );
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), anthropic).await).await;

    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers()) // 客户端侧 Bearer；上游头由 proxy 改写为 x-api-key
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": "claude-sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("text/event-stream"));
    let body = resp.bytes().await.unwrap();
    let text = std::str::from_utf8(&body).unwrap();
    assert!(text.contains("message_start"));
    assert!(text.contains("message_delta"));
    assert!(text.contains("[DONE]"));
}
