# WordInk Roadmap

**WordInk is dictation as a component.** It's one open-source, provider-agnostic core that turns speech into finished text. It ships as a drop-in SDK for any app. How it reaches the desktop (integrations or a standalone app) is the open Phase 2 decision.

- **Open source (MIT), bring your own key.** Use Groq, OpenAI or Deepgram with your own key, or a local model. No WordInk account, no WordInk servers.
- **SDK first.** The SDK is the product. The desktop app is its flagship consumer.
- **Wayland is first-class.** The desktop app treats Linux/Hyprland as a primary platform, not an afterthought.

Detailed plan for the current phase: [`docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md`](docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md).

## Phases

| Phase | What ships | Status |
|---|---|---|
| **0. Foundation** | Repo cleanup: archive the legacy Python app under `legacy/`, monorepo tooling, CI, positioning | Done (PRs #16–#17) |
| **1. Core + Web SDK** | `@wordink/core` (provider-agnostic engine), `<wordink-mic>` web component, `useDictation` React hook, Groq/OpenAI/Deepgram + local engine, reference credential server, docs site with live demo | Built, in review (PRs #18–#26); not yet published |
| **2. Desktop** | **Under review (see below).** Originally a standalone Linux app; now leaning toward integrating with existing desktop apps instead | Decision pending |
| **3. WordInk Desktop: Windows + macOS** | Only if Phase 2 picks the standalone app | On hold |
| **4. More SDKs** | Python SDK for agents and CLIs; then mobile (React Native / native) based on demand | Exploring |

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

The open Phase 1 items to close before any of these: live-API checks for OpenAI (GA session shape, `gpt-live-transcribe`, `ek_` auth) and Deepgram (`token` vs `bearer`), the npm org plus trusted publishing, GitHub Pages, and the residuals listed in PR #26.

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
- **Cross-platform apps with good Wayland support are rare.** Linux-first tools are Linux-only, and cross-platform tools have weak Wayland support. That's Phase 2.
- **The open-source desktop field is crowded** (Handy, OpenWhispr), so the app competes on Wayland quality, SDK parity and BYOK freedom, not on features alone.
