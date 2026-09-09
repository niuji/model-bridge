# Repository Guidelines

## Project Structure & Module Organization

Model Bridge is a Rust 2021 LLM API gateway with a Vue 3/TypeScript admin UI.
- `src/router/` handles proxy and admin HTTP endpoints; `src/middleware/` handles authentication.
- `src/admin/` contains provider, balance, and statistics services; balance adapters live in `src/admin/balance_svc/`.
- `src/db/` contains SQLite models and schema migrations; `src/config.rs`, `src/state.rs`, and `src/crypto.rs` manage configuration, shared state, and encryption.
- `web/src/views/` contains UI pages; `web/public/` holds icons and static assets.
- `providers.json` defines embedded providers. `docs/superpowers/` holds design specs and implementation plans.

## Build, Test, and Development Commands

Use stable Rust and Node.js 20+. Run commands from the repository root unless noted.
- `cd web && npm ci && npm run build`: install frontend dependencies and generate `web/dist/`. Run before the first Rust build or test; Rust embeds this directory at compile time.
- `cargo build --release`: build the production binary.
- `cargo run`: start the proxy on port 10010 and admin server on port 10020 by default.
- `cd web && npm run dev`: start Vite on port 3000, proxying `/api` to the admin server.
- `cargo check`, `cargo clippy`, and `cargo test`: check compilation, lint, and run backend tests.

After frontend changes, rebuild assets, run `cargo clean -p model-bridge`, and rebuild Rust to avoid embedding stale assets.

## Coding Style & Naming Conventions

Follow surrounding code: four-space Rust indentation, snake_case functions/modules, and PascalCase types. Use rustfmt for Rust formatting without reformatting unrelated code. Frontend code uses two-space indentation, single quotes, no semicolons, and PascalCase Vue filenames. No frontend lint or formatting script is configured.

## Testing Guidelines

Use Rust `#[test]` and `#[tokio::test]` with descriptive snake_case names. Unit tests live alongside implementation; `src/router/proxy_route_tests.rs` uses Wiremock for HTTP behavior. Add regression tests for fixes and mock upstream services. Run focused tests with `cargo test <test_name> -- --nocapture`, then the full suite. No coverage threshold or frontend test runner is configured; validate UI changes with a production build and browser checks.

## Commit & Pull Request Guidelines

Follow existing scoped Conventional Commits, such as `feat(balance): ...`, `refactor(balance): ...`, and `fix(proxy): ...`. Keep changes focused. PRs should explain the behavior change, reason, and validation; link relevant issues and include screenshots for UI changes.

## Security & Configuration

Keep credentials and database files out of commits. Use `model-bridge.toml.example` as the configuration reference and `~/.mb/providers.json` for private providers. Keep the unauthenticated admin server bound to loopback.
