# OpenAI (chat/responses) 跨 provider 同名模型限定名路由 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 openai_chat 与 openai_responses 两个通道像 anthropic 一样，跨 provider 声明同名模型时用限定名 key（`{provider_id}/{model_id}`）精确路由，替代当前的"保留先入者、后入者丢弃"。

**Architecture:** 在 `provider_svc::refresh_routes` 的 openai 部分增加一个预扫描 pass，按通道独立统计裸名出现次数；主循环按 count==1/count>1 决定用裸名 key 还是限定名 key。限定名 key 中 `provider_id` 转小写（与代理查找侧 `to_lowercase` 对齐）。`models_list.rs`、`proxy.rs`、`state.rs`、前端零改动。

**Tech Stack:** Rust + axum + sqlx + tokio；wiremock 路由级测试（`src/router/proxy_route_tests.rs`）。

## Global Constraints

- 限定名 key 格式：`format!("{}/{}", def.id.to_lowercase(), key_lower)`，其中 `key_lower = model.model_id.to_lowercase()`（openai 的 bare 归一化就是纯 `to_lowercase`，无 `[1m]` 剥离、无 `claude-` 前缀补全）。
- 冲突检测粒度：`openai_chat` 与 `openai_responses` **各自独立**统计，跨表同名互不影响。
- 冲突模型（count>1）：只用限定名 key，裸名 key 完全不建；`model_name` 打 `[{}]{}` 前缀（`provider_id` 保留原始大小写，仅展示用）。
- 非冲突模型（count==1）：只用裸名 key，无限定名 key。
- **无 `[1m]` 变体优先级分支**：Occupied 撞车一律保留先入者 + warn（OpenAI 无 1M 上下文概念）。
- `provider_id` 大写时 key 必须转小写——这是已修的 anthropic 同款 bug，openai 不得再犯。
- 测试沿用 `build_state_with_defs` + `update_provider` + `refresh_routes` + wiremock 模式。

---

### Task 1: openai 通道裸名冲突预扫描 + 主循环限定名 key

**Files:**
- Modify: `src/admin/provider_svc.rs:130-167`（openai 预扫描，紧接 anthropic 预扫描之后）
- Modify: `src/admin/provider_svc.rs:282-321`（openai 主循环改两遍构建）
- Test: `src/router/proxy_route_tests.rs`（新增 3 个用例，见 Task 2）

**Interfaces:**
- Consumes: `refresh_routes` 的现有结构；`HashMap` 已 import；`Entry` 在 `provider_svc.rs:205` 已 import。
- Produces: 两个新局部 map `openai_chat_bare_counts` / `openai_responses_bare_counts`（`HashMap<String, usize>`）；openai 主循环按通道查对应 map 决定 key。

- [ ] **Step 1: 在 anthropic 预扫描后追加 openai 预扫描**

在 `provider_svc.rs:167`（anthropic 预扫描 `for def` 循环的闭合 `}` 之后、`for def in &state.provider_defs {` 主循环之前）插入 openai 预扫描。复用同一个 `for def` 遍历结构，但统计进两个独立的 openai map：

```rust
    // 预计算 openai 裸名冲突表（chat/responses 各自独立，跨 provider 同名冲突）：
    // openai 的 bare 归一化就是纯 to_lowercase（无 [1m] 剥离、无 claude- 前缀补全）。
    // 与 anthropic 预扫描共用同一批 DB 查询（enabled/channels/models），但按通道分别计数。
    let mut openai_chat_bare_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut openai_responses_bare_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for def in &state.provider_defs {
        let config = get_provider_config(&state.db, &def.id).await;
        let is_enabled = config.as_ref().map(|c| c.is_enabled).unwrap_or(false);
        if !is_enabled {
            continue;
        }
        let api_key = config.as_ref().map(|c| c.api_key.clone()).unwrap_or_default();
        if api_key.is_empty() {
            continue;
        }
        let channel_configs = get_channel_configs(&state.db, &def.id).await;
        let channels = merge_channels(&def.channels, &channel_configs);
        let enabled: Vec<&ChannelDetail> = channels
            .iter()
            .filter(|c| c.is_enabled && is_safe_base_url(&c.base_url))
            .collect();
        let models = sqlx::query_as::<_, ProviderModel>(
            "SELECT id, provider_id, channel_type, model_id, model_name FROM provider_models WHERE provider_id = ?",
        )
        .bind(&def.id)
        .fetch_all(&state.db)
        .await?;
        for ch in enabled.iter().copied().filter(|c| c.channel_type == "openai_chat" || c.channel_type == "openai_responses") {
            let table: &mut std::collections::HashMap<String, usize> = match ch.channel_type.as_str() {
                "openai_chat" => &mut openai_chat_bare_counts,
                "openai_responses" => &mut openai_responses_bare_counts,
                _ => unreachable!(),
            };
            for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let bare = model.model_id.to_lowercase();
                *table.entry(bare).or_insert(0) += 1;
            }
        }
    }
```

- [ ] **Step 2: 编译检查**

Run: `cargo check`
Expected: 编译通过（此时新 map 尚未使用，可能有 `unused variable` 警告——Step 3 消除）。

- [ ] **Step 3: 改写 openai 主循环为按冲突与否生成 key**

把 `provider_svc.rs:282-321` 的 openai 建表循环整体替换为：

```rust
        // ---- openai 路由：chat 与 responses 各自独立建表，不再合并 ----
        // 每个启用的 openai 通道单独成一张路由表：openai_chat → openai_chat_routes，
        // openai_responses → openai_responses_routes。模型归属哪个通道就进哪张表，转发用该通道 base_url，
        // 无需按 path 过滤。跨 provider 同名 model_id 冲突时（按通道独立统计）改用限定名 key
        // `{provider_id}/{model_id}`，裸名 key 不建；不冲突的模型仍用裸名 key。
        for ch in enabled.iter().copied().filter(|c| c.channel_type != "anthropic") {
            let table: &mut HashMap<String, ProviderRoute> = match ch.channel_type.as_str() {
                "openai_chat" => &mut openai_chat_routes,
                "openai_responses" => &mut openai_responses_routes,
                other => {
                    tracing::warn!(
                        "provider '{}' channel '{}' has unknown openai channel type, skipped from routing",
                        def.id, other
                    );
                    continue;
                }
            };
            // 冲突计数表：按通道选对应的预扫描结果
            let bare_counts: &HashMap<String, usize> = match ch.channel_type.as_str() {
                "openai_chat" => &openai_chat_bare_counts,
                "openai_responses" => &openai_responses_bare_counts,
                _ => unreachable!(),
            };
            for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let key_lower = model.model_id.to_lowercase();
                let route = ProviderRoute {
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                };
                if bare_counts.get(&key_lower).copied().unwrap_or(0) == 1 {
                    // 非冲突：裸名 key（count==1 保证唯一，无冲突分支）
                    match table.entry(key_lower) {
                        Entry::Vacant(v) => {
                            v.insert(route);
                        }
                        Entry::Occupied(o) => {
                            // 理论上不可达（count==1），防御性保留
                            let existing = o.get();
                            tracing::warn!(
                                "model '{}' on '{}' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                                model.model_id, ch.channel_type, existing.provider_id, existing.base_url, def.id
                            );
                        }
                    }
                } else {
                    // 冲突：只用限定名 key；model_name 打上 [{provider_id}] 前缀（列表侧区分来源）
                    let mut prefixed_route = route;
                    prefixed_route.model_name = format!("[{}]{}", def.id, prefixed_route.model_name);
                    let qualified_key = format!("{}/{}", def.id.to_lowercase(), key_lower);
                    match table.entry(qualified_key) {
                        Entry::Vacant(v) => {
                            v.insert(prefixed_route);
                        }
                        Entry::Occupied(o) => {
                            let existing = o.get();
                            tracing::warn!(
                                "model '{}' on '{}' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                                model.model_id, ch.channel_type, existing.provider_id, existing.base_url, def.id
                            );
                        }
                    }
                }
            }
        }
```

- [ ] **Step 4: 编译检查 + 现有测试全绿**

Run: `cargo check && cargo test`
Expected: 编译通过，72 个测试全过（现有 openai 测试如 `models_list_isolated_per_endpoint`、`openai_chat_forwards_canonical_model_and_injects_stream_options` 不涉及跨 provider 同名，行为不变）。

- [ ] **Step 5: 提交**

```bash
git add src/admin/provider_svc.rs
git commit -m "feat: openai 跨 provider 同名模型用限定名 key 路由

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: openai 限定名路由回归测试

**Files:**
- Modify: `src/router/proxy_route_tests.rs`（在 `anthropic_qualified_name_lowercases_provider_id_in_key` 之后追加 3 个用例）

**Interfaces:**
- Consumes: `build_state_with_defs(defs: Vec<ProviderDef>)`、`update_provider(&state.db, id, api_key, enabled, channels, models)`、`refresh_routes(&state)`、`spawn_proxy(state)`、`http()`、`auth_headers()`、`UPSTREAM_KEY`、wiremock `Mock`/`MockServer`/`ResponseTemplate`——全部已存在于测试文件。
- Produces: 3 个 `#[tokio::test(flavor = "multi_thread")]` 用例，验证 Task 1 的 key 生成与 HTTP 行为。

- [ ] **Step 1: 写失败测试 1（冲突模型用限定名 key）**

在文件末尾追加：

```rust
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
```

- [ ] **Step 2: 跑测试 1 确认失败**

Run: `cargo test openai_chat_conflicting_models_use_qualified_key 2>&1 | tail -20`
Expected: FAIL——路由表里仍是裸名 `gpt-4o`（Task 1 未完成时），断言 `!routes.contains_key("gpt-4o")` 触发。

- [ ] **Step 3: 写失败测试 2（非冲突模型只用裸名 key）**

继续追加：

```rust
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
```

- [ ] **Step 4: 跑测试 2 确认失败**

Run: `cargo test openai_chat_non_conflicting_uses_only_bare_key 2>&1 | tail -20`
Expected: FAIL——Task 1 未完成时主循环 `table.entry(key_lower)` 是裸名 `gpt-4o`，`assert!(!routes.contains_key("alpha/gpt-4o"))` 实际会**通过**（因为当前代码不生成限定名 key）……注意：**这个测试在旧代码下可能意外通过**。若如此，跳到 Task 1 完成后再回来验证。真正的断言价值在 Task 1 完成后：主循环必须仍为 count==1 生成裸名 key、且不能额外生成限定名 key。

- [ ] **Step 5: 写失败测试 3（chat/responses 冲突检测独立）**

继续追加：

```rust
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
```

- [ ] **Step 6: 跑测试 3 确认失败**

Run: `cargo test openai_chat_responses_conflict_independent 2>&1 | tail -20`
Expected: FAIL——Task 1 未完成时主循环 `table.entry(key_lower)` 对 chat 表插入 `gpt-4o` 裸名 key、responses 表也插入 `gpt-4o`，两表各 1 个，`assert_eq!(chat.len(), 1)` 会**通过**。此测试真正捕获的是 Task 1 的错误实现（例如把跨表同名算成冲突，导致两表出现限定名 key）。若当前代码下意外通过，属正常——本测试是"Task 1 实现必须不破坏现状"的守卫，而非驱动 Task 1 的 RED。

- [ ] **Step 7: 跑全部 anthropic + openai 测试确认绿灯**

Run: `cargo test 2>&1 | tail -8`
Expected: 75 个测试全过（72 + 3 新增）。

- [ ] **Step 8: 提交**

```bash
git add src/router/proxy_route_tests.rs
git commit -m "test: openai 跨 provider 同名模型限定名路由

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Self-Review 记录

**Spec coverage 对照：**

| Spec 要求 | Plan 任务 |
|---|---|
| 非冲突 count==1 → 裸名 key | Task 1 Step 3 `bare_counts.get(&key_lower)==1` 分支 |
| 冲突 count>1 → 限定名 key `{provider_id}/{model_id}` | Task 1 Step 3 else 分支 `format!("{}/{}", def.id.to_lowercase(), key_lower)` |
| provider_id 转小写 | Task 1 Step 3 `def.id.to_lowercase()` |
| 冲突模型 model_name 加 `[{provider_id}]` 前缀 | Task 1 Step 3 `prefixed_route.model_name = format!("[{}]{}", def.id, ...)` |
| chat/responses 冲突检测独立 | Task 1 Step 1 两个独立 map + Step 3 按通道查对应 map；Task 2 测试 3 |
| 无 `[1m]` 优先级分支 | Task 1 Step 3 Occupied 分支统一保留先入者 + warn |
| 列表侧/转发零改动 | 两任务均不触碰 `models_list.rs`/`proxy.rs` |
| 3 个回归测试 | Task 2 Step 1/3/5 |

**Placeholder 扫描：** 无 TBD/TODO；每步含完整代码与精确命令。测试 2/3 在旧代码下可能意外通过的说明已写入 Step 4/6，避免执行者误判。

**类型一致性：** `openai_chat_bare_counts`/`openai_responses_bare_counts` 类型 `HashMap<String, usize>`，Task 1 Step 3 用 `bare_counts.get(&key_lower).copied().unwrap_or(0)` 读取——与 anthropic 分支 `bare_counts.get(&bare).copied().unwrap_or(0)` 一致。`qualified_key = format!("{}/{}", def.id.to_lowercase(), key_lower)`，测试断言 `alpha/gpt-4o`/`beta/gpt-4o` 匹配。
