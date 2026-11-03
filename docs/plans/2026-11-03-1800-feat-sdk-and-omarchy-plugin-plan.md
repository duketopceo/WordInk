---
title: WordInk SDK and Omarchy bar plugin - Plan
type: feat
date: 2026-11-03
topic: sdk-and-omarchy-plugin
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-plan-bootstrap
execution: code
status: planned
---

# WordInk SDK and Omarchy bar plugin - Plan

## Goal Capsule

- **Objective:** let other people use WordInk from their own code in two small ways, and give Omarchy users a WordInk control in their bar.
  1. **SDK:** publish the existing web SDK to npm, and document the desktop app's local API with a tiny client, so another app can ask the running WordInk to dictate or transcribe.
  2. **Omarchy plugin:** a small bar widget that shows WordInk's state and gives one-click control.
- **Means:** reuse what exists. The web SDK (`packages/*`) and its publish workflow are already built. The desktop app already serves an owner-only, bearer-authenticated local API on loopback. The plugin follows the shape of the existing plugins in the separate omarchy-plugins repo.
- **Not building:** a native library (Rust, C, Swift) for embedding WordInk in other apps. It would need a stable ABI and builds on three OSes, and the local API gives app builders the same result.

## Product Contract

### SDK

- R1. `@wordink/core`, `@wordink/web`, `@wordink/react` and `@wordink/local` are installable from npm. `@wordink/server` (the gateway) publishes with them, unchanged.
- R2. A public doc, `apps/desktop/docs/local-api.md`, explains how another program on the same machine, run by the same user, talks to the running WordInk:
  - finding it (the owner-only `local-api.json` discovery file: URL, token, API version);
  - authentication (`Authorization: Bearer <token>`);
  - the endpoints meant for outside use: `GET /health`, `GET /capabilities`, `POST /transcriptions` (send audio, get text), `POST /dictations` and its `finish` / `cancel` / `levels` steps (have WordInk record and transcribe), `POST /control` (the same commands as the CLI);
  - which endpoints are internal and may change (`/dev/*`, `/profile/*`, `/streams`).
- R3. A small JavaScript client, `@wordink/desktop`, wraps the documented endpoints: `connect()` reads the discovery file, then `transcribe(audio)`, `dictate()` and `control(command)`. It has no dependencies beyond Node's standard library.
- R4. The API is versioned. A breaking change bumps `apiVersion`, and the client refuses a version it does not know rather than guessing.
- R5. Security does not change: loopback only, owner-only discovery file, a bearer token on every request. The doc says plainly that any program running as the same user can use the API, and that no other user and no other machine can.

### Omarchy plugin

- R6. A bar widget, `io.github.duketopceo.wordink`, lives in the omarchy-plugins repo with its own installable repo, like the other plugins.
- R7. The bar shows a mic icon whose look follows WordInk's state: not running, idle, recording, transcribing, error. It does not draw a second recording overlay; WordInk's own indicator stays the one on-screen pill.
- R8. Clicking the icon toggles a dictation (`wordink dictation toggle`). A right-click or the panel opens the rest.
- R9. The panel shows:
  - the engine in use and the local fallback model;
  - a microphone picker (`wordink devices`);
  - the last three dictations, each with a copy button;
  - this week's words and cost (`wordink usage`);
  - buttons: Open WordInk, Settings, Copy diagnostics.
- R10. When WordInk is not installed, the panel shows the one-line install command, and nothing else breaks.
- R11. The widget only reads from and talks to the local `wordink` CLI. It stores no keys and makes no network calls.

### Acceptance examples

- AE1. A web developer runs `npm install @wordink/web`, adds `<wordink-mic>`, and it works against a Groq key or the gateway, as the package README describes.
- AE2. A Node script uses `@wordink/desktop` to transcribe a WAV file through the running WordInk with no API key of its own.
- AE3. On Omarchy, clicking the bar icon starts a dictation and the icon turns to recording; clicking again stops it and the text pastes.
- AE4. With WordInk quit, the icon shows "not running" within a few seconds and clicking it starts WordInk.

## Key Technical Decisions

- KTD1. **Publish the web SDK as it is.** The packages and `release.yml` (Changesets, npm trusted publishing with provenance, no stored token) already exist and are gated on `vars.NPM_PUBLISH_ENABLED`. The work is the owner's one-time npm setup plus a changeset; no code changes.
- KTD2. **The desktop SDK is a documented HTTP API plus a thin client, not a library.** Other languages can use the doc with curl or any HTTP client. Python gets a helper later only if someone asks.
- KTD3. **The plugin uses the CLI, not raw HTTP.** `wordink ... --json` already handles discovery, auth and versioning, and Quickshell `Process` is how the other plugins talk to their tools.
- KTD4. **State updates from the event stream, with polling as the fallback.** The widget follows `wordink logs -f` (the `live.ndjson` event stream) for state changes. When WordInk is not running, it checks `wordink status --json` every 5 s, so an idle bar costs almost nothing.
- KTD5. **The plugin ships separately from WordInk.** It is listed in the omarchy-plugins catalog and in its own `omarchy-wordink` repo, not bundled into the app's installer. The WordInk README links to it under Linux.

## Implementation Units

### U1. Publish the web SDK (owner actions + one changeset)

- Owner, once on npmjs.com:
  - create the `wordink` organization;
  - publish a first version of each package by hand, or use npm's placeholder flow;
  - add the GitHub Actions trusted publisher (`release.yml`) to each package;
  - require 2FA and disallow tokens on each package.

  The steps are listed at the top of `release.yml`. If the scope is taken, rename to the unscoped fallbacks.
- Agent:
  - add a changeset;
  - check each package README's install line and that `files` / `exports` cover the built output (`npm pack --dry-run`);
  - after the owner sets `NPM_PUBLISH_ENABLED=true`, let the Version PR merge and confirm the packages are on npm.
- Verify: AE1 on a clean project.

### U2. Local API doc

- Write `apps/desktop/docs/local-api.md` from `src/local_api.rs` and `docs/cli.md`, covering R2, R4 and R5. Link it from the desktop README and the root README's "Build on WordInk" line.
- Add a test that fails when a documented public route is removed from the router, so the doc can't silently drift.

### U3. `@wordink/desktop` client

- New package `packages/desktop`: about 200 lines of TypeScript.
  - `connect()` finds the discovery file in the per-OS data directory and checks `apiVersion`.
  - `transcribe()`, `dictate()` and `control()` wrap the documented endpoints.
  - Errors are typed: not running, unauthorized, unsupported version.
- Unit tests run against a fake local server. No live engine calls.
- Example: `examples/desktop-node/transcribe.mjs`.
- It publishes through the same Changesets flow as U1.

### U4. Omarchy bar plugin (in omarchy-plugins)

- `plugins/io.github.duketopceo.wordink/`: `manifest.json` (kind `bar-widget`, category Productivity), `Panel.qml`, `bin/` helper only if a CLI call needs reshaping, `README.md`, `AGENTS.md`, `LICENSE` (MIT), `preview.png`.
- Build it from the nordvpn plugin's structure and toast patterns. Cover R7–R11 with the CLI calls listed in R9 and KTD4.
- Add a catalog entry, pass `python scripts/validate-manifests.py` and `omarchy plugin validate`, install with `./scripts/install.sh --link`, and test AE3 and AE4 on a real Omarchy machine.
- Publish the standalone `omarchy-wordink` repo through the repo's usual release flow.

### U5. Docs and roadmap

- ROADMAP: move these items from Next to Done as they land.
- Root README: a short "Build on WordInk" section (npm packages, local API) and the Omarchy plugin under Linux install.

## Verification

- The `packages/*` tests and `pnpm build` pass; `npm pack --dry-run` lists the expected files for every package.
- The desktop tests pass, including the new route-versus-doc test.
- The plugin passes manifest validation, plus the manual AE3 and AE4 check on Omarchy.

## Owner actions

- npm: create the `wordink` org, publish the first versions or placeholders, set up trusted publishing, then run `gh variable set NPM_PUBLISH_ENABLED --body true -R duketopceo/WordInk`.
- Create the `omarchy-wordink` repo with `gh-new-repo` when U4 is ready to publish.
