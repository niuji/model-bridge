# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Model Bridge is an LLM API proxy/gateway that routes client requests to upstream LLM providers. It exposes two HTTP servers:

- **Proxy server** (default `:10010`): accepts OpenAI-compatible and Anthropic-compatible API requests at `/openai/v1/{*}` and `/anthropic/v1/{*}`, looks up the target model in an in-memory route table, and proxies the request to the correct upstream provider with the correct auth header format.
- **Admin server** (default `:10020`): serves a Vue 3 SPA management UI and a REST admin API at `/api/admin/` for managing provider credentials, API keys, and usage statistics.

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

### Docker

```bash
docker build -t model-bridge .
docker run -p 10010:10010 -p 10020:10020 -v $(pwd)/data:/data model-bridge
```

Note: the Dockerfile does not copy `providers.json` into the image — mount it or bake it in if customizing.

## Architecture

### Provider Configuration: JSON Definitions + DB Overrides

Providers are defined in two layers that merge at runtime:

1. **`providers.json`** (static definition) — id, name, icon, `models_endpoint` (optional), and `channels` array (each with `type` and `base_url`). These are loaded once at startup into `AppState.provider_defs`.
2. **SQLite tables** (user overrides) — `provider_config` (api_key, is_enabled), `provider_channel_config` (per-channel base_url override, is_enabled), `provider_models` (model whitelist).

The merge happens in `provider_svc::merge_channels()` and `list_providers()`/`get_provider()`. User overrides take precedence over JSON defaults. The model list stored in `provider_models` is what gets routed — it is populated either manually via the admin UI or fetched from the provider's `/v1/models` endpoint via the `fetch-models` button.

### Multi-Channel Routing

Each provider can declare multiple channels with different protocol types. A channel's `type` determines which route table the model maps to:

- `"anthropic"` → models go to `anthropic_routes` (served at `/anthropic/v1/{*}`)
- Everything else (`"openai_chat"`, `"openai_responses"`, etc.) → models go to `openai_routes` (served at `/openai/v1/{*}`)

A model listed under a provider with both an `openai_chat` and an `anthropic` channel will appear in BOTH route tables, routing to different base URLs per channel.

### Request Flow

```
Client → Proxy server (:10010) → proxy.rs handler
  1. GET /v1/models → return cached model list from in-memory route table
  2. POST /v1/chat/completions etc. → extract "model" from JSON body → lookup in-memory route table → proxy to upstream provider with appropriate auth headers
```

Auth header format is set per API format: Bearer token for OpenAI, `x-api-key` for Anthropic.

### Route Table Refresh

Routes are rebuilt by `provider_svc::refresh_routes()`, which:

1. Iterates all provider definitions from `providers.json`
2. Loads DB overrides for each provider
3. Loads the model whitelist from `provider_models`
4. For each enabled channel of an enabled provider, inserts every model into the appropriate route table

Refresh triggers: on startup, on a periodic timer (configurable via `bridge.refresh_interval_min`), and after any provider update via the admin API.

### Key Design Decisions

- **In-memory route table.** Two `HashMap<String, ProviderRoute>` in `AppState` (one for OpenAI, one for Anthropic format). No DB lookup per request.
- **JSON definitions + DB overrides.** Provider metadata that rarely changes (name, channels) lives in `providers.json`. User-specific secrets and toggles (api_key, is_enabled, model whitelist) live in SQLite. The JSON file is the source of truth for provider identity; DB rows reference it by `provider_id`.
- **Two HTTP servers.** The proxy and admin servers run on separate ports (default 10010/10020) in a `tokio::select!` — if either exits, both shut down.
- **API key stored as plaintext.** The `api_key` column in `provider_config` stores the upstream provider API key in plaintext. The frontend-facing API keys (for authenticating admin access) use SHA-256 hashing, but the auth middleware is **not wired into the router** — admin endpoints are currently unauthenticated.
- **SSE streaming support.** Upstream responses with `content-type: text/event-stream` are proxied as streaming byte streams via a tokio mpsc channel. Usage metrics are extracted from the final SSE data frame at stream end. Buffered responses are parsed for `usage` blocks (OpenAI and Anthropic formats) to extract token counts, including cache read/write tokens.
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
| `src/middleware/auth.rs` | API key auth middleware (SHA-256 hash check) — defined but not wired into the router |
| `web/src/` | Vue 3 SPA: `Dashboard.vue`, `Providers.vue`, `ApiKeys.vue`, `Logs.vue`. Naive UI components, ECharts charts |
| `web/dist/` | Built frontend assets; embedded into the Rust binary via `include_dir!` |

## Admin API Endpoints

- `GET /api/admin/providers` — list providers (merged JSON + DB)
- `GET /api/admin/providers/{id}` — single provider detail (includes api_key, models)
- `PUT /api/admin/providers/{id}` — update provider config (api_key, is_enabled, channel overrides, models)
- `GET /api/admin/providers/{id}/fetch-models` — probe upstream `/v1/models` endpoint, return model IDs
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
