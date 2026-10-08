# WordInk

**Dictation as a component.** WordInk is an open-source, provider-agnostic dictation engine that turns speech into finished text. It ships as a drop-in SDK for any web app first, and later as a standalone dictation app for Linux, Omarchy, Windows and macOS.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

> **Status: pre-release.** The Phase 1 Web SDK is built and on its way to its first npm release. See [ROADMAP.md](ROADMAP.md) and the [Phase 1 plan](docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md). The original Windows dictation app still works and lives in [`legacy/`](legacy/).

**Docs and live demo: <https://duketopceo.github.io/WordInk/>**

## Quickstart

Plain HTML, no build step:

```html
<textarea id="message"></textarea>
<wordink-mic for="message" endpoint="/api/wordink"></wordink-mic>

<script
  type="module"
  src="https://cdn.jsdelivr.net/npm/@wordink/web@VERSION/dist/wordink-web.cdn.js"
  integrity="sha384-…"
  crossorigin="anonymous"
></script>
```

The [HTML quickstart](https://duketopceo.github.io/WordInk/quickstart-html.html) has the pinned version and its integrity hash. `endpoint` is your [`@wordink/server`](packages/server) relay, which holds the provider key.

React:

```tsx
import { WordInkMic } from "@wordink/react";

<textarea id="message" />
<WordInkMic htmlFor="message" endpoint="/api/wordink" />
```

- **Drop-in:** one element or one React hook, inserted at the cursor with native undo.
- **Bring your own key:** Groq (default), OpenAI or Deepgram through your own small relay (`@wordink/server`), so long-lived keys never reach the browser. Or use a local model (`@wordink/local`) that works offline.
- **No WordInk servers, no telemetry.** MIT licensed.

| Package | |
|---|---|
| [`@wordink/web`](packages/web) | The `<wordink-mic>` element (ES module and single-file CDN build) |
| [`@wordink/react`](packages/react) | `useDictation` and `<WordInkMic>` |
| [`@wordink/core`](packages/core) | The engine: Rust core as WebAssembly plus a TypeScript browser host |
| [`@wordink/local`](packages/local) | Offline speech recognition in the browser (Moonshine) |
| [`@wordink/server`](packages/server) | Fail-closed credential relay for Cloudflare Workers and Node |

## Repository layout

| Path | What |
|---|---|
| `crates/wordink-core` | Rust sans-I/O dictation core (sessions, audio pipeline, provider protocols) |
| `crates/wordink-wasm` | WebAssembly bindings for the core |
| `packages/*` | npm packages: `@wordink/core`, `web`, `react`, `local`, `server` |
| `apps/docs` | The docs site and live demo (deployed to GitHub Pages) |
| `examples/*` | Plain HTML, React and relay examples |
| `legacy/` | The original Python/Windows dictation app (groq_flow lineage) |
| `docs/plans/` | Product and implementation plans |

## Develop

```bash
cargo test --workspace                       # Rust core
pnpm install && pnpm typecheck && pnpm test  # TypeScript packages
pnpm --filter @wordink/docs dev               # docs site on localhost (offers a dev-key field)
```

## Releases and docs

Changesets drives releases: add one with `pnpm changeset`, and `.github/workflows/release.yml` opens a version PR, then publishes to npm on merge through npm trusted publishing (OIDC, with provenance; no npm token is stored). The one-time npm setup is described at the top of that workflow. `.github/workflows/docs.yml` deploys `apps/docs` to GitHub Pages; it needs Settings > Pages > Source set to "GitHub Actions" once.

## The legacy Windows app

```bash
cd legacy
uv sync
uv run wordink --onboard   # setup wizard at http://localhost:18981
uv run wordink             # tray daemon
```

## Credits

WordInk began as a fork of [groq_flow](https://github.com/ParthJain18/groq_flow) by Parth Jain (MIT). See [LICENSE](LICENSE).
