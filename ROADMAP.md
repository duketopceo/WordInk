# WordInk Roadmap

**WordInk is dictation as a component.** It's one open-source, provider-agnostic core that turns speech into finished text. It ships as a drop-in SDK for any app, and as a standalone dictation app for Linux, Omarchy, Windows and macOS.

- **Open source (MIT), bring your own key.** Use Groq, OpenAI or Deepgram with your own key, or a local model. No WordInk account, no WordInk servers.
- **SDK first.** The SDK is the product. The desktop app is its flagship consumer.
- **Wayland is first-class.** The desktop app treats Linux/Hyprland as a primary platform, not an afterthought.

Detailed plan for the current phase: [`docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md`](docs/plans/2026-10-04-1757-feat-wordink-web-sdk-plan.md).

## Phases

| Phase | What ships | Status |
|---|---|---|
| **0. Foundation** | Repo cleanup: archive the legacy Python app under `legacy/`, monorepo tooling, CI, positioning | Next |
| **1. Core + Web SDK** | `@wordink/core` (provider-agnostic engine), `<wordink-mic>` web component, `useDictation` React hook, Groq/OpenAI/Deepgram + local engine, reference credential server, docs site with live demo | Planned |
| **2. WordInk Desktop: Linux** | System-wide hold-to-talk dictation on Wayland (Hyprland/Omarchy first, then GNOME/KDE) and X11. Portal global shortcuts, virtual-keyboard injection with clipboard fallback, tray, optional LLM cleanup. AUR and Omarchy packages | Planned |
| **3. WordInk Desktop: Windows + macOS** | The same app on Windows (replacing today's Python daemon) and macOS (accessibility-permission flow, signed builds) | Planned |
| **4. More SDKs** | Python SDK for agents and CLIs; then mobile (React Native / native) based on demand | Exploring |

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
