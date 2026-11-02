# WordInk Roadmap

WordInk is converging on **one desktop app** (`apps/desktop`, based on [Hex](https://github.com/kitlangton/Hex), MIT) that anyone can install on Linux, Windows or macOS, press a hotkey, see a recording indicator, and get their words typed into the focused app. The current plan is `docs/plans/2026-11-02-0136-feat-wordink-one-app-plan.md`; earlier plans are in [`docs/archive/`](docs/archive/) and are superseded.

This page lists themes, not tickets. "Done" means merged to `master`. "Planned" means not started or not merged yet.

## Direction

- **Groq by default.** Bring your own Groq key and dictation works. A local model needs no key: Parakeet v3 is the planned local default.
- **Open source, no telemetry, no WordInk servers.** MIT licensed.
- **Linux on any Wayland desktop** (Omarchy gets extras), **Windows**, and **macOS on Apple silicon**.
- **`wordink-dictd` stays** (Python, `sdks/python`) until the desktop app reaches parity on each OS. Then it is retired there.
- **Separate products, not merged into the app:** the web SDK (`packages/*`), the gateway (`packages/server`) and the iOS app (`apps/ios`, `sdks/ios`).

## Now

- **Desktop app foundation.** Bring Hex into `apps/desktop` with its MIT notice, rebrand it, add a Groq cloud engine as the default, and a choice of recording-indicator styles. Planned; arrives in a pull request.
- **Repo hygiene.** Remove the superseded `legacy/` app, archive old plans, keep personal and machine-specific details out of the repo, and make the docs describe the product as it is. In progress on `chore/repo-cleanup`.
- **Open-source basics.** Third-party notices, security policy, contributing guide with DCO sign-off, code of conduct, issue templates, CodeQL and Dependabot for every package root. Planned.

## Next

- **Parity with the interim daemon.** Text replacements, filler removal, media pause, more cloud providers, meeting mode, usage and history. Planned.
- **Cross-OS install.** A single installer path per OS, signed Linux releases with a verified updater. Planned.
- **Retire `wordink-dictd` per OS** once the app matches it there, starting with Linux. Planned.
- **Publish the web SDK and gateway to npm.** The release workflow exists; publishing is switched off until an npm scope is set up. Planned.

## Later

- macOS notarization and Windows Authenticode signing (both need paid accounts). Until then macOS and Windows builds are unsigned and the docs say so.
- Live-API hardening for OpenAI and Deepgram in the web SDK, and a manual cross-browser handcheck.
- Mobile beyond the current iOS app.
- Stronger local engines in the browser SDK.

## Platform support

| Platform | Desktop app (`apps/desktop`) | `wordink-dictd` today | Notes |
|---|---|---|---|
| Linux, Wayland (any desktop) | Planned | Works; text insertion and binds are Hyprland-first (see `examples/omarchy-dictation`) | App targets any Wayland desktop; Linux builds will be signed |
| Windows | Planned | Works (`examples/windows-dictation`) | Unsigned |
| macOS, Apple silicon | Planned | Works; first-run verified on a real Mac (`examples/macos-dictation`) | Unsigned, not notarized |
| iOS | Separate product (`apps/ios`) | n/a | Talks to a gateway you run; built from source |
| Web | Separate product (`packages/*`) | n/a | `<wordink-mic>` and React; not on npm yet |

## Model and provider support

| Engine | Where | State |
|---|---|---|
| Groq Whisper | `wordink-dictd`, gateway, web relay | Done. Default |
| Groq Whisper | Desktop app | Planned. Default |
| Parakeet v3 (local, no key) | Desktop app | Planned. Local default |
| OpenAI transcription | Gateway, web relay | Done; live-API check still to do |
| Deepgram | Gateway, web relay | Done; live-API check still to do |
| More cloud providers | Desktop app | Planned |
| Moonshine (in the browser, offline) | `@wordink/local` | Done |

## Done so far

- **Foundation.** Monorepo tooling, CI, docs site with live demo.
- **Web SDK.** `@wordink/core` (Rust core as WebAssembly plus a TypeScript host), `<wordink-mic>`, `useDictation`, the credential relay.
- **Gateway.** `wordink-gateway`: OpenAI-compatible endpoint, provider fallback, per-device tokens, runtime vocabulary.
- **Python tooling.** `wordink` CLI and `wordink-mcp`; `wordink-dictd` on Linux, Windows and macOS.
- **iOS app.** Hold-to-talk remote and keyboard through a gateway (in progress).

Background: [`docs/research/2026-10-10-landscape.md`](docs/research/2026-10-10-landscape.md) (landscape), [`DESIGN.md`](DESIGN.md) (design direction).
