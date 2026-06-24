# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Model Bridge is an LLM API proxy/gateway that routes client requests to upstream LLM providers. It exposes two HTTP servers:

- **Proxy server** (default `:10010`): accepts OpenAI-compatible and Anthropic-compatible API requests at `/openai/v1/{*}` and `/anthropic/v1/{*}`. Every request is authenticated against an in-memory API-key cache (SHA-256 of the bearer/`x-api-key` token). After auth, the target model is looked up in an in-memory route table (case-insensitive) and the request is proxied to the correct upstream provider with the correct auth header format.
- **Admin server** (default `:10020`): serves a Vue 3 SPA management UI and a REST admin API at `/api/admin/` for managing provider credentials, API keys, and usage statistics. The admin API has no app-level authentication; it is protected by binding to **loopback (`127.0.0.1`) by default**. Binding it to another address logs a warning at startup — do so only with a trusted network in front.

Provider definitions (name, channels, models endpoint) live in a static JSON file (`providers.json`). User-specific credentials and overrides (API keys, enabled state, channel base URLs, model whitelists) are stored in SQLite. At runtime, the two are merged to build the in-memory route table.

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

# Run with default config (model-bridge.toml + providers.json)
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

Push a `v*` tag to trigger `.github/workflows/release.yml`, which builds prebuilt binaries for `x86_64-unknown-linux-musl` (static Linux, no dynamic deps) and `x86_64-pc-windows-msvc` (Windows) and attaches them to a GitHub Release with auto-generated notes. Because the binary embeds `web/dist/` at compile time, the workflow runs `npm ci && npm run build` in `web/` before `cargo build`. Manual dispatch from the Actions tab runs the build only (no release).

```bash
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

## Architecture

### Provider Configuration: JSON Definitions + DB Overrides

Providers are defined in three layers that merge at startup:

1. **`providers.json`** (static definition) — id, name, icon, `models_endpoint` (optional), and `channels` array (each with `type` and `base_url`). These are loaded once at startup into `AppState.provider_defs`.
2. **`~/.mb/providers.json`** (user-local overrides) — same schema as `providers.json`. Loaded and merged into layer 1 at startup: same `id` replaces the builtin entry, new `id` is appended. Parse failures are logged and skipped; the file is optional. Use this for private or corporate providers that shouldn't be committed to the repo.
3. **SQLite tables** (runtime overrides) — `provider_config` (api_key, is_enabled), `provider_channel_config` (per-channel base_url override, is_enabled), `provider_models` (model whitelist).

The merge happens in `provider_svc::merge_channels()` and `list_providers()`/`get_provider()`. User overrides take precedence over JSON defaults. The model list stored in `provider_models` is what gets routed — it is populated either manually via the admin UI or fetched from the provider's `/v1/models` endpoint via the `fetch-models` button. Each `provider_models` row stores both a `model_id` (the canonical identifier sent to the upstream, case preserved) and a `model_name` (display name shown in the `/v1/models` listing).

### Multi-Channel Routing

Each provider can declare multiple channels with different protocol types. A channel's `type` determines which route table the model maps to:

- `"anthropic"` → models go to `anthropic_routes` (served at `/anthropic/v1/{*}`)
- Everything else (`"openai_chat"`, `"openai_responses"`, etc.) → models go to `openai_routes` (served at `/openai/v1/{*}`)

A model listed under a provider with both an `openai_chat` and an `anthropic` channel will appear in BOTH route tables, routing to different base URLs per channel.

### Request Flow

```
Client → Proxy server (:10010) → auth_middleware → proxy.rs handler
  1. auth_middleware: extract Bearer / x-api-key token, SHA-256 hash, look up in api_key_cache; on match inject AuthenticatedKey(id) into request extensions, else 401.
  2. GET /v1/models → return cached model list from in-memory route table
  3. POST /v1/chat/completions etc. → extract "model" from JSON body → lowercase-lookup in route table → rewrite body's "model" to route.model_id (canonical case) → proxy to upstream provider with appropriate auth headers
```

Auth header format is set per API format: Bearer token for OpenAI, `x-api-key` for Anthropic. The client-facing auth header (the `mb-xxx` key the gateway issues) is accepted in either form regardless of the upstream protocol.

### Case-Insensitive Model Lookup

Route table `HashMap` keys are lowercased (`model.model_id.to_lowercase()`), so a client request with any-case `model` matches. The route stores the original `model_id`, and `replace_model_in_body()` rewrites the request body's `model` field back to that canonical value before forwarding upstream — so the upstream always sees the exact case stored in `provider_models`.

### Route Table Refresh

Routes are rebuilt by `provider_svc::refresh_routes()`, which:

1. Iterates all provider definitions from `providers.json`
2. Loads DB overrides for each provider
3. Loads the model whitelist from `provider_models`
4. For each enabled channel of an enabled provider, inserts every model into the appropriate route table

Refresh triggers: on startup, on a periodic timer (configurable via `bridge.refresh_interval_min`), and after any provider update via the admin API.

### Key Design Decisions

- **In-memory route table.** Two `HashMap<String, ProviderRoute>` in `AppState` (one for OpenAI, one for Anthropic format). Keys are lowercased for case-insensitive lookup; `ProviderRoute` carries the canonical `model_id`/`model_name`. No DB lookup per request.
- **JSON definitions + DB overrides.** Provider metadata that rarely changes (name, channels) lives in `providers.json`. User-specific secrets and toggles (api_key, is_enabled, model whitelist) live in SQLite. The JSON file is the source of truth for provider identity; DB rows reference it by `provider_id`.
- **Two HTTP servers.** The proxy and admin servers run on separate ports (default 10010/10020) in a `tokio::select!` — if either exits, both shut down.
- **Proxy auth via API-key cache.** The proxy router is wrapped in `auth_middleware`. Client API keys (`mb-{uuid}`, created in the admin UI) are SHA-256 hashed and matched against `AppState.api_key_cache` (`HashMap<key_hash, api_key_id>`, enabled keys only). The cache is refreshed on startup, on the periodic timer, and after any api-key create/delete/toggle. A matched key injects `AuthenticatedKey(id)` into request extensions; `proxy.rs` threads that `api_key_id` into every `usage_records` insert. The admin API has no app-level auth (loopback-bound by default).
- **API key at rest.** The upstream provider API key (`provider_config.api_key`) is stored in plaintext. Client-facing `mb-xxx` keys are stored **twice** in `api_keys`: as a SHA-256 `key_hash` (used by `auth_middleware` for proxy auth) and, to support the "reveal key later" UI feature, as `api_key`. When `[database] encryption_key` is set (base64 of 32 bytes), `api_key` is encrypted at rest via AES-256-GCM (`crypto::seal`/`reveal`); decryption falls back to returning the raw value for legacy plaintext rows. Without a key configured, `api_key` is stored in plaintext (acceptable only while admin is loopback-bound).
- **SSE streaming support.** Upstream responses with `content-type: text/event-stream` are proxied as streaming byte streams via a tokio mpsc channel. Bytes are buffered and split on `\n` so usage is accumulated across all SSE `data:` frames (handles events split across chunk boundaries, and multiple events per chunk). Buffered responses are parsed for `usage` blocks (OpenAI and Anthropic formats) to extract token counts, including cache read/write tokens.
- **No tests.** The project currently has no unit or integration tests.

## Source Structure

| Path | Purpose |
|------|---------|
| `model-bridge.toml` | Runtime config: proxy/admin server addresses, DB path, refresh interval, providers file path |
| `providers.json` | Static provider definitions (id, name, icon, channels, models_endpoint) |
| `src/main.rs` | Entry point: config loading, DB init, route table init, background refresh spawn, dual-server launch |
| `src/config.rs` | CLI args (clap), TOML config parsing, `providers.json` loading, `ProviderDef`/`ChannelDef` types |
| `src/state.rs` | `AppState` (in-memory route tables, provider defs, DB pool, HTTP client), model list response types |
| `src/router/mod.rs` | Router assembly: proxy router (`/openai/`, `/anthropic/`), admin router (`/api/admin/`, SPA fallback), CORS |
| `src/router/proxy.rs` | Core proxy: model extraction, route lookup, upstream forwarding, SSE streaming, usage recording |
| `src/router/models_list.rs` | Returns cached model lists in OpenAI/Anthropic format from in-memory route tables |
| `src/router/admin.rs` | Admin HTTP handlers: provider CRUD, API key CRUD, stats queries |
| `src/admin/provider_svc.rs` | Provider business logic: JSON+DB merge, route refresh, `/models` endpoint probing, CRUD |
| `src/admin/stats_svc.rs` | Usage stats queries: overview, per-model, daily, hourly |
| `src/db/schema.rs` | SQLite migrations: `provider_config`, `provider_channel_config`, `provider_models`, `api_keys`, `usage_records` |
| `src/db/models.rs` | SQLx `FromRow` structs and merge-result DTOs (`ProviderDetail`, `ProviderSummary`, `ChannelDetail`) |
| `src/middleware/mod.rs` + `auth.rs` | API key auth: `auth_middleware` (wired onto the proxy router), `refresh_api_key_cache()`, `AuthenticatedKey` extension type |
| `web/src/` | Vue 3 SPA: `Dashboard.vue`, `Providers.vue`, `ApiKeys.vue`, `Logs.vue`. Naive UI components, ECharts charts |
| `web/dist/` | Built frontend assets; embedded into the Rust binary via `include_dir!` |

## Admin API Endpoints

- `GET /api/admin/providers` — list providers (merged JSON + DB)
- `GET /api/admin/providers/{id}` — single provider detail (includes api_key, models)
- `PUT /api/admin/providers/{id}` — update provider config (api_key, is_enabled, channel overrides, models)
- `GET /api/admin/providers/{id}/fetch-models?api_key=...` — probe upstream `/v1/models` endpoint using the supplied key (not the stored one), return `[{model_id, model_name}]`
- `POST /api/admin/providers/{id}/refresh` — force full route table refresh
- `GET /api/admin/api-keys` — list API keys (with masked preview, no full key)
- `POST /api/admin/api-keys` — create API key (returns full `mb-{uuid}` key once)
- `GET /api/admin/api-keys/{id}` — get full API key
- `PUT /api/admin/api-keys/{id}` — toggle API key enabled state
- `DELETE /api/admin/api-keys/{id}` — delete API key
- `GET /api/admin/stats/overview` — total requests, tokens, avg latency, errors (last 7 days)
- `GET /api/admin/stats/models` — per-model usage breakdown (last 7 days)
- `GET /api/admin/stats/daily?days=7` — daily usage for last N days
- `GET /api/admin/stats/hourly` — hourly token usage (last 7 days)
