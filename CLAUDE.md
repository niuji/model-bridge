# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Model Bridge is an LLM API proxy/gateway that routes client requests to upstream LLM providers. It supports both OpenAI-compatible and Anthropic-compatible API formats, transparently proxying requests while tracking usage metrics. A Vue.js admin UI manages providers, API keys, and usage statistics.

## Build & Run

```bash
# Build the Rust backend (release)
cargo build --release

# Run with default config
cargo run

# Run with custom config
cargo run -- --config /path/to/model-bridge.toml

# Build and run via Docker
docker build -t model-bridge .
docker run -p 8080:8080 -v $(pwd)/data:/data model-bridge

# Frontend dev server (for UI development)
cd web && npm install && npm run dev

# Frontend production build (output to web/dist/, embedded in binary via include_dir)
cd web && npm run build
```

After `npm run build`, rebuild the Rust binary to embed the updated frontend — the binary embeds the entire `web/dist/` directory at compile time via `include_dir!`.

## Architecture

### Request Flow

```
Client → /openai/v1/{*} or /anthropic/v1/{*} → proxy.rs
  1. GET /v1/models? → return cached model list from in-memory route table
  2. Other requests → extract "model" from JSON body → lookup in-memory route table → proxy to upstream provider
```

### Key Design Decisions

- **In-memory route table.** Provider model lists are probed via `/models` endpoints and cached in `AppState.openai_routes` / `AppState.anthropic_routes` (HashMap<String, ProviderRoute>). No DB lookup per request.
- **Periodic refresh.** A background task refreshes the route table every N minutes (configurable via `bridge.refresh_interval_min`). Routes are also refreshed on provider CRUD operations.
- **API key stored as plaintext.** Despite the `api_key_encrypted` column name, the API key is stored in plaintext in the SQLite DB. The frontend API key authentication uses SHA-256 hashing.
- **No frontend auth middleware applied.** `api_key_auth` middleware exists but is not wired into the router — admin endpoints are currently unauthenticated.
- **SSE streaming support.** Responses with `content-type: text/event-stream` are proxied as streaming byte streams (tokio mpsc channel), while buffered responses are parsed for token usage extraction.

### Source Structure

| Path | Purpose |
|------|---------|
| `src/main.rs` | Entry point: config loading, DB init, route table init, background refresh, server start |
| `src/config.rs` | CLI args and TOML config parsing (server host/port, DB path, refresh interval) |
| `src/state.rs` | `AppState` (in-memory route tables + DB pool), model list response types, deserialization types for upstream `/models` responses |
| `src/router/mod.rs` | Router assembly: mounts `/openai/`, `/anthropic/`, `/api/admin/` sub-routers, fallback to SPA HTML |
| `src/router/proxy.rs` | Core proxy logic: model extraction, route lookup, upstream forwarding, SSE streaming, usage recording |
| `src/router/models_list.rs` | Returns cached model lists in OpenAI/Anthropic format from in-memory route tables |
| `src/router/admin.rs` | Admin CRUD handlers for providers, API keys, and stats |
| `src/admin/provider_svc.rs` | Provider business logic: route refresh, `/models` probing, CRUD operations |
| `src/admin/stats_svc.rs` | Usage stats queries: overview, per-model, daily breakdown |
| `src/db/schema.rs` | SQLite schema migrations (providers, api_keys, usage_records tables) |
| `src/db/models.rs` | SQLx `FromRow` structs for DB rows |
| `src/middleware/auth.rs` | API key auth middleware (SHA-256 hash check) — defined but not wired in router |
| `web/src/` | Vue 3 SPA with Naive UI, Vue Router (hash mode), ECharts for stats charts |
| `web/dist/` | Built frontend assets; `index.html` is embedded into the Rust binary |

### Admin API Endpoints

- `GET/POST /api/admin/providers` — list/create providers
- `GET/PUT/DELETE /api/admin/providers/{id}` — CRUD single provider
- `POST /api/admin/providers/{id}/refresh` — force route refresh
- `GET/POST /api/admin/api-keys` — list/create API keys (key is returned once on creation as `mb-{uuid}`)
- `DELETE /api/admin/api-keys/{id}` — delete API key
- `GET /api/admin/stats/overview` — total requests, tokens, avg latency, errors
- `GET /api/admin/stats/models` — per-model usage breakdown
- `GET /api/admin/stats/daily?days=7` — daily usage for last N days