## What changed
<!-- One or two sentences. Name the plan unit (U1-U9) if this implements one. -->

## Why
<!-- The problem, not the solution. -->

## Checks
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` pass
- [ ] `pnpm typecheck` and `pnpm test` pass
- [ ] A changeset is added if a published package changed (`pnpm changeset`)

## If this touches key handling (`@wordink/server`, credentials, dev-key mode)
- [ ] No long-lived key can reach the browser (AE3)
- [ ] `authorize` still fails closed

## If this touches `legacy/` (the Python app)
- [ ] `cd legacy && uv sync && uv run python -m compileall wordink` passes
- [ ] Tested on Windows if it touches tray, boot or `setup*` scripts
