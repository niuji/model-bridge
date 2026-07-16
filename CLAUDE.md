# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Model Bridge is an LLM API proxy/gateway that routes client requests to upstream LLM providers. It exposes two HTTP servers:

- **Proxy server** (default `:10010`): accepts OpenAI Chat, OpenAI Responses, and Anthropic API requests at `/openai-chat/v1/{*}`, `/openai-responses/v1/{*}`, and `/anthropic/v1/{*}` — three fully independent endpoints, each with its own in-memory route table, model list, and upstream forwarding. Every request is authenticated against an in-memory API-key cache (SHA-256 of the bearer/`x-api-key` token). After auth, the target model is looked up in that endpoint's route table (case-insensitive) and the request is proxied to the correct upstream provider with the correct auth header format.
- **Admin server** (default `:10020`): serves a Vue 3 SPA management UI and a REST admin API at `/api/admin/` for managing provider credentials, API keys, and usage statistics. The admin API has no app-level authentication; it is protected by binding to **loopback (`127.0.0.1`) by default**. Binding it to another address logs a warning at startup — do so only with a trusted network in front.

Provider definitions (id, name, icon, channels) are embedded at compile time via `include_str!("../providers.json")` in `config.rs::load_providers()` — there is no runtime disk read of this file. User-level overrides (`~/.mb/providers.json`, same schema) are loaded and merged at startup: same `id` replaces the builtin entry, new `id` is appended. User-specific credentials and per-model toggles (API keys, enabled state, model whitelists) are stored in SQLite. At runtime, the three layers are merged to build the in-memory route table.

## Build & Run

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Type-check only (no compilation)
cargo check

# Lint
cargo clippy

# Run tests
cargo test

# Run with default config (model-bridge.toml)
cargo run

# Run with custom config
cargo run -- --config /path/to/model-bridge.toml

# Frontend dev server (UI development, proxies to admin server)
cd web && npm install && npm run dev

# Frontend production build → web/dist/ (must rebuild Rust binary to embed)
cd web && npm run build
```

After `npm run build`, rebuild the Rust binary to embed the updated frontend — the binary embeds `web/dist/` at compile time via `include_dir!`. The admin server serves this embedded UI at all non-`/api/` paths.

### Client API key encryption (`encryption_key`, optional)

The client `mb-xxx` gateway keys are stored twice in the `api_keys` table: as a SHA-256 `key_hash` (used by `auth_middleware` for proxy auth) **and** as `api_key` (so the admin UI can reveal the full key later). Without encryption that second copy is plaintext, which defeats the hashing — a DB read yields every live key. `[database] encryption_key` encrypts the `api_key` copy at rest with AES-256-GCM.

Generate a key (base64 of 32 bytes) and set it in `model-bridge.toml`:

```bash
openssl rand -base64 32
```

```toml
[database]
path = "model-bridge.db"
encryption_key = "<output of openssl rand -base64 32>"
```

Behavior:

| `encryption_key` | Result |
|---|---|
| unset / empty | Plaintext storage; logs a startup warning. Only acceptable while the admin server is loopback-bound. |
| set but malformed | Treated as unset (warning + plaintext). Must be base64 decoding to exactly 32 bytes. |
| set correctly | `mb-xxx` keys are sealed before insert (`crypto::seal`), decrypted on read (`crypto::reveal`). |

Notes:
- **Backward compatible**: decryption failure (legacy plaintext rows, or a rotated key) returns the raw stored value, so existing keys need no migration.
- **Rotating the key** invalidates previously-sealed keys — they will no longer decrypt. Re-issue those keys via the admin UI.
- **Auth is unaffected**: proxy auth uses `key_hash` (SHA-256 in-memory lookup), not the encrypted column.
- **Out of scope**: this only encrypts client `mb-` keys. Upstream provider keys (`provider_config.api_key`) remain plaintext and are used directly when forwarding upstream.

### Releases

Push a `v*` tag to trigger `.github/workflows/release.yml`, which builds prebuilt binaries for `x86_64-unknown-linux-musl` (static Linux, no dynamic deps) and `x86_64-pc-windows-msvc` (Windows) and attaches them to a GitHub Release with auto-generated notes. Because the binary embeds `web/dist/` at compile time, the workflow runs `npm ci && npm run build` in `web/` before `cargo build`. Manual dispatch from the Actions tab runs the build only (no release). Each archive also bundles `model-bridge.toml.example` (a commented config template; users copy it to `model-bridge.toml` to set `encryption_key` — the binary runs fine without it via built-in defaults, but the template is the only way to ship the per-deployment encryption key, which must not be baked into the binary).

The **tag is the source of truth for the version** — do not manually bump `Cargo.toml` per release. On a tag push, the build job's `Set version from tag` step writes the tag name (minus `v`) into `Cargo.toml` and runs `cargo update -p model-bridge --precise <version>` to sync `Cargo.lock`, so the built binary's `--version` (sourced from `CARGO_PKG_VERSION` via clap) matches the tag. After the release is published, the `release` job's `Sync Cargo.toml/Cargo.lock version to main` step checks out `main`, re-applies the same bump, and fast-forward pushes it back to `main` as `github-actions[bot]` (`[skip ci]`, `continue-on-error` so a push failure doesn't fail the release). So `main`'s `Cargo.toml`/`Cargo.lock` track the latest tag automatically — no manual bump. `model-bridge --version` / `-V` prints the version.

```bash
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

## Architecture

### Provider Configuration: JSON Definitions + DB Overrides

Providers are defined in three layers that merge at startup:

1. **`providers.json`** (static definition, embedded at compile time) — id, name, icon, and `channels` array (each with `type`, `base_url`, and optional `models_endpoint`). These are loaded once at startup into `AppState.provider_defs` via `include_str!`. The binary always uses the compile-time embedded definition; there is no runtime disk read of a repo-level `providers.json`.
2. **`~/.mb/providers.json`** (user-local overrides) — same schema as `providers.json`. Loaded and merged into layer 1 at startup: same `id` replaces the builtin entry, new `id` is appended. Parse failures are logged and skipped; the file is optional. Use this for private or corporate providers that shouldn't be committed to the repo.
3. **SQLite tables** (runtime overrides) — `provider_config` (api_key, is_enabled), `provider_channel_config` (per-channel is_enabled; base_url is always sourced from the JSON definition, not overridable), `provider_models` (model whitelist).

The merge happens in `provider_svc::merge_channels()` and `list_providers()`/`get_provider()`. User overrides take precedence over JSON defaults. The model list stored in `provider_models` is what gets routed — it is populated either manually via the admin UI or fetched from the provider's `/v1/models` endpoint via the `fetch-models` button. Each `provider_models` row stores both a `model_id` (the canonical identifier sent to the upstream, case preserved) and a `model_name` (display name shown in the `/v1/models` listing).

### Multi-Channel Routing

Each provider can declare multiple channels with different protocol types. A channel's `type` determines which route table the model maps to:

- `"anthropic"` → models go to `anthropic_routes` (served at `/anthropic/v1/{*}`)
- `"openai_chat"` → models go to `openai_chat_routes` (served at `/openai-chat/v1/{*}`)
- `"openai_responses"` → models go to `openai_responses_routes` (served at `/openai-responses/v1/{*}`)

A model is bound to a specific channel — each `provider_models` row carries a `channel_type`, and `refresh_routes()` inserts it only into the route table whose channel matches (`m.channel_type == ch.channel_type`). So a model is NOT auto-surfaced on every channel of its provider: to serve the same `model_id` on both `openai_chat` and `anthropic` endpoints, it must be added to each channel's model list separately (one `provider_models` row per `channel_type`). Adding a new channel to a provider therefore starts with an empty route table for that channel until its models are fetched/added.

### Request Flow

```
Client → Proxy server (:10010) → auth_middleware → endpoint handler (openai_chat / openai_responses / anthropic)
  1. auth_middleware: extract Bearer / x-api-key token, SHA-256 hash, look up in api_key_cache; on match inject AuthenticatedKey(id) into request extensions, else 401.
  2. GET /v1/models → return that endpoint's cached model list from its in-memory route table
  3. POST chat/completions (or responses, or messages) → extract "model" from JSON body → lowercase-lookup in the endpoint's route table → rewrite body's "model" to route.model_id (canonical case) → proxy to the route's base_url with appropriate auth headers
```

Auth header format is set per API format: Bearer token for OpenAI, `x-api-key` for Anthropic. The client-facing auth header (the `mb-xxx` key the gateway issues) is accepted in either form regardless of the upstream protocol.

### Case-Insensitive Model Lookup

Route table `HashMap` keys are lowercased (`model.model_id.to_lowercase()`), so a client request with any-case `model` matches. The route stores the original `model_id`, and `replace_model_in_body()` rewrites the request body's `model` field back to that canonical value before forwarding upstream — so the upstream always sees the exact case stored in `provider_models`.

### Endpoint Independence (OpenAI Paths)

The OpenAI surface is split into two fully independent endpoints — no shared route table, no path-gating:

| Endpoint | Served path | Route table |
|----------|-------------|-------------|
| `/openai-chat/v1/` | `chat/completions`, `models` | `openai_chat_routes` |
| `/openai-responses/v1/` | `responses`, `models` | `openai_responses_routes` |

Each endpoint only serves its own interface's path — `/openai-chat/v1/responses` 404s, as does `/openai-responses/v1/chat/completions`. A model appears in an endpoint's `/v1/models` only if it is listed in `provider_models` under that channel's `channel_type` (not merely because the provider declared the channel), so a chat client never sees responses-only models (and vice versa). The old shared `/openai/v1/*` prefix is gone — existing clients must move their OpenAI base URL to `/openai-chat/v1` (chat) or `/openai-responses/v1` (responses).

Anthropic is a single endpoint: all models in `anthropic_routes` are served at `/anthropic/v1/{*}`.

### SSE Streaming & Usage Extraction

Upstream SSE responses (`content-type: text/event-stream`) are proxied through a tokio mpsc channel. Bytes are buffered and split on `\n` to extract `data:` payloads, handling the fact that SSE events can span multiple TCP chunks and a single chunk can contain multiple events.

Usage is accumulated across all SSE events using a **"last non-zero value"** strategy (not summation): each field (input, output, cache_read, cache_write) is overwritten when a new non-zero value arrives. This is because SSE usage events carry cumulative totals, not increments — `message_delta` in Anthropic carries the final values and should replace `message_start`'s initial/stub values. Summation would double-count providers like glm-5.2 where both `message_start` and `message_delta` carry non-trivial `input_tokens`.

### Usage Normalization Across Protocols

The `input_tokens` column in `usage_records` is normalized differently per protocol so that it always represents the **total input including cache tokens**:

- **OpenAI protocol**: `prompt_tokens` already includes cache tokens, so it's stored as-is. `cache_read_tokens` is extracted from `prompt_tokens_details.cached_tokens` (with fallbacks to `cached_tokens` / `prompt_cache_hit_tokens`).
- **Anthropic protocol**: `input_tokens` excludes cache tokens, so the stored value is `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`. This makes `cache_hit_rate = cache_read / input` bounded and consistent across protocols.

### Proxy Request Modifications

Before forwarding, the proxy makes these modifications to the client's request:

1. **Model name canonicalization**: `model` in the JSON body is rewritten to the exact case stored in `provider_models`.
2. **`stream_options` injection**: For OpenAI `chat/completions` requests with `stream: true` and no existing `stream_options`, the proxy injects `{"stream_options": {"include_usage": true}}` to ensure the upstream returns token usage in its SSE stream.
3. **Auth header rewrite**: The client's `mb-xxx` key is replaced with the upstream provider's API key, formatted as `Bearer` (OpenAI-style upstreams) or `x-api-key` (Anthropic-style upstreams).
4. **Header passthrough**: Only `content-type` and `anthropic-version` are forwarded from the client; all other client headers are dropped.

Upstream request timeout is **480 seconds**. Error responses from the proxy:
- **413** — request body exceeds 64 MiB
- **502** — upstream connection failed or generic upstream error
- **504** — upstream request timed out

### Route Table Refresh

Routes are rebuilt by `provider_svc::refresh_routes()`, which:

1. Iterates all provider definitions from `providers.json`
2. Loads DB overrides for each provider
3. Loads the model whitelist from `provider_models`
4. For each enabled channel of an enabled provider, inserts every model into that channel's route table (`openai_chat_routes`, `openai_responses_routes`, or `anthropic_routes`) — each table is built independently, no cross-channel merging

Refresh triggers: on startup, on a periodic timer (configurable via `bridge.refresh_interval_min`), and after any provider update via the admin API.

### Upstream Drift Detection

A separate periodic task (`probe_upstream_models`, on its own `bridge.probe_interval_min` — default 1440 min / 1 day, decoupled from route refresh) probes each enabled channel's `/v1/models` via the existing `fetch_models_from_api` (using the stored provider key) and transactionally overwrites that (provider, channel) row set in `upstream_models` (DELETE+INSERT; on failure the old snapshot is kept, only a warn is logged). `upstream_models` holds the latest successful upstream list per channel.

Drift is **derived on read, not stored**: `compute_drift` takes the symmetric difference of the current `upstream_models` snapshot vs `upstream_models_seen` — the **baseline**, i.e. the upstream list as of the last time the user opened that provider's "上游变更" modal. New = upstream has / baseline lacks; Removed = baseline has / upstream lacks. Comparison is per-channel + case-insensitive on `model_id`; `model_name`-only differences are ignored (renames skipped). **`provider_models` (the user's curated selection) is not part of the diff** — only routing + actionability — so deliberately-unadopted models are never flagged as "new".

- `GET /api/admin/providers` adds `drift: {new, removed}` per provider (count only; omitted when the baseline is empty / never viewed, to avoid a first-view flood).
- `GET /api/admin/providers/{id}/model-changes` returns per-channel `{added, removed}` lists and **lands the baseline** (`upstream_models_seen := upstream_models`) as a side effect — opening the modal clears the card badge until the next upstream change. First view (empty baseline) returns no drift and just lands the baseline.
- The card badge (`✚N ✖M`) opens a read-only "上游变更" modal listing the changes; adopting/removing models still happens in the existing config modal — this feature only *notices* changes, it doesn't apply them.

### Key Design Decisions

- **In-memory route tables.** Three `HashMap<String, ProviderRoute>` in `AppState` — `openai_chat_routes`, `openai_responses_routes`, `anthropic_routes` — one per client endpoint. Keys are lowercased for case-insensitive lookup; `ProviderRoute` carries the canonical `model_id`/`model_name`/`base_url`/`api_key`. No DB lookup per request.
- **JSON definitions + DB overrides.** Provider metadata that rarely changes (name, channels) lives in `providers.json` (embedded at compile time) and `~/.mb/providers.json` (user overrides at runtime). User-specific secrets and toggles (api_key, is_enabled, model whitelist) live in SQLite. The JSON files are the source of truth for provider identity; DB rows reference them by `provider_id`.
- **Two HTTP servers.** The proxy and admin servers run on separate ports (default 10010/10020) in a `tokio::select!` — if either exits, both shut down.
- **Proxy auth via API-key cache.** The proxy router is wrapped in `auth_middleware`. Client API keys (`mb-{uuid}`, created in the admin UI) are SHA-256 hashed and matched against `AppState.api_key_cache` (`HashMap<key_hash, api_key_id>`, enabled keys only). The cache is refreshed on startup, on the periodic timer, and after any api-key create/delete/toggle. A matched key injects `AuthenticatedKey(id)` into request extensions; `proxy.rs` threads that `api_key_id` into every `usage_records` insert. The admin API has no app-level auth (loopback-bound by default).
- **API key at rest.** The upstream provider API key (`provider_config.api_key`) is stored in plaintext. Client-facing `mb-xxx` keys are stored **twice** in `api_keys`: as a SHA-256 `key_hash` (used by `auth_middleware` for proxy auth) and, to support the "reveal key later" UI feature, as `api_key`. When `[database] encryption_key` is set (base64 of 32 bytes), `api_key` is encrypted at rest via AES-256-GCM (`crypto::seal`/`reveal`); decryption falls back to returning the raw value for legacy plaintext rows. Without a key configured, `api_key` is stored in plaintext (acceptable only while admin is loopback-bound).
- **SSE streaming support.** Upstream SSE responses are proxied through a tokio mpsc channel with line-buffered parsing. Usage is accumulated via "last non-zero value" (not summation) to handle providers where multiple SSE events carry cumulative totals. See [SSE Streaming & Usage Extraction](#sse-streaming--usage-extraction) above for details.
- **SSRF hardening.** HTTP redirects are disabled on the shared `reqwest::Client`. `refresh_routes()` skips channels whose `base_url` is not `http(s)` (`is_safe_base_url` check), logging a warning — this prevents `file://` or other local-protocol URLs from being used as upstream targets.
- **Upstream drift detection.** A separate `probe_interval_min` (default 1 day) task probes each enabled channel's `/v1/models` into an `upstream_models` snapshot; drift is the symmetric diff vs `upstream_models_seen` (the last-viewed baseline) — **not** vs `provider_models` (the curated set). A card badge + read-only "上游变更" modal surface it; opening the modal lands the baseline and clears the badge. See [Upstream Drift Detection](#upstream-drift-detection).
- **Tests.** The `crypto` module has unit tests (seal/reveal roundtrip, key parsing, legacy fallback). The `proxy.rs` module has unit tests covering all pure functions: SSE data-line parsing, model extraction from request bodies, model name canonicalization, `stream_options` injection, usage extraction/normalization across both protocols, SSE event usage parsing, API key masking, and base URL safety checks. Use `cargo test` to run all tests.

## Source Structure

| Path | Purpose |
|------|---------|
| `model-bridge.toml` | Runtime config: proxy/admin server addresses, DB path, refresh interval, encryption key |
| `providers.json` | Static provider definitions (id, name, icon, channels with type/base_url/models_endpoint), embedded at compile time |
| `src/main.rs` | Entry point: config loading, DB init, route table init, background refresh spawn, dual-server launch |
| `src/config.rs` | CLI args (clap), TOML config parsing, `providers.json` loading, `ProviderDef`/`ChannelDef` types |
| `src/state.rs` | `AppState` (in-memory route tables, provider defs, DB pool, HTTP client), model list response types |
| `src/router/mod.rs` | Router assembly: proxy router (`/openai-chat/`, `/openai-responses/`, `/anthropic/`), admin router (`/api/admin/`, SPA fallback), CORS |
| `src/router/proxy.rs` | Core proxy: model extraction, route lookup, upstream forwarding, SSE streaming, usage recording |
| `src/router/models_list.rs` | Returns cached model lists (openai-chat / openai-responses / anthropic) from in-memory route tables |
| `src/router/admin.rs` | Admin HTTP handlers: provider CRUD, API key CRUD, stats queries |
| `src/admin/provider_svc.rs` | Provider business logic: JSON+DB merge, route refresh, `/models` endpoint probing, CRUD |
| `src/admin/stats_svc.rs` | Usage stats queries: overview, per-model, daily, hourly |
| `src/db/schema.rs` | SQLite migrations: `provider_config`, `provider_channel_config`, `provider_models`, `api_keys`, `usage_records`, `upstream_models`, `upstream_models_seen` |
| `src/db/models.rs` | SQLx `FromRow` structs and merge-result DTOs (`ProviderDetail`, `ProviderSummary`, `ChannelDetail`) |
| `src/middleware/mod.rs` + `auth.rs` | API key auth: `auth_middleware` (wired onto the proxy router), `refresh_api_key_cache()`, `AuthenticatedKey` extension type |
| `web/src/` | Vue 3 SPA: `Dashboard.vue`, `Providers.vue`, `ApiKeys.vue`, `Logs.vue`, `Help.vue` (接入指南). Naive UI components, ECharts charts |
| `web/dist/` | Built frontend assets; embedded into the Rust binary via `include_dir!` |

## Admin API Endpoints

- `GET /api/admin/providers` — list providers (merged JSON + DB)
- `GET /api/admin/providers/{id}` — single provider detail (includes api_key, models)
- `PUT /api/admin/providers/{id}` — update provider config (api_key, is_enabled, channel overrides, models)
- `GET /api/admin/providers/{id}/fetch-models?api_key=...&channel=...` — probe upstream `/v1/models` endpoint on a specific channel using the supplied key (not the stored one), return `{models: [{model_id, model_name}]}`
- `GET /api/admin/providers/{id}/model-changes` — return per-channel upstream drift (added/removed) since the last view and land the baseline (clearing the card badge); see [Upstream Drift Detection](#upstream-drift-detection)
- `POST /api/admin/providers/{id}/refresh` — force full route table refresh
- `GET /api/admin/api-keys` — list API keys (with masked preview, no full key)
- `POST /api/admin/api-keys` — create API key (returns full `mb-{uuid}` key once)
- `GET /api/admin/api-keys/{id}` — get full API key
- `PUT /api/admin/api-keys/{id}` — update API key (optional `name` rename and/or `is_enabled` toggle; only `is_enabled` changes refresh the auth cache)
- `DELETE /api/admin/api-keys/{id}` — delete API key
- `GET /api/admin/logs?page=1&page_size=50` — paginated proxy request logs (with decrypted API key previews)
- `GET /api/admin/stats/overview` — total requests, tokens, avg latency, errors (last 7 days)
- `GET /api/admin/stats/models` — per-model usage breakdown (last 7 days)
- `GET /api/admin/stats/daily?days=7` — daily usage for last N days
- `GET /api/admin/stats/hourly` — hourly token usage (last 7 days)
- `GET /api/admin/settings` — return `{"proxy_base_url":"http://<host>:<port>","version":"0.x.y"}`; consumed by the「接入指南」help page to render copy-paste-ready client config snippets and the page footer
