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
//   - 流式非 2xx 落库为 error（上游 429 带 text/event-stream，须与非流式同口径）
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
use crate::config::{ChannelDef, ProviderDef};
use crate::admin::provider_svc::{refresh_routes, update_provider};

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

/// 构造带 provider_defs 的 AppState（供 refresh_routes 建表）。
async fn build_state_with_defs(defs: Vec<ProviderDef>) -> Arc<AppState> {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    run_migrations(&pool).await.unwrap();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut cache = HashMap::new();
    cache.insert(hex_sha256(TEST_KEY), "key-1".to_string());
    Arc::new(AppState {
        openai_chat_routes: Arc::new(RwLock::new(HashMap::new())),
        openai_responses_routes: Arc::new(RwLock::new(HashMap::new())),
        anthropic_routes: Arc::new(RwLock::new(HashMap::new())),
        provider_defs: defs,
        db: pool,
        client,
        api_key_cache: Arc::new(RwLock::new(cache)),
        encryption_key: None,
        proxy_base_url: "http://test".into(),
    })
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

/// 验证 anthropic 建表的冲突场景：多个 provider 声明同名模型时，只创建限定名 key
/// `claude-{provider}/{model}`，不创建裸名 key。客户端必须用限定名请求才能命中对应 provider，
/// 裸名请求返回 404。
#[tokio::test(flavor = "multi_thread")]
async fn anthropic_qualified_name_routes_to_correct_provider() {
    // 两个上游，各自只认识自己的 model
    let server_a = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_a)
        .await;
    let server_b = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_b)
        .await;

    // 两个 provider 声明同名模型 claude-sonnet-4，channel 均启用、base_url 各自指向自己的 mock
    let defs = vec![
        ProviderDef {
            id: "alpha".into(),
            name: "Alpha".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server_a.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
        ProviderDef {
            id: "beta".into(),
            name: "Beta".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server_b.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
    ];
    let state = build_state_with_defs(defs).await;
    // 写入两个 provider 的启用配置 + anthropic 模型
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-sonnet-4".into(), "Claude Sonnet 4".into())],
    )
    .await
    .unwrap();
    update_provider(
        &state.db,
        "beta",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-sonnet-4".into(), "Claude Sonnet 4".into())],
    )
    .await
    .unwrap();
    // 触发建表
    refresh_routes(&state).await.unwrap();

    // 路由表应含两条限定名 key、不含裸名 key（冲突模型只用限定名）
    {
        let routes = state.anthropic_routes.read().await;
        assert!(
            !routes.contains_key("claude-sonnet-4"),
            "bare key must NOT exist for conflicting model: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert!(
            routes.contains_key("claude-alpha/sonnet-4"),
            "qualified alpha key present: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert!(routes.contains_key("claude-beta/sonnet-4"), "qualified beta key present");
    }

    let base = spawn_proxy(state).await;

    // 模型列表的 display_name 应带 [{provider_id}] 前缀（仅展示用，区分同名模型来源）
    let r = http()
        .get(format!("{base}/anthropic/v1/models"))
        .headers(auth_headers())
        .send()
        .await
        .unwrap();
    let b: serde_json::Value = r.json().await.unwrap();
    let display_names: Vec<&str> = b["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["display_name"].as_str().unwrap())
        .collect();
    assert!(
        display_names.contains(&"[alpha]Claude Sonnet 4"),
        "display_name has [alpha] prefix: {:?}",
        display_names
    );
    assert!(
        display_names.contains(&"[beta]Claude Sonnet 4"),
        "display_name has [beta] prefix: {:?}",
        display_names
    );

    // 限定名请求 → 精确命中 beta（server_b）
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-beta/sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server_b.received_requests().await.unwrap().len(), 1);
    assert_eq!(server_a.received_requests().await.unwrap().len(), 0);

    // 限定名请求 → alpha（server_a）
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-alpha/sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server_a.received_requests().await.unwrap().len(), 1);
    assert_eq!(server_b.received_requests().await.unwrap().len(), 1);

    // 裸名请求 → 404（冲突模型无裸名 key）
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    assert_eq!(server_a.received_requests().await.unwrap().len(), 1);
    assert_eq!(server_b.received_requests().await.unwrap().len(), 1);
}

/// 验证冲突场景下限定名请求上游 body 中 model 被回写为干净 model_id（剥 [1M] 后缀、无 provider 前缀、保留 claude- 前缀）。
#[tokio::test(flavor = "multi_thread")]
async fn anthropic_qualified_name_upstream_body_is_clean_model_id() {
    let server = MockServer::start().await;
    // 上游应收到干净 model_id：claude-kimi-k3（剥 [1M] 后缀后的 model_id，保留 claude- 前缀，无 provider 前缀）
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-kimi-k3"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    // 两个 provider 声明相同的「自带 claude- 前缀 + [1M] 后缀」模型 → 冲突 → 只走限定名
    let defs = vec![
        ProviderDef {
            id: "kimi".into(),
            name: "Kimi".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
        ProviderDef {
            id: "kimi2".into(),
            name: "Kimi2".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
    ];
    let state = build_state_with_defs(defs).await;
    update_provider(
        &state.db,
        "kimi",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-kimi-k3[1M]".into(), "Kimi K3".into())],
    )
    .await
    .unwrap();
    update_provider(
        &state.db,
        "kimi2",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-kimi-k3[1M]".into(), "Kimi K3".into())],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();
    let base = spawn_proxy(state).await;

    // 限定名 key：claude-kimi/kimi-k3（claude- 前缀已从 model_id 剥除、[1M] 已剥除、最前补 claude-）
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-kimi/kimi-k3",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    // 上游收到干净 model_id：claude-kimi-k3（[1M] 后缀被剥除，保留 claude- 前缀，无 provider 前缀）
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// 验证非冲突场景：单 provider 单模型只建裸名 key（无限定名 key），display_name 无前缀。
#[tokio::test(flavor = "multi_thread")]
async fn anthropic_non_conflicting_uses_only_bare_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let defs = vec![ProviderDef {
        id: "alpha".into(),
        name: "Alpha".into(),
        icon: None,
        channels: vec![ChannelDef {
            channel_type: "anthropic".into(),
            base_url: server.uri(),
            models_endpoint: None,
        }],
        usage: None,
    }];
    let state = build_state_with_defs(defs).await;
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-sonnet-4".into(), "Claude Sonnet 4".into())],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();

    // 路由表只含裸名 key，不含限定名 key
    {
        let routes = state.anthropic_routes.read().await;
        assert!(routes.contains_key("claude-sonnet-4"), "bare key present: {:?}", routes.keys().collect::<Vec<_>>());
        assert!(
            !routes.contains_key("claude-alpha/sonnet-4"),
            "qualified key must NOT exist for non-conflicting model: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert_eq!(routes.len(), 1);
    }

    // display_name 无 [provider_id] 前缀
    let base = spawn_proxy(state).await;
    let r = http()
        .get(format!("{base}/anthropic/v1/models"))
        .headers(auth_headers())
        .send()
        .await
        .unwrap();
    let b: serde_json::Value = r.json().await.unwrap();
    let names: Vec<&str> = b["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["display_name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Claude Sonnet 4"), "display_name unprefixed: {:?}", names);

    // 裸名请求 200
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// 验证同 provider 声明 claude-sonnet-4 与 claude-sonnet-4[1M] 时，[1M] 变体优先保留：
/// 路由表只含一条限定名 key，其 model_id 为 [1M] 后缀变体；/v1/models 只暴露一个带 [1M] 后缀的 id；
/// 限定名请求转发上游时收到剥后缀的 claude-sonnet-4。无论 DB 返回行的顺序如何都应一致。
#[tokio::test(flavor = "multi_thread")]
async fn anthropic_same_provider_1m_variant_preferred() {
    let server = MockServer::start().await;
    // 上游应收到干净 model_id：claude-sonnet-4（[1M] 后缀被 proxy 剥除）
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let defs = vec![ProviderDef {
        id: "alpha".into(),
        name: "Alpha".into(),
        icon: None,
        channels: vec![ChannelDef {
            channel_type: "anthropic".into(),
            base_url: server.uri(),
            models_endpoint: None,
        }],
        usage: None,
    }];
    let state = build_state_with_defs(defs).await;
    // 同一 provider 声明两个归一化同名模型：claude-sonnet-4 与 claude-sonnet-4[1M]
    // 两者 bare 均为 claude-sonnet-4 → count==2 → 冲突 → 走限定名 key
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[
            ("anthropic".into(), "claude-sonnet-4".into(), "Claude Sonnet 4".into()),
            ("anthropic".into(), "claude-sonnet-4[1M]".into(), "Claude Sonnet 4".into()),
        ],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();

    // 路由表应只含一条限定名 key：claude-alpha/sonnet-4，且 model_id 为 [1M] 变体
    // （无论 DB 返回行的顺序，[1M] 变体都应胜出）
    {
        let routes = state.anthropic_routes.read().await;
        assert_eq!(
            routes.len(), 1,
            "only one qualified-name key: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        let route = routes.get("claude-alpha/sonnet-4").expect("qualified key present");
        assert!(
            route.model_id.to_lowercase().ends_with("[1m]"),
            "route.model_id should be the [1M] variant, got: {}",
            route.model_id
        );
    }

    let base = spawn_proxy(state).await;

    // /v1/models 只暴露一个 id，且以 [1M] 后缀结尾（让 Claude Code 开启 1M 上下文）
    let r = http()
        .get(format!("{base}/anthropic/v1/models"))
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
    assert_eq!(ids.len(), 1, "exactly one model listed: {:?}", ids);
    assert!(
        ids[0].to_lowercase().ends_with("[1m]"),
        "model id should end with [1M] suffix, got: {}",
        ids[0]
    );

    // 限定名请求 → 上游收到干净 claude-sonnet-4（[1M] 后缀被 proxy 剥除）
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-alpha/sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// 回归：限定名 key 嵌入 provider id 时必须转小写，否则 /v1/models 下发的 id 与
/// 代理查找侧 to_lowercase 后的 key 对不上，冲突模型彻底不可达（任何拼写都 404）。
#[tokio::test(flavor = "multi_thread")]
async fn anthropic_qualified_name_lowercases_provider_id_in_key() {
    let server_a = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_a)
        .await;
    let server_b = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_b)
        .await;

    // 大写 provider id（用户自定义 provider 合法形态）声明同名模型 → 冲突 → 走限定名 key
    let defs = vec![
        ProviderDef {
            id: "alpha".into(),
            name: "Alpha".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server_a.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
        ProviderDef {
            id: "MyClaude".into(),
            name: "MyClaude".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "anthropic".into(),
                base_url: server_b.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
    ];
    let state = build_state_with_defs(defs).await;
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-sonnet-4".into(), "Claude Sonnet 4".into())],
    )
    .await
    .unwrap();
    update_provider(
        &state.db,
        "MyClaude",
        UPSTREAM_KEY,
        true,
        &[("anthropic".into(), true)],
        &[("anthropic".into(), "claude-sonnet-4".into(), "Claude Sonnet 4".into())],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();

    // 限定名 key 必须是小写 provider id（代理查找侧总是 to_lowercase）
    {
        let routes = state.anthropic_routes.read().await;
        assert!(
            routes.contains_key("claude-myclaude/sonnet-4"),
            "lowercased qualified key present: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert!(
            !routes.contains_key("claude-MyClaude/sonnet-4"),
            "uppercase-id key must not exist: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
    }

    let base = spawn_proxy(state).await;

    // 按 /v1/models 下发的 id 原样回发 → 必须命中（回归前 404）
    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "claude-myclaude/sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server_b.received_requests().await.unwrap().len(), 1);
    assert_eq!(server_a.received_requests().await.unwrap().len(), 0);
}

/// openai_chat 跨 provider 同名冲突模型用限定名 key：路由表只含 `{provider}/{model}` 两个限定名 key、
/// 不含裸名；/v1/models 下发限定名 id；限定名请求精确命中；裸名请求 404。
#[tokio::test(flavor = "multi_thread")]
async fn openai_chat_conflicting_models_use_qualified_key() {
    let server_a = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model": "gpt-4o"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_a)
        .await;
    let server_b = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model": "gpt-4o"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_b)
        .await;

    // 两个 provider 在 chat 通道声明同名 gpt-4o → 冲突 → 走限定名 key
    let defs = vec![
        ProviderDef {
            id: "alpha".into(),
            name: "Alpha".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "openai_chat".into(),
                base_url: server_a.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
        ProviderDef {
            id: "beta".into(),
            name: "Beta".into(),
            icon: None,
            channels: vec![ChannelDef {
                channel_type: "openai_chat".into(),
                base_url: server_b.uri(),
                models_endpoint: None,
            }],
            usage: None,
        },
    ];
    let state = build_state_with_defs(defs).await;
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("openai_chat".into(), true)],
        &[("openai_chat".into(), "gpt-4o".into(), "GPT-4o".into())],
    )
    .await
    .unwrap();
    update_provider(
        &state.db,
        "beta",
        UPSTREAM_KEY,
        true,
        &[("openai_chat".into(), true)],
        &[("openai_chat".into(), "gpt-4o".into(), "GPT-4o".into())],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();

    // 路由表应含两条限定名 key、不含裸名 key（冲突模型只用限定名）
    {
        let routes = state.openai_chat_routes.read().await;
        assert!(
            !routes.contains_key("gpt-4o"),
            "bare key must NOT exist for conflicting model: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert!(
            routes.contains_key("alpha/gpt-4o"),
            "qualified alpha key present: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert!(routes.contains_key("beta/gpt-4o"), "qualified beta key present");
    }

    let base = spawn_proxy(state).await;

    // /v1/models 下发两个限定名 id
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
    assert!(ids.contains(&"alpha/gpt-4o"), "models list has alpha/gpt-4o: {:?}", ids);
    assert!(ids.contains(&"beta/gpt-4o"), "models list has beta/gpt-4o: {:?}", ids);
    assert!(!ids.contains(&"gpt-4o"), "bare gpt-4o not listed: {:?}", ids);

    // 限定名请求 alpha → 命中 server_a
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "alpha/gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server_a.received_requests().await.unwrap().len(), 1);
    assert_eq!(server_b.received_requests().await.unwrap().len(), 0);

    // 裸名请求 → 404（冲突模型无裸名 key）
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    assert_eq!(server_a.received_requests().await.unwrap().len(), 1);
}

/// openai_chat 非冲突模型只用裸名 key：路由表只含裸名、不含限定名；/v1/models 下发裸名 id；
/// 裸名请求 200。
#[tokio::test(flavor = "multi_thread")]
async fn openai_chat_non_conflicting_uses_only_bare_key() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model": "gpt-4o"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let defs = vec![ProviderDef {
        id: "alpha".into(),
        name: "Alpha".into(),
        icon: None,
        channels: vec![ChannelDef {
            channel_type: "openai_chat".into(),
            base_url: server.uri(),
            models_endpoint: None,
        }],
        usage: None,
    }];
    let state = build_state_with_defs(defs).await;
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("openai_chat".into(), true)],
        &[("openai_chat".into(), "gpt-4o".into(), "GPT-4o".into())],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();

    // 路由表只含裸名 key，不含限定名 key
    {
        let routes = state.openai_chat_routes.read().await;
        assert!(routes.contains_key("gpt-4o"), "bare key present: {:?}", routes.keys().collect::<Vec<_>>());
        assert!(
            !routes.contains_key("alpha/gpt-4o"),
            "qualified key must NOT exist for non-conflicting model: {:?}",
            routes.keys().collect::<Vec<_>>()
        );
        assert_eq!(routes.len(), 1);
    }

    let base = spawn_proxy(state).await;

    // 裸名请求 200
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// openai_chat 与 openai_responses 冲突检测各自独立：单 provider 在两个通道各声明同名 gpt-4o，
/// 两表互不干扰——各只有一个裸名 key，跨表同名不算冲突。
#[tokio::test(flavor = "multi_thread")]
async fn openai_chat_responses_conflict_independent() {
    let server_chat = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({"model": "gpt-4o"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_chat)
        .await;
    let server_resp = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .and(body_partial_json(serde_json::json!({"model": "gpt-4o"})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("data: {\"type\":\"message_start\"}\n\ndata: [DONE]\n\n".as_bytes(), "text/event-stream"),
        )
        .mount(&server_resp)
        .await;

    // 单 provider，chat 与 responses 两通道各声明 gpt-4o → 跨表同名不冲突，各用裸名 key
    let defs = vec![ProviderDef {
        id: "alpha".into(),
        name: "Alpha".into(),
        icon: None,
        channels: vec![
            ChannelDef {
                channel_type: "openai_chat".into(),
                base_url: server_chat.uri(),
                models_endpoint: None,
            },
            ChannelDef {
                channel_type: "openai_responses".into(),
                base_url: server_resp.uri(),
                models_endpoint: None,
            },
        ],
        usage: None,
    }];
    let state = build_state_with_defs(defs).await;
    update_provider(
        &state.db,
        "alpha",
        UPSTREAM_KEY,
        true,
        &[("openai_chat".into(), true), ("openai_responses".into(), true)],
        &[
            ("openai_chat".into(), "gpt-4o".into(), "GPT-4o".into()),
            ("openai_responses".into(), "gpt-4o".into(), "GPT-4o".into()),
        ],
    )
    .await
    .unwrap();
    refresh_routes(&state).await.unwrap();

    // 两表各只有一个裸名 key（跨表同名不算冲突）
    {
        let chat = state.openai_chat_routes.read().await;
        assert!(chat.contains_key("gpt-4o"), "chat bare key: {:?}", chat.keys().collect::<Vec<_>>());
        assert_eq!(chat.len(), 1, "chat table only bare key");
        let resp = state.openai_responses_routes.read().await;
        assert!(resp.contains_key("gpt-4o"), "responses bare key: {:?}", resp.keys().collect::<Vec<_>>());
        assert_eq!(resp.len(), 1, "responses table only bare key");
    }

    let base = spawn_proxy(state).await;

    // chat 裸名请求 → 命中 server_chat
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server_chat.received_requests().await.unwrap().len(), 1);

    // responses 裸名请求 → 命中 server_resp
    let resp = http()
        .post(format!("{base}/openai-responses/v1/responses"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(server_resp.received_requests().await.unwrap().len(), 1);
}

/// 上游非 2xx 且带 text/event-stream 时，usage 必须落库为 error。
/// 回归点：曾经流式路径无条件写 "success"（status 只用于构造响应、不参与落库判定），
/// 导致 429/5xx 的流式请求在 Dashboard 错误计数与 Logs 页面里彻底不可见。
#[tokio::test]
async fn streamed_non_success_recorded_as_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_raw(
            b"data: {\"error\":\"rate limited\"}\n\n".as_slice(),
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let mut chat = HashMap::new();
    chat.insert("gpt-4o".to_string(), route("gpt-4o", &server.uri()));
    let state = build_state(chat, HashMap::new(), HashMap::new()).await;
    // clone：spawn_proxy 会 move state，测试还要回读 state.db
    let base = spawn_proxy(state.clone()).await;

    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({"model": "gpt-4o", "stream": true, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    // 必须读完流：落库发生在流末，不读完则 usage 永不写入
    let _ = resp.bytes().await.unwrap();

    // write_usage 走 tokio::spawn，请求返回时行还没落库，须轮询等待
    let mut row: Option<(String, Option<String>)> = None;
    for _ in 0..50 {
        row = sqlx::query_as("SELECT status, error_msg FROM usage_records LIMIT 1")
            .fetch_optional(&state.db)
            .await
            .unwrap();
        if row.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (status, err) = row.expect("usage record should be written at stream end");
    assert_eq!(status, "error");
    assert!(err.unwrap().contains("429"));
}
