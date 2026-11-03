# WordInk Roadmap

WordInk now has **one desktop app** (`apps/desktop`, based on [Hex](https://github.com/anomalyco/hex), MIT) for Linux, Windows and macOS. The app foundation is merged on `master` (PR #111); its active implementation plan is `docs/plans/2026-11-02-0136-feat-wordink-one-app-plan.md`. The default HUD will merge Hex's status pill with Voxtype's audio-reactive meter in one popup. Earlier plans are in [`docs/archive/`](docs/archive/) or superseded.

This page lists themes, not tickets. "Done" means merged to `master`. "Planned" means not started or not merged yet.

## Direction

- **Groq by default.** Bring your own Groq key and dictation works. A local model needs no key: Parakeet v3 is the planned local default.
- **Open source, no telemetry, no WordInk servers.** MIT licensed.
- **Linux on any Wayland desktop** (Omarchy gets extras), **Windows**, and **macOS on Apple silicon**.
- **`wordink-dictd` stays** (Python, `sdks/python`) until the desktop app reaches parity on each OS. Then it is retired there.
- **Separate products, not merged into the app:** the web SDK (`packages/*`), the gateway (`packages/server`). The iOS app is paused.

## Now

- **Desktop app foundation.** Hex's MIT app is in `apps/desktop` with WordInk branding, Groq cloud transcription, local models, settings and indicator styles (PR #111; settings fixes #119 and #121). The current gap is the combined default HUD and real-device acceptance; no desktop release is tagged yet.
- **Repo hygiene.** The superseded `legacy/` app is removed, old plans are archived, and personal details were scrubbed (PR #110).
- **Open-source basics.** Third-party notices, security policy, DCO contribution guide, code of conduct, issue templates and CodeQL are merged (PR #109).

## Next

- **Parity with the interim daemon.** Text replacements, filler removal, media pause, more cloud providers, meeting mode, usage and history. Planned.
- **Cross-OS install.** Complete and verify install/update paths, then publish signed Linux releases; macOS and Windows remain unsigned until paid signing is available.
- **Retire `wordink-dictd` per OS** once the app matches it there, starting with Linux. Planned.
- **Publish the web SDK and gateway to npm.** The release workflow exists; publishing is switched off until an npm scope is set up. Planned.

## Later

- macOS notarization and Windows Authenticode signing (both need paid accounts). Until then macOS and Windows builds are unsigned and the docs say so.
- Live-API hardening for OpenAI and Deepgram in the web SDK, and a manual cross-browser handcheck.
- Rebuild the iOS app, lightweight (the earlier app is preserved on branch `archive/ios-2026-11-03`).
- Stronger local engines in the browser SDK.

## Platform support

| Platform | Desktop app (`apps/desktop`) | `wordink-dictd` today | Notes |
|---|---|---|---|
| Linux, Wayland (any desktop) | App code in `master`; device acceptance and release pending | Works; text insertion and binds are Hyprland-first (see `examples/omarchy-dictation`) | Omarchy smoke test remains; Linux release signing is planned |
| Windows | App code in `master`; device acceptance and release pending | Works (`examples/windows-dictation`) | Real-hardware acceptance remains; unsigned |
| macOS, Apple silicon | App code in `master`; device acceptance and release pending | Works; first-run verified on a real Mac (`examples/macos-dictation`) | Real-app acceptance remains; unsigned, not notarized |
| iOS | Paused | n/a | The earlier app is preserved on branch `archive/ios-2026-11-03` and will be rebuilt later |
| Web | Separate product (`packages/*`) | n/a | `<wordink-mic>` and React; not on npm yet |

## Model and provider support

| Engine | Where | State |
|---|---|---|
| Groq Whisper | `wordink-dictd`, gateway, web relay | Done. Default |
| Groq Whisper | Desktop app | Implemented in `apps/desktop`; default when a Groq key is set |
| Parakeet v3 (local, no key) | Desktop app | In the app's model path; intended no-key default |
| OpenAI transcription | Gateway, web relay | Done; live-API check still to do |
| Deepgram | Gateway, web relay | Done; live-API check still to do |
| More cloud providers | Desktop app | Planned |
| Moonshine (in the browser, offline) | `@wordink/local` | Done |

## Done so far

- **Foundation.** Monorepo tooling, CI, docs site with live demo.
- **Web SDK.** `@wordink/core` (Rust core as WebAssembly plus a TypeScript host), `<wordink-mic>`, `useDictation`, the credential relay.
- **Gateway.** `wordink-gateway`: OpenAI-compatible endpoint, provider fallback, per-device tokens, runtime vocabulary.
- **Python tooling.** `wordink` CLI and `wordink-mcp`; `wordink-dictd` on Linux, Windows and macOS.

Background: [`docs/research/2026-10-10-landscape.md`](docs/research/2026-10-10-landscape.md) (landscape), [`DESIGN.md`](DESIGN.md) (design direction).
