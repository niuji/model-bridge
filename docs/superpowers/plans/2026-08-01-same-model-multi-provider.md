### Task 1: `refresh_routes()` OpenAI 通道 key 改为限定名

**Files:**
- Modify: `src/admin/provider_svc.rs:215-237`（openai 通道 key 拼接）
- Test: `src/router/proxy_route_tests.rs`（新增用例）

**Interfaces:**
- Consumes: `ProviderRoute { provider_id, provider_name, model_id, model_name, base_url, api_key }`（`src/state.rs`）
- Produces: 路由表 key = `format!("{}/{}", def.id, model.model_id.to_lowercase())`，如 `deepseek/deepseek-chat`。同一 `ProviderRoute` 实例被一个限定名 key 引用。

- [ ] **Step 1: 修改 `refresh_routes()` openai 通道 key**

在 `src/admin/provider_svc.rs:215-216`，把：

```rust
for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
    let key = model.model_id.to_lowercase();
    let route = ProviderRoute {
        provider_id: def.id.clone(),
```

改为：

```rust
for model in models.iter().filter(|m| m.channel_type == ch.channel_type) {
    let key = format!("{}/{}", def.id, model.model_id.to_lowercase());
    let route = ProviderRoute {
        provider_id: def.id.clone(),
```

- [ ] **Step 2: 确认冲突处理仍正确**

`Entry::Occupied` 分支（`:229-235`）保留不动。限定名 key 下，跨 provider 同名模型不再冲突（`deepseek/deepseek-chat` vs `openrouter/deepseek-chat` key 不同），只有同一 provider 内部 model 重复才冲突，保留先入者 + warn 正确。

- [ ] **Step 3: 编译检查**

Run: `cargo check`
Expected: 编译通过。

- [ ] **Step 4: 新增测试「限定名精确路由到指定 provider」**

在 `src/router/proxy_route_tests.rs` 末尾追加：

```rust
#[tokio::test(flavor = "multi_thread")]
async fn qualified_name_routes_to_specific_provider() {
    // 两个 provider 声明同名模型，限定名精确命中指定 provider
    let s1 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", format!("Bearer {UPSTREAM_KEY}")))
        .and(body_partial_json(serde_json::json!({"model": "deepseek-chat"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "from-deepseek"}}]
        })))
        .mount(&s1)
        .await;

    let s2 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", format!("Bearer {UPSTREAM_KEY}")))
        .and(body_partial_json(serde_json::json!({"model": "deepseek-chat"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "choices": [{"message": {"content": "from-openrouter"}}]
        })))
        .mount(&s2)
        .await;

    // 构造两个同名限定名 key，指向不同上游
    let mut chat = HashMap::new();
    let mut r1 = route("deepseek-chat", &s1.uri());
    r1.provider_id = "deepseek".into();
    let mut r2 = route("deepseek-chat", &s2.uri());
    r2.provider_id = "openrouter".into();
    chat.insert("deepseek/deepseek-chat".to_string(), r1);
    chat.insert("openrouter/deepseek-chat".to_string(), r2);
    let base = spawn_proxy(build_state(chat, HashMap::new(), HashMap::new()).await).await;

    // 请求 deepseek 限定名 → 命中 s1
    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({
            "model": "deepseek/deepseek-chat",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["choices"][0]["message"]["content"].as_str(), Some("from-deepseek"));
}
```

- [ ] **Step 5: 运行新测试**

Run: `cargo test qualified_name_routes_to_specific_provider -- --nocapture`
Expected: PASS。上游 s1 收到 `model: deepseek-chat`（干净名），响应透传。

- [ ] **Step 6: Commit**

```bash
git add src/admin/provider_svc.rs src/router/proxy_route_tests.rs
git commit -m "feat: qualify openai route keys as {provider_id}/{model_id}"
```

---

### Task 2: `refresh_routes()` Anthropic 通道 key 改为限定名（先 provider 再 claude-）

**Files:**
- Modify: `src/admin/provider_svc.rs:174-196`（anthropic key 派生 + 冲突处理）

**Interfaces:**
- Consumes: Task 1 的限定名 key 约定。
- Produces: anthropic key = 先拼 `{provider_id}/{剥[1m]后的model_id}`，再对非 claude/anthropic 的**整个限定名**补 `claude-` 前缀。

- [ ] **Step 1: 修改 anthropic key 派生逻辑**

在 `src/admin/provider_svc.rs:187-195`，把：

```rust
let lower = route.model_id.to_lowercase();
let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
let key = if clean.starts_with("claude") || clean.starts_with("anthropic") {
    clean.to_string()
} else {
    format!("claude-{}", clean)
};
anthropic_routes.insert(key, route);
```

改为：

```rust
let lower = route.model_id.to_lowercase();
let clean = lower.strip_suffix("[1m]").unwrap_or(&lower);
let qualified = format!("{}/{}", def.id, clean);
let key = if qualified.starts_with("claude") || qualified.starts_with("anthropic") {
    qualified.to_string()
} else {
    format!("claude-{}", qualified)
};
match anthropic_routes.entry(key) {
    std::collections::hash_map::Entry::Vacant(v) => {
        v.insert(route);
    }
    std::collections::hash_map::Entry::Occupied(o) => {
        let existing = o.get();
        tracing::warn!(
            "anthropic model '{}' on '{}' already routed by provider '{}' (base '{}'); keeping first, provider '{}' skipped",
            model.model_id, ch.channel_type, existing.provider_id, existing.base_url, def.id
        );
    }
}
```

注意：`claude-` 前缀的判定现在作用于**整个限定名**（`qualified`），不再作用于剥离后的裸名。非 claude 模型（如 `deepseek-chat`）在 anthropic 端点 key 为 `claude-deepseek/deepseek-chat`；claude 模型（如 `claude-sonnet-4`）key 为 `deepseek/claude-sonnet-4`。

- [ ] **Step 2: 编译检查**

Run: `cargo check`
Expected: 编译通过。注意 `use std::collections::hash_map::Entry` 已在 `:202` 引入（openai 部分），此处直接用 `std::collections::hash_map::Entry::Vacant/Occupied` 全限定名，避免依赖引入位置——但更简洁的做法是复用同模块已引入的 `Entry`。检查 `:202` 的 `use` 是否在函数作用域内（是，`refresh_routes` 函数体内 `:202`）。若在函数作用域内，Task 2 的 `Entry` 可直接用（无需 `std::collections::hash_map::Entry` 前缀）。以编译结果为准，若报「未定义」则在函数体顶部补 `use std::collections::hash_map::Entry;` 或改用全限定名。

- [ ] **Step 3: 新增测试「anthropic 限定名精确路由」**

在 `src/router/proxy_route_tests.rs` 末尾追加：

```rust
#[tokio::test(flavor = "multi_thread")]
async fn anthropic_qualified_name_routes_to_specific_provider() {
    let s1 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(header("x-api-key", UPSTREAM_KEY))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "data: {\"type\":\"message_start\"}\n\n\
                 data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n\
                 data: [DONE]\n\n".as_bytes(),
                "text/event-stream",
            ),
        )
        .mount(&s1)
        .await;

    let s2 = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(header("x-api-key", UPSTREAM_KEY))
        .and(body_partial_json(serde_json::json!({"model": "claude-sonnet-4"})))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"type\":\"message_start\"}\n\n\
             data: {\"type\":\"message_delta\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n\n\
             data: [DONE]\n\n".as_bytes(),
            "text/event-stream",
        ))
        .mount(&s2)
        .await;

    let mut anthropic = HashMap::new();
    let mut r1 = route("claude-sonnet-4", &s1.uri());
    r1.provider_id = "deepseek".into();
    let mut r2 = route("claude-sonnet-4", &s2.uri());
    r2.provider_id = "openrouter".into();
    // anthropic 测试直接构造路由表，key 用「claude 模型限定名」：provider_id/模型名
    anthropic.insert("deepseek/claude-sonnet-4".to_string(), r1);
    anthropic.insert("openrouter/claude-sonnet-4".to_string(), r2);
    let base = spawn_proxy(build_state(HashMap::new(), HashMap::new(), anthropic).await).await;

    let resp = http()
        .post(format!("{base}/anthropic/v1/messages"))
        .headers(auth_headers())
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": "deepseek/claude-sonnet-4",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 10
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}
```

**关键说明**：测试绕过 `refresh_routes()`（直接构造 HashMap），所以 anthropic 测试里的 key 不经过「补 `claude-` 前缀」派生——key 就是测试自己写的 `deepseek/claude-sonnet-4`。生产逻辑里 `claude-sonnet-4` 是 claude 模型，`refresh_routes` 不会补 `claude-` 前缀，所以 `deepseek/claude-sonnet-4` 与生产派生一致。请求 model 用 `deepseek/claude-sonnet-4`，命中 s1。上游收到 `route.model_id` = `claude-sonnet-4`。

- [ ] **Step 4: 运行新测试**

Run: `cargo test anthropic_qualified_name_routes_to_specific_provider -- --nocapture`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src/admin/provider_svc.rs src/router/proxy_route_tests.rs
git commit -m "feat: qualify anthropic route keys (provider prefix, then claude- prefix) + conflict warning"
```

---

### Task 3: `/v1/models` 列表去 dedup + 更新既有测试 + 新增闭环用例

**Files:**
- Modify: `src/router/models_list.rs:25-27`（openai 去 dedup）、`:73-74`（anthropic 去 dedup）、`:58-64`（anthropic 后缀拼接注释）
- Modify: `src/router/proxy_route_tests.rs`（既有测试 key 更新 + 新增列表/404 用例）

**Interfaces:**
- Consumes: Task 1/2 的限定名 key。
- Produces: 列表 id = 路由表 key（限定名），无 dedup。anthropic 列表 id = `{key}{suffix}`（suffix 为 `[1m]`/`[1M]` 时）。

- [ ] **Step 1: openai 列表去掉 dedup**

在 `src/router/models_list.rs:25-27`，把：

```rust
    // 排序后去重
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
```

改为：

```rust
    // 排序。key 已含 provider_id 前缀，限定名天然唯一，无需去重。
    models.sort_by(|a, b| a.id.cmp(&b.id));
```

- [ ] **Step 2: anthropic 列表去掉 dedup + 后缀拼接注释**

在 `src/router/models_list.rs:73-74`，把：

```rust
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
```

改为：

```rust
    models.sort_by(|a, b| a.id.cmp(&b.id));
```

在 `:58-64`（anthropic 后缀补回逻辑），更新注释说明 key 已含 provider 前缀：

```rust
    for (key, route) in routes.iter() {
        let id = if route.model_id.to_lowercase().ends_with("[1m]") {
            // 补回原始大小写的后缀；[1m]/[1M] 为末 4 个 ASCII 字节，ends_with 已保证按字节切安全
            // key 已含 provider_id 前缀（如 deepseek/claude-sonnet-4），后缀拼在其后
            let suffix = &route.model_id[route.model_id.len() - 4..];
            format!("{}{}", key, suffix)
        } else {
            key.clone()
        };
```

- [ ] **Step 3: 编译检查**

Run: `cargo check`
Expected: 编译通过。

- [ ] **Step 4: 更新既有测试的 key 为限定名**

现有测试直接构造路由表，key 改为限定名（`route()` 的 `provider_id` 是 `"prov"`，故 key 用 `prov/{model_id}`）：

`src/router/proxy_route_tests.rs`:

- `:180` `chat.insert("gpt-4o".to_string(), route("GPT-4o", "http://unused"));` → `chat.insert("prov/gpt-4o".to_string(), route("GPT-4o", "http://unused"));`
- `:233` `chat.insert("gpt-4o".to_string(), route("GPT-4o", &dead_url));` → `chat.insert("prov/gpt-4o".to_string(), route("GPT-4o", &dead_url));`
- `:265` `chat.insert("gpt-4o".to_string(), route("GPT-4o", &server.uri()));` → `chat.insert("prov/gpt-4o".to_string(), route("GPT-4o", &server.uri()));`
- `:298` `responses.insert("gpt-4o".to_string(), route("GPT-4o", &server.uri()));` → `responses.insert("prov/gpt-4o".to_string(), route("GPT-4o", &server.uri()));`
- `:332-335` `anthropic.insert("claude-sonnet-4".to_string(), route("claude-sonnet-4", &server.uri()));` → `anthropic.insert("prov/claude-sonnet-4".to_string(), route("claude-sonnet-4", &server.uri()));`

同时更新这些测试的**请求 model** 为限定名（否则 404）：

- `model_not_in_route_table_returns_404`（`:168` 请求 model `gpt-4o` + `:174` 断言 error 含 `gpt-4o`）→ 请求 model 改 `prov/gpt-4o`，断言 error 含 `prov/gpt-4o`（此测试路由表为空，任何名都 404，改成限定名与「限定名 404」语义一致）
- `models_list_isolated_per_endpoint`（`:180` key + `:197` 断言 `["gpt-4o"]`）→ 断言改为 `["prov/gpt-4o"]`
- `openai_chat_forwards_canonical_model_and_injects_stream_options`（`:265` key + `:273` 请求 model `gpt-4o`）→ 请求 model `prov/gpt-4o`；上游断言 body `model: "GPT-4o"` 不变（回写干净名）
- `openai_responses_forwards_canonical_model`（`:298` key + `:304` 请求 model `gpt-4o`）→ 请求 model `prov/gpt-4o`
- `anthropic_forwards_sse_and_uses_x_api_key_header`（`:332` key + `:343` 请求 model `claude-sonnet-4`）→ 请求 model `prov/claude-sonnet-4`

**注意 `upstream_connect_failure_returns_502`**（`:233` key + `:239` 请求 model `gpt-4o`）→ key `prov/gpt-4o`、请求 model `prov/gpt-4o`。

- [ ] **Step 5: 运行全部测试**

Run: `cargo test`
Expected: 全部 PASS（含 Task 1/2 新增 + 既有更新）。

- [ ] **Step 6: 新增「裸名 404」用例（闭环验证）**

在 `src/router/proxy_route_tests.rs` 末尾追加：

```rust
#[tokio::test(flavor = "multi_thread")]
async fn bare_model_name_returns_404_when_only_qualified_key_exists() {
    // 列表只下发限定名，路由表只有限定名 key → 裸名请求 404（闭环：列表里也没有此名）
    let mut chat = HashMap::new();
    chat.insert("prov/gpt-4o".to_string(), route("GPT-4o", "http://unused"));
    let base = spawn_proxy(build_state(chat, HashMap::new(), HashMap::new()).await).await;

    let resp = http()
        .post(format!("{base}/openai-chat/v1/chat/completions"))
        .headers(auth_headers())
        .json(&serde_json::json!({"model":"gpt-4o","messages":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
```

- [ ] **Step 7: 新增「列表只含限定名」用例**

在 `src/router/proxy_route_tests.rs` 末尾追加：

```rust
#[tokio::test(flavor = "multi_thread")]
async fn models_list_exposes_qualified_names_only() {
    // 两个 provider 同名模型 → 列表两条限定名，无裸名
    let mut chat = HashMap::new();
    let mut r1 = route("deepseek-chat", "http://unused");
    r1.provider_id = "deepseek".into();
    let mut r2 = route("deepseek-chat", "http://unused");
    r2.provider_id = "openrouter".into();
    chat.insert("deepseek/deepseek-chat".to_string(), r1);
    chat.insert("openrouter/deepseek-chat".to_string(), r2);
    let base = spawn_proxy(build_state(chat, HashMap::new(), HashMap::new()).await).await;

    let r = http()
        .get(format!("{base}/openai-chat/v1/models"))
        .headers(auth_headers())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let b: serde_json::Value = r.json().await.unwrap();
    let ids: Vec<&str> = b["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["deepseek/deepseek-chat", "openrouter/deepseek-chat"]);
}
```

- [ ] **Step 8: 运行全部测试 + clippy**

Run: `cargo test && cargo clippy`
Expected: 全部 PASS，无 clippy 警告。

- [ ] **Step 9: Commit**

```bash
git add src/router/models_list.rs src/router/proxy_route_tests.rs
git commit -m "feat: expose qualified-only model lists, update tests, closed-loop 404"
```
