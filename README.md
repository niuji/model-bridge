# Model Bridge

[![Release](https://img.shields.io/github/v/release/niuji/model-bridge?style=flat)](https://github.com/niuji/model-bridge/releases)
[![CI](https://github.com/niuji/model-bridge/actions/workflows/release.yml/badge.svg)](https://github.com/niuji/model-bridge/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/built%20with-Rust-dea584?style=flat)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/github/license/niuji/model-bridge?style=flat)](LICENSE)

A self-hosted **LLM API gateway** that routes OpenAI- and Anthropic-compatible requests to multiple upstream providers (OpenAI, DeepSeek, MiniMax, Anthropic, …) behind a single set of client keys. Expose one endpoint to your apps, swap providers without touching clients, and track usage per model.

- **One endpoint, many providers.** Clients speak OpenAI or Anthropic protocol; Model Bridge looks the model up in its route table and forwards to the right upstream with the correct auth header.
- **Self-contained binary.** The Vue admin UI and the builtin provider list are embedded at compile time — download one file and run.
- **Client key management.** Issue `mb-...` gateway keys, hashed (SHA-256) for auth, optionally encrypted (AES-256-GCM) at rest.
- **Usage & cost visibility.** Per-model token counts (incl. cache read/write), request logs, daily/hourly charts in the dashboard.
- **SSE streaming** with cross-chunk usage extraction.
- **Case-insensitive model routing** — clients can send any case; the canonical id is forwarded upstream.
- **Hardened defaults.** Loopback-bound admin API, disabled HTTP redirects (SSRF), `http(s)`-only upstream base URLs, 64 MiB body limit, SQLite WAL.

## How it works

```
            ┌──────────────── model-bridge ────────────────┐
  client    │  proxy  :10010              admin  :10020    │
  ────────► │  /openai/v1/*    ─┐        (loopback only)   │
  mb- key   │  /anthropic/v1/* ─┤        /api/admin/* REST │
            │                   ▼        /          Vue   │
            │       route table (model → provider)        │
            │   ┌────────┬──────────┬──────────┬────────┐ │
            │   │ OpenAI │ DeepSeek │ MiniMax  │ Anthr. │ │
            │   └────────┴──────────┴──────────┴────────┘ │
            └──────────────────────────────────────────────┘
                            │
                            ▼
                 upstream LLM provider
```

Two HTTP servers run side by side: a **proxy** (port `10010`) your clients call, and an **admin** (port `10020`, loopback) where you configure providers, keys, and view stats. The route table is an in-memory `model → provider` map rebuilt on startup, on a timer, and after any config change.

## Quick start

### 1. Run the binary

Download the latest archive from [Releases](https://github.com/niuji/model-bridge/releases) — `model-bridge-linux-amd64.tar.gz` or `model-bridge-windows-amd64.zip` — extract, and run:

```bash
# Linux
tar -xzf model-bridge-linux-amd64.tar.gz
./model-bridge

# Windows: extract the zip, then double-click model-bridge.exe
```

No config file required — it starts with sensible defaults. To set an encryption key (recommended), copy the bundled template:

```bash
cp model-bridge.toml.example model-bridge.toml
# then set encryption_key to the output of:  openssl rand -base64 32
```

Logs confirm startup:

```
INFO  proxy service starting on 0.0.0.0:10010
INFO  admin service starting on 127.0.0.1:10020
```

### 2. Configure a provider

Open the admin UI at **http://127.0.0.1:10020** → **Providers**:

1. Pick a provider (e.g. DeepSeek), toggle it **enabled**.
2. Paste your **upstream API key** (the one the provider issued you).
3. Click **fetch models** to pull the model list from the upstream `/v1/models`, or add models manually.
4. Save.

### 3. Issue a client key

**API Keys** → **Create** → copy the `mb-...` key (shown once).

### 4. Call it

Point any OpenAI/Anthropic-compatible client at the proxy using your `mb-` key:

```bash
# OpenAI protocol
curl http://localhost:10010/openai/v1/chat/completions \
  -H "Authorization: Bearer mb-xxxxxxxx-..." \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}]}'
```

```bash
# Anthropic protocol
curl http://localhost:10010/anthropic/v1/messages \
  -H "x-api-key: mb-xxxxxxxx-..." \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet-4-6","max_tokens":256,"messages":[{"role":"user","content":"hi"}]}'
```

Or from the SDKs:

```python
from openai import OpenAI
client = OpenAI(base_url="http://localhost:10010/openai/v1", api_key="mb-...")
```

```python
from anthropic import Anthropic
client = Anthropic(base_url="http://localhost:10010/anthropic", api_key="mb-...")
```

The `mb-` key is accepted as either `Authorization: Bearer` or `x-api-key`; Model Bridge rewrites the request with the correct upstream auth header (Bearer for OpenAI-style, `x-api-key` for Anthropic-style) and rewrites the body's `model` to the canonical case stored for that route.

## Configuration

`model-bridge.toml` is optional; defaults are shown below. See [`model-bridge.toml.example`](model-bridge.toml.example) for a commented template.

| Field | Default | Description |
|---|---|---|
| `proxy.host` / `proxy.port` | `0.0.0.0` / `10010` | Proxy listen address. `0.0.0.0` = reachable on your LAN (a valid `mb-` key is still required). |
| `admin.host` / `admin.port` | `127.0.0.1` / `10020` | Admin listen address. **Keep loopback** — the admin API has no app-level auth. |
| `database.path` | `model-bridge.db` | SQLite database path. |
| `database.encryption_key` | `""` | base64 of 32 bytes; encrypts client `mb-` keys at rest (AES-256-GCM). Empty = plaintext (only safe while admin is loopback). Generate with `openssl rand -base64 32`. |
| `bridge.refresh_interval_min` | `10` | How often the route table and API-key cache refresh. |
| `providers_file` | `providers.json` | Provider definitions file. If missing on disk, the embedded builtin set is used. |

### Client key encryption

Without `encryption_key`, client `mb-` keys are stored in plaintext (acceptable only while the admin server is loopback-bound). Set it to seal keys with AES-256-GCM. Decryption falls back to the raw value for legacy rows, so enabling it needs no migration; **rotating** the key invalidates previously-sealed keys (re-issue them via the UI). Proxy auth uses a SHA-256 hash, not the encrypted column, so auth is unaffected.

## Providers

Providers are defined in three layers, merged at startup:

1. **Builtin `providers.json`** — id, name, channels, models endpoint. Embedded in the binary. Ships with **OpenAI, DeepSeek, MiniMax, Anthropic**.
2. **`~/.mb/providers.json`** — same schema; a matching `id` overrides the builtin, a new `id` is appended. Use this for private or corporate providers without forking the repo.
3. **SQLite** (runtime overrides, via the admin UI) — upstream API key, enabled state, per-channel base URL, model whitelist.

A provider can declare multiple **channels** of different protocol types (`openai_chat`, `openai_responses`, `anthropic`). A model under a provider with both an `openai_*` and an `anthropic` channel appears in **both** route tables, routing to different upstream base URLs per protocol. Non-`claude`/`anthropic` models reachable via an Anthropic channel are also exposed with a `claude-` prefix, so Claude-Code-style clients can address them.

## Admin API

Served on the loopback admin server at `/api/admin/`:

| Method | Path | Purpose |
|---|---|---|
| GET | `/providers` | list merged providers |
| GET / PUT | `/providers/{id}` | get / update a provider (api_key, enabled, channels, models) |
| GET | `/providers/{id}/fetch-models?api_key=...` | probe upstream `/v1/models` with the given key |
| POST | `/providers/{id}/refresh` | force a full route table refresh |
| GET / POST | `/api-keys` | list / create client keys |
| GET / PUT / DELETE | `/api-keys/{id}` | reveal / toggle / delete a key |
| GET | `/logs` | request logs (paginated) |
| GET | `/stats/overview` · `/stats/models` · `/stats/daily` · `/stats/hourly` | usage statistics |

## Build from source

Requires Rust (stable) and Node 20+.

```bash
# frontend — embedded into the binary at compile time via include_dir!
cd web && npm ci && npm run build && cd ..

# backend
cargo build --release      # → target/release/model-bridge
```

Rebuild the Rust binary after any frontend change so the updated `web/dist/` is embedded.

## Security notes

- **Admin API is unauthenticated.** It is bound to `127.0.0.1` by default. Do **not** set `admin.host` to a public address unless a trusted reverse proxy with auth sits in front — doing so logs a startup warning.
- **Client `mb-` keys** are SHA-256 hashed for proxy auth. The reveal copy is AES-256-GCM encrypted at rest when `encryption_key` is set.
- **Upstream provider keys** are stored in plaintext (used directly to forward requests). Protect the database file accordingly.
- **SSRF**: HTTP redirects are disabled and upstream base URLs are restricted to `http(s)`.
- **Body limit**: 64 MiB per request.

## Releases

Prebuilt static binaries are published for `x86_64-unknown-linux-musl` (no dynamic deps) and `x86_64-pc-windows-msvc`. Each archive bundles the binary plus `model-bridge.toml.example`. Pushing a `v*` tag triggers the release workflow; manual dispatch from the Actions tab runs the build only.

## License

Licensed under the [MIT License](LICENSE).
