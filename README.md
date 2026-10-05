# WordInk

**Dictation as a component.** WordInk is an open-source, provider-agnostic dictation engine that turns speech into finished text. It ships as a drop-in SDK for any web app first, and later as a standalone dictation app for Linux, Omarchy, Windows and macOS.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **Status: pre-release.** The SDK is being built now. See [ROADMAP.md](ROADMAP.md) and the [Phase 1 plan](docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md). The original Windows dictation app still works and lives in [`legacy/`](legacy/).

## What it will look like

```html
<textarea id="message"></textarea>
<wordink-mic for="message" provider="groq" endpoint="/api/wordink"></wordink-mic>
```

- **Drop-in:** one element or one React hook, inserted at the cursor with native undo.
- **Bring your own key:** Groq, OpenAI or Deepgram through your own small relay (`@wordink/server`), so long-lived keys never reach the browser. Or use a local model that works offline.
- **No WordInk servers, no telemetry.** MIT licensed.

## Repository layout

| Path | What |
|---|---|
| `crates/wordink-core` | Rust sans-I/O dictation core (sessions, audio pipeline, provider protocols) |
| `crates/wordink-wasm` | WebAssembly bindings for the core |
| `packages/*` | npm packages: `@wordink/core`, `web`, `react`, `local`, `server` (added per the plan) |
| `legacy/` | The original Python/Windows dictation app (groq_flow lineage) |
| `docs/plans/` | Product and implementation plans |

## Develop

```bash
cargo test --workspace                       # Rust core
pnpm install && pnpm typecheck && pnpm test  # TypeScript packages
```

## The legacy Windows app

```bash
cd legacy
uv sync
uv run wordink --onboard   # setup wizard at http://localhost:18981
uv run wordink             # tray daemon
```

## Credits

WordInk began as a fork of [groq_flow](https://github.com/ParthJain18/groq_flow) by Parth Jain (MIT). See [LICENSE](LICENSE).
