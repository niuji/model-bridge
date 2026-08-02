# Anthropic 条件限定名路由 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 anthropic 路由表的 key 策略从「每个模型双 key」改为「仅冲突模型使用限定名 key + display_name 前缀，非冲突模型只用裸名 key」。

**Architecture:** `refresh_routes()` 的 anthropic 部分改为两遍构建——第一遍统计归一化裸名的出现次数判定冲突集合，第二遍按冲突与否生成 key：非冲突用裸名、冲突只用限定名（`claude-{provider}/{clean_id}`）；冲突模型的 `model_name` 在建表时改写为带 `[{provider_id}]` 前缀，列表侧直接取用 `route.model_name`（`models_list` 与 `provider_svc` 必须同批提交，否则冲突模型 display_name 会出现双前缀或丢前缀）。

**Tech Stack:** Rust, axum, sqlx, tokio, wiremock (test)

## Global Constraints

- 归一化裸名规则不变：`model_id.to_lowercase()` → 剥 `[1m]` 后缀 → 非 `claude`/`anthropic` 开头补 `claude-` 前缀。
- 冲突判定用归一化裸名**总出现次数 > 1**（含同 provider 内归一化同名），不是按 provider 维度。
- 冲突模型只用限定名 key，裸名 key 完全不建（客户端裸名请求 → 404）。
- 非冲突模型只用裸名 key，无限定名 key。
- display_name 前缀 `[{provider_id}]` 只在建表时写入冲突模型的 `model_name`；`models_list.rs` 不做前缀拼接。
- `state.rs` 不改动（无新字段）。`proxy.rs` 转发逻辑零改动。
- 工作区已有未提交改动（`models_list.rs` 的 `[alpha]` 前缀格式、`proxy_route_tests.rs` 对应断言）与本任务共存，不要回退它们。
- 每个任务结束时 `cargo test` 必须全绿（含既有用例），不允许中间提交破坏测试。

---

### Task 1: 两遍构建 + display_name 直接取 model_name + 冲突测试更新

**Files:**
- Modify: `src/admin/provider_svc.rs:176-223`
- Modify: `src/router/models_list.rs:65-72`
- Modify: `src/router/proxy_route_tests.rs`（`anthropic_qualified_name_routes_to_correct_provider` 裸名断言 + 裸名 404 断言 + 重写 `anthropic_qualified_name_upstream_body_is_clean_model_id`）

**Interfaces:**
- Consumes: `channels`/`models`/`def`/`api_key` 等外层已就绪的变量；`ProviderRoute` 结构（`state.rs`，本任务不改）。
- Produces: `anthropic_routes: HashMap<String, ProviderRoute>`，key 为裸名或限定名；冲突模型的 `model_name` 已带 `[{provider_id}]` 前缀；`get_anthropic_models` 的 display_name = `route.model_name.clone()`。Task 2 的非冲突测试依赖此不变量。

> 本任务将 `provider_svc`（前缀写入 model_name）与 `models_list`（display_name 直接取 model_name）**同批提交**——两者必须一起落地，否则中间状态冲突模型的 display_name 会双前缀或丢前缀。测试更新也在本任务内，保证提交时全绿。

- [ ] **Step 1: 重写 anthropic 建表块为两遍构建**

将 `provider_svc.rs:176-223`（从 `// ---- anthropic 路由...` 注释到限定名 insert 的 `}` 闭括号，即 184-223 行的 for 循环整体）替换为以下代码：

```rust
        // ---- anthropic 路由：每个启用的 anthropic 通道，插入「归属该通道」的模型 ----
        // 检索 key 由 model_id 派生（剥 [1m] 后缀；非 claude/anthropic 开头的补 claude- 前缀），
        // 与 proxy 转发剥除 [1m] 的逻辑配套。
        //
        // 两遍构建：先统计归一化裸名出现次数判定冲突，再按冲突与否生成 key。
        //   - 非冲突模型：只用裸名 key（count==1 保证唯一，直接 insert）。
        //   - 冲突模型（归一化裸名 count>1，含同 provider 归一化同名）：
        //     只用 `claude-{provider}/{model}` 限定名 key，裸名 key 完全不建；
        //     model_name 改写为带 [{provider_id}] 前缀（列表侧区分同名来源，转发不看它）。
        let mut bare_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for ch in enabled.iter().copied().filter(|c| c.channel_type == "anthropic") {
            for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let lower = model.model_id.to_lowercase();
                let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
                let bare = if clean.starts_with("claude") || clean.starts_with("anthropic") {
                    clean.to_string()
                } else {
                    format!("claude-{}", clean)
                };
                *bare_counts.entry(bare).or_insert(0) += 1;
            }
        }

        for ch in enabled.iter().copied().filter(|c| c.channel_type == "anthropic") {
            for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
                let route = ProviderRoute {
                    provider_id: def.id.clone(),
                    provider_name: def.name.clone(),
                    model_id: model.model_id.clone(),
                    model_name: model.model_name.clone(),
                    base_url: ch.base_url.clone(),
                    api_key: api_key.clone(),
                };
                let lower = route.model_id.to_lowercase();
                let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
                let bare = if clean.starts_with("claude") || clean.starts_with("anthropic") {
                    clean.to_string()
                } else {
                    format!("claude-{}", clean)
                };
                if bare_counts.get(&bare).copied().unwrap_or(0) == 1 {
                    // 非冲突：裸名 key（count==1 保证唯一，无冲突分支）
                    anthropic_routes.insert(bare, route);
                } else {
                    // 冲突：只用限定名 key；model_name 打上 [{provider_id}] 前缀（列表侧区分来源）
                    let clean_id = clean.strip_prefix("claude-").unwrap_or(clean);
                    let qualified_key = format!("claude-{}/{}", def.id, clean_id);
                    // 同 provider 归一化同名等边缘场景下限定名 key 可能撞车，保留先入者 + warn
                    match anthropic_routes.entry(qualified_key) {
                        Entry::Vacant(v) => {
                            let mut route = route;
                            route.model_name = format!("[{}]{}", def.id, route.model_name);
                            v.insert(route);
                        }
                        Entry::Occupied(o) => {
                            let existing = o.get();
                            tracing::warn!(
                                "model '{}' on 'anthropic' channel already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
                                model.model_id, existing.provider_id, existing.base_url, def.id
                            );
                        }
                    }
                }
            }
        }
```

- [ ] **Step 2: 修改 `get_anthropic_models` 的 display_name**

将 `models_list.rs:65-72` 中 `display_name: format!("[{}]{}", route.provider_id, route.model_name),` 及上方注释替换为：

```rust
            // display_name 直接用 model_name：冲突模型的 [{provider_id}] 前缀已在建表时写入，
            // 非冲突模型是纯 model_name。仅展示用，不影响路由 key（id）与上游转发（route.model_id）
            display_name: route.model_name.clone(),
```

- [ ] **Step 3: 修改冲突测试的路由表断言（裸名不存在）**

在 `proxy_route_tests.rs` 的 `anthropic_qualified_name_routes_to_correct_provider` 函数内，找到 `// 路由表应同时含裸名 key 与两条限定名 key` 注释块，将其整体替换为：

```rust
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
```

- [ ] **Step 4: 在该函数末尾（限定名请求断言之后）追加裸名 404 断言**

在 `anthropic_qualified_name_routes_to_correct_provider` 函数末尾（最后一行 `assert_eq!(server_b.received_requests().await.unwrap().len(), 1);` 之后、函数结束 `}` 之前）追加：

```rust
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
```

- [ ] **Step 5: 重写 `anthropic_qualified_name_upstream_body_is_clean_model_id` 为冲突场景**

将整个函数替换为（两个 provider 声明相同 `claude-kimi-k3[1M]`，用限定名验证上游收到干净 `claude-kimi-k3`）：

```rust
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
```

- [ ] **Step 6: 运行测试验证全绿**

Run: `cargo test proxy_route_tests -- --nocapture`
Expected: 全部通过。冲突测试的 display_name 断言（`[alpha]Claude Sonnet 4` / `[beta]Claude Sonnet 4`）应通过——`model_name` 建表时已写入前缀，`models_list` 直接取用，无双前缀。

- [ ] **Step 7: 提交**

```bash
git add src/admin/provider_svc.rs src/router/models_list.rs src/router/proxy_route_tests.rs
git commit -m "feat: anthropic 按需限定名路由 — 仅冲突模型用限定名 key + model_name 前缀

两遍构建：第一遍统计归一化裸名 count，第二遍非冲突只建裸名 key，
冲突模型只建 claude-{provider}/{model} 限定名 key（裸名 404）并在 model_name
打 [{provider_id}] 前缀；models_list display_name 直接取 model_name。

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: 新增非冲突场景测试

**Files:**
- Modify: `src/router/proxy_route_tests.rs`（在 `anthropic_qualified_name_upstream_body_is_clean_model_id` 之后追加新用例）

**Interfaces:**
- Consumes: Task 1 的不变量（非冲突模型只有裸名 key；`model_name` 无前缀）。
- Produces: 对非冲突场景（裸名 key 唯一、display_name 无前缀、裸名请求 200）的回归保护。

- [ ] **Step 1: 追加非冲突场景测试**

在 `proxy_route_tests.rs` 的 `anthropic_qualified_name_upstream_body_is_clean_model_id` 函数结束 `}` 之后追加：

```rust
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
```

- [ ] **Step 2: 运行测试验证全绿**

Run: `cargo test proxy_route_tests -- --nocapture`
Expected: 全部通过（含既有 `anthropic_forwards_sse_and_uses_x_api_key_header`、冲突用例、重写后的 kimi 用例、新增非冲突用例）。

- [ ] **Step 3: 提交**

```bash
git add src/router/proxy_route_tests.rs
git commit -m "test: 非冲突场景只建裸名 key、display_name 无前缀、裸名请求 200

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: 全量验证 + 收尾

**Files:**
- 无代码改动，仅验证。

- [ ] **Step 1: 运行完整测试套件**

Run: `cargo test`
Expected: 全部通过（proxy.rs 单测 + proxy_route_tests 路由测试 + crypto/provider_svc 单测）。

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy --all-targets`
Expected: 无新警告。

- [ ] **Step 3: 核对工作区状态**

Run: `git status`
Expected: 仅剩本任务的 2 个提交后的干净工作区（Task 1 已把用户之前未提交的 `[alpha]` 格式改动并入提交；若有残留未提交改动需向用户说明）。
