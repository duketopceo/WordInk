# WordInk Roadmap

**WordInk is dictation as a component.** It's one open-source, provider-agnostic core that turns speech into finished text. It ships as a drop-in SDK for any app, and it reaches the desktop by integrating with the dictation apps people already use (Phase 2).

- **Open source (MIT), bring your own key.** Use Groq, OpenAI or Deepgram with your own key, or a local model. No WordInk account, no WordInk servers.
- **SDK first.** The SDK is the product. The desktop dictation apps people already use are its flagship consumers.
- **Wayland is first-class.** The desktop app treats Linux/Hyprland as a primary platform, not an afterthought.

Detailed plans: [`docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md`](docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md) (Phase 1), [`docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md`](docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md) (Phase 2 gateway), [`docs/plans/2026-10-06-1603-feat-phase-1-closeout-plan.md`](docs/plans/2026-10-06-1603-feat-phase-1-closeout-plan.md) (closeout).

## Phases

| Phase | What ships | Status |
|---|---|---|
| **0. Foundation** | Repo cleanup: archive the legacy Python app under `legacy/`, monorepo tooling, CI, positioning | Done (PRs #16–#17) |
| **1. Core + Web SDK** | `@wordink/core` (provider-agnostic engine), `<wordink-mic>` web component, `useDictation` React hook, Groq/OpenAI/Deepgram + local engine, reference credential server, docs site with live demo | Done — merged to master; docs live on Pages; npm publish gated on org setup |
| **2. Desktop** | Integrate with existing apps: `wordink-gateway`, an OpenAI-compatible transcription endpoint with provider fallback and per-device tokens. Voxtype on Linux verified; TypeWhisper via its bundled OpenAI-compatible engine + `GET /v1/models` | Done — gateway + docs + `/v1/models` shipped |
| **3. WordInk Desktop: Windows + macOS** | Was contingent on Phase 2 picking a standalone app | Dropped — Phase 2 chose integration |
| **4. More SDKs** | Python tooling for agents and CLIs: `wordink` CLI + `wordink-mcp` MCP server over the gateway (`sdks/python`); then mobile (React Native / native) based on demand | Built |

### Phase 2 decision (Oct 2026)

A comparison after Phase 1 changed the picture:

- **TypeWhisper** (GPLv3; macOS, Windows, iOS; no Linux) already ships system-wide dictation with more engines than planned here, LLM cleanup presets, per-app/URL profiles, an HTTP API, a CLI and a plugin SDK with a marketplace.
- **Voxtype** (MIT; Linux only) already covers Wayland/Hyprland well: 7 local engines, GPU acceleration, remote Whisper, and a meeting mode.

A standalone WordInk Desktop would be a fourth app chasing two good, free, open-source ones. Neither of them is an embeddable SDK, and that is still WordInk's gap.

| Option | What it means | Trade-off |
|---|---|---|
| **A. Integrate (recommended)** | WordInk becomes a provider/relay that desktop apps point at. A TypeWhisper plugin (plugin SDK / HTTP API), and Voxtype's remote mode against `@wordink/server` (OpenAI-compatible transcription endpoint). | Small surface, rides existing user bases. WordInk stops being a desktop brand. |
| **B. Standalone Linux app** | The original Phase 2: a native host for the Rust core with portal shortcuts and virtual-keyboard injection. | Full control, but it competes head-on with Voxtype on its home turf. |
| **C. Skip desktop** | Put everything into the SDK: Python bindings, mobile, more providers, the live-API hardening below. | Tightest focus. It drops the desktop story entirely. |

**Decided: Option A.** The gateway shipped on `feat/p2-gateway` (PR #27), and Voxtype on omarchy-max already dictates through it with provider fallback. Phase 1 closeout is in flight under [`docs/plans/2026-10-06-1603-feat-phase-1-closeout-plan.md`](docs/plans/2026-10-06-1603-feat-phase-1-closeout-plan.md): the stacked PR chain, npm publish with trusted publishing, GitHub Pages, and the PR #26 residuals. Still deferred (no provider keys on this machine): live-API checks for OpenAI (GA session shape, `gpt-live-transcribe`, `ek_` auth) and Deepgram (`token` vs `bearer`), plus the manual cross-browser blur/permission handcheck.

### Phase 1 milestones (Core + Web SDK)

1. **Core engine:** the provider interface, audio capture, session state machine, Groq adapter.
2. **Web component + React hook:** `<wordink-mic>`, `useDictation`, text insertion with native undo, themeable.
3. **Providers + keys:** OpenAI and Deepgram adapters, streaming partials, reference credential server.
4. **Local engine:** in-browser model, works offline after first load.
5. **Docs + launch:** docs site, live demo, npm publish, launch post.

## Competitive landscape (Oct 2026)

Pricing comes mostly from aggregator sites. Treat it as approximate.

### Standalone dictation apps

| App | Linux | Windows | macOS | Engine | Price | Open source |
|---|---|---|---|---|---|---|
| Wispr Flow | No | Yes | Yes | Cloud + LLM cleanup | Free tier; ~$12-15/mo | No |
| Superwhisper | No | Yes | Yes | Local or cloud | ~$8.49/mo or $249.99 lifetime | No |
| TypeWhisper | No | Yes | Yes (+ iOS) | Local (WhisperKit, Parakeet, Qwen3, Granite, SpeechAnalyzer) + cloud (Groq, OpenAI, Deepgram, AssemblyAI, Cloudflare); HTTP API, CLI, plugin SDK | Free; Team €19/mo, Enterprise €99/mo | GPLv3 |
| Aqua Voice | No | Yes | Yes | Cloud (own model) | ~$8/mo | No |
| Willow Voice | No | Yes | Yes | Cloud | ~$12-15/mo | No |
| Typeless | No | Yes | Yes | Cloud | ~$12/mo | No |
| Monologue | No | ? | Yes | Cloud + offline | ~$12-15/mo | No |
| MacWhisper | No | No | Yes | Local Whisper | €59 lifetime | No |
| VoiceInk | No | No | Yes | Local | $25-49 lifetime | GPLv3 |
| Spokenly | Yes | Yes | Yes | Local + BYOK | Free local; $9.99/mo Pro | No |
| Handy | Partial Wayland | Yes | Yes | Local (Whisper, Parakeet) | Free | MIT |
| OpenWhispr | Yes (Wayland unclear) | Yes | Yes | Local or cloud + cleanup | Free | MIT |
| Voxtype | Wayland-native | No | No | Local (many engines) | Free | Yes |
| hyprwhspr | Wayland (Omarchy) | No | No | Local + cloud | Free | Yes |
| whisrs | Wayland + X11 | No | No | Groq/Deepgram/OpenAI + whisper.cpp | Free | MIT |
| nerd-dictation | Partial | No | No | Local Vosk | Free | Yes |
| Talon | Retreating | Yes | Yes | Local voice control | Free / paid beta | No |
| Built-in (Win+H, macOS Dictation) | No | Yes | Yes | On-device | Free | No |

### Embeddable dictation for developers

| Option | Type | Limitation WordInk addresses |
|---|---|---|
| Corti `@corti/dictation-web` | Web component | Tied to Corti's API |
| Suki Dictation SDK | JS/React, hosted iframe | Healthcare vendor account required |
| Kendo UI / Syncfusion SpeechToText | UI-suite components | Part of a paid suite |
| react-dictate-button and similar | React wrappers over Web Speech API | No Firefox; needs network; no provider choice |
| Wispr Flow Enterprise API | Hosted API | Closed and sales-gated |
| Deepgram, AssemblyAI, OpenAI, Groq, ElevenLabs, Speechmatics, Gladia | Raw speech APIs | You build capture, UX, insertion and key safety yourself |
| whisper.cpp, Vosk, transformers.js | Local engines | Engines, not drop-in dictation |

### Where WordInk fits

- **No open, vendor-neutral drop-in dictation component exists.** That's Phase 1.
- **Cross-platform apps with good Wayland support are rare.** Linux-first tools are Linux-only, and cross-platform tools have weak Wayland support. That's why Phase 2 integrates with them instead of building another app.
- **The open-source desktop field is crowded** (Handy, OpenWhispr, Voxtype), so WordInk reaches the desktop through those apps rather than competing with them — the gateway centralizes provider keys, fallback and vocabulary for any OpenAI-compatible client.
