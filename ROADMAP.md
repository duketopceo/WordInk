# WordInk Roadmap

WordInk is one desktop app (`apps/desktop`, based on [Hex](https://github.com/anomalyco/hex) by Anomaly, MIT) for Linux, Windows and macOS. Version 1.0.0 is released (`desktop-v1.0.0`, 2026-11-03) for Linux x86_64 and aarch64, macOS arm64 and Windows x64. The 1.0 release plan, [`docs/plans/2026-11-02-1907-feat-wordink-1-0-release-plan.md`](docs/plans/2026-11-02-1907-feat-wordink-1-0-release-plan.md), is `status: done`. Earlier plans are in [`docs/archive/`](docs/archive/) or marked `status: done`.

This page lists themes, not tickets. "Done" means merged to `master`.

## Direction

- **Groq by default.** Bring your own Groq key and dictation works. A local Parakeet model needs no key.
- **Open source, no telemetry, no WordInk servers.** MIT licensed. The only opt-in is "Copy diagnostics"; download counts come from GitHub.
- **Linux on any Wayland desktop**, **Windows**, and **macOS on Apple silicon**.
- **`wordink-dictd` stays** (Python, `sdks/python`) until the desktop app is verified as its replacement on each OS.
- **Separate products, not merged into the app:** the web SDK (`packages/*`) and the gateway (`packages/server`).

## Next

- Make Parakeet v3 the installed local default (today it is Parakeet Unified).
- Find the roughly 9 s of app overhead on Groq requests.
- Pick the top three OpenRouter engines per use: dictation (Groq turbo, MAI-Transcribe 2, hosted Parakeet v3) and meetings (Nemotron 3.5, Voxtral Small 3, MAI-Transcribe 2).
- Show Groq quota in the app.
- Larger chunks for cloud meeting transcription.
- A Compare engines screen, building on `wordink compare`.
- **SDK:** publish the web SDK and gateway to npm (the workflow exists; publishing is off until the `@wordink` scope is set up), document the desktop app's local API, and add a small `@wordink/desktop` client so other apps can ask the running WordInk to dictate or transcribe. No native library. Plan: `docs/plans/2026-11-03-1800-feat-sdk-and-omarchy-plugin-plan.md`.
- **Omarchy bar plugin:** a WordInk mic icon in the Omarchy bar that shows state, toggles dictation on click, and opens a small panel (engine, mic picker, last dictations, weekly usage). Ships in the omarchy-plugins catalog, not inside the app. Same plan.

## Later

- Signed macOS (notarization) and Windows (Authenticode) builds. Both need paid accounts.
- Self-hosted release runners.
- A lightweight iOS rebuild. The earlier app is preserved on branch `archive/ios-2026-11-03`.
- Vocabulary generation during onboarding.
- Live-API hardening for OpenAI and Deepgram in the web SDK; stronger local engines in the browser SDK.

## Platform support

| Platform | Desktop app (`apps/desktop`) | Install | Signing and updates |
|---|---|---|---|
| Linux x86_64, aarch64 (Wayland; glibc 2.39+, OpenSSL 3) | 1.0.0 | `install.sh` one-liner | ed25519-verified; self-updating |
| macOS, Apple silicon | 1.0.0 | `install-macos.sh` one-liner (`WordInk.app` in `~/Applications`) | Unsigned, not notarized; updates are download-only |
| Windows x64 | 1.0.0 | `install.ps1` one-liner (per-user) | Unsigned (SmartScreen warns); updates are download-only |
| iOS | Paused | n/a | Preserved on `archive/ios-2026-11-03` |
| Web | Separate product (`packages/*`) | `<wordink-mic>` and React; not on npm yet | n/a |

`wordink-dictd` still works on Linux, Windows and macOS (`examples/*-dictation`).

## Engines

| Engine | Where | State |
|---|---|---|
| Groq `whisper-large-v3-turbo` | Desktop app, `wordink-dictd`, gateway, web relay | Done. Default when a Groq key is set |
| OpenRouter | Desktop app | Done. Audio-chat models and the transcription endpoint (29 models, including MAI-Transcribe 2 and Nemotron 3.5), with p50/p95 latency in Usage |
| Parakeet (local, no key) | Desktop app | Done. v3 is recommended; the installed default is Parakeet Unified until the switch. GPU through Vulkan on Linux |
| OpenAI transcription, Deepgram | Gateway, web relay | Done; live-API check still to do |
| Moonshine (in the browser, offline) | `@wordink/local` | Done |

## Done so far

- **Desktop app 1.0 line.** App merged (#111), terminal auto-paste (#126), usage stats (#123), vocabulary suggestions (#125), copy diagnostics and download counts (#132), Bluetooth earbuds and clean mic names (#134), shorter first run (#138), FLAC uploads and `wordink compare` (#143), developer-mode debug logs, coded errors and earbud reliability (#139), OpenRouter transcription engine and p50/p95 latency (#142), cross-OS installers and release workflow.
- **Foundation.** Monorepo tooling, CI, docs site with live demo, open-source basics.
- **Web SDK.** `@wordink/core`, `<wordink-mic>`, `useDictation`, the credential relay.
- **Gateway.** `wordink-gateway`: OpenAI-compatible endpoint, provider fallback, per-device tokens, runtime vocabulary.
- **Python tooling.** `wordink` CLI and `wordink-mcp`; `wordink-dictd`.

Background: [`docs/research/2026-10-10-landscape.md`](docs/research/2026-10-10-landscape.md), [`DESIGN.md`](DESIGN.md).
