# AGENTS.md: WordInk

WordInk is an open-source, provider-agnostic dictation platform. It's SDK first: a Rust sans-I/O core compiled to WebAssembly, a TypeScript browser host, a `<wordink-mic>` web component, a React hook, a local engine and a credential relay. A desktop app comes later. The plan of record is `docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md`, and the phases are in `ROADMAP.md`. Repo name `WordInk`, npm scope `@wordink`.

## Commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p wordink-wasm --target wasm32-unknown-unknown --release
pnpm install && pnpm typecheck && pnpm test
cd legacy && uv sync && uv run python -m compileall wordink   # legacy app
```

## Architecture invariants

- `wordink-core` does no I/O: no sockets, clocks, threads or filesystem. It emits effects and takes events. Hosts (browser TS now, native Rust later) perform I/O. Don't add I/O dependencies to the core.
- The core wasm has a 150 KB gzip budget (KTD1). Check size before adding a crate.
- Long-lived provider keys never go to the browser. `@wordink/server` fails closed until an `authorize` hook is supplied. Dev-key mode is localhost-only.
- No telemetry leaves the end user's device.
- `legacy/` is the original Python app, kept runnable until WordInk Desktop replaces it. Its data still lives in `~/.groq_flow/` (don't rename: that's user data).

## Workflow

- `master` is protected (PR + 1 review, squash-only). Work happens on branches, one PR per plan unit (U1-U9).
- Add a changeset (`pnpm changeset`) for any change to a published package.
- Don't hand-edit lockfiles (`Cargo.lock`, `pnpm-lock.yaml`, `legacy/uv.lock`).
