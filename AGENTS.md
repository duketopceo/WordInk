# AGENTS.md: WordInk

WordInk is open-source dictation. The direction is **one desktop app**, `apps/desktop`, based on Hex (MIT) and released as 1.0.0 (`desktop-v1.0.0`, 2026-11-03). It uses Groq by default and local Parakeet with no key, and targets Linux on any Wayland desktop, Windows, and macOS on Apple silicon. The 1.0 release plan (`docs/plans/2026-11-02-1907-feat-wordink-1-0-release-plan.md`) is done; finished plans carry `status: done` and `docs/archive/` holds superseded ones. Phases and status are in `ROADMAP.md`.

Until the app reaches parity on each OS, the Python `wordink-dictd` (`sdks/python`) is the working desktop client. Products are separate and stay separate: the web SDK (`packages/core`, `web`, `react`, `local`), the gateway (`packages/server`). The iOS app is paused (preserved on branch `archive/ios-2026-11-03`). Repo name `WordInk`, npm scope `@wordink`.

## Commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p wordink-wasm --target wasm32-unknown-unknown --release
pnpm install && pnpm typecheck && pnpm test     # build packages first if imports fail: pnpm --filter "./packages/*" build
cd sdks/python && uv run pytest                 # wordink-dictd and the Python SDK
scripts/scrub-check.sh <expressions-file>       # private strings must not appear in the tree
```

## Architecture invariants

- `wordink-core` does no I/O: no sockets, clocks, threads or filesystem. It emits effects and takes events. Hosts (the browser TS host) perform I/O. Don't add I/O dependencies to the core.
- The core wasm has a 150 KB gzip budget. Check size before adding a crate.
- Long-lived provider keys never go to the browser. `@wordink/server` fails closed until an `authorize` hook is supplied. Dev-key mode is localhost-only.
- No telemetry leaves the end user's device.
- Shared behavior fixtures live in `tests/fixtures/` and are checked by the TypeScript, Python and Rust tests. Change a fixture and its tests together.
- No personal or machine-specific details in the repo: no real hostnames, home paths, personal emails or private tool names. Use `example.com`, `Alex`, `~`.
- TypeWhisper is GPL-3.0: use its ideas only, never its code, strings or assets.

## Workflow

- `master` is protected (PR + 1 review, squash-only). Work happens on branches, one PR per plan unit.
- Add a changeset (`pnpm changeset`) for any change to a published package.
- Don't hand-edit lockfiles (`Cargo.lock`, `pnpm-lock.yaml`, `uv.lock`).
