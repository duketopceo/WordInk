# WordInk landscape research (2026-10-10)

Scope: who else does dictation, what users expect, and what is unique about WordInk. Sources are vendor pages and repositories fetched on 2026-10-10. Pricing from aggregators is approximate and marked as such. A claim without a link is a claim about this repository, verified from its files.

## What WordInk is

An embeddable, open-source (MIT), provider-agnostic dictation engine: Rust core compiled to WebAssembly (34.7 KB gzip, `pnpm test` size check), `<wordink-mic>` web component, React hook, in-browser local engine (Moonshine), a credential relay (`@wordink/server`) and an OpenAI-compatible gateway so desktop apps can use it as a provider. It also has a Python CLI and MCP server (`sdks/python`). Nothing is published to npm yet (`npm view @wordink/web` returns 404).

## Comparison

| Project | Type | Platforms | Engine | Cost | Open source | Source |
|---|---|---|---|---|---|---|
| Wispr Flow | Dictation app | Mac, Windows, iPhone, Android; no Linux found | Cloud, needs internet | Free ~2,000 words/week; Pro $15/mo or $144/yr (aggregators) | No | [pricing roundup](https://www.getvoibe.com/resources/wispr-flow-pricing/) |
| Superwhisper | Dictation app | Mac, Windows, iOS, Android | Local (best on Apple silicon) or cloud; BYO API keys on Pro | Free tier; Pro price not readable from the fetched page | No | [superwhisper.com](https://superwhisper.com) |
| MacWhisper | Transcription + dictation | Mac (iOS/iPad version mentioned) | Local models; BYO keys for OpenAI, Anthropic, Gemini, Deepgram | Free; Pro one-time EUR 64 | No | [macwhisper.com](https://www.macwhisper.com) |
| Talon | Hands-free input | macOS, Windows, Linux (X11 only on its page) | Voice commands and dictation | Free; Talon+ via Patreon | No (EULA) | [talonvoice.com](https://talonvoice.com) |
| nerd-dictation | CLI script | Desktop Linux | VOSK, offline | Free | Yes (GPL-3.0) | [GitHub](https://github.com/ideasman42/nerd-dictation) |
| whisper.cpp | Inference library | macOS, iOS, Android, Linux, Windows, WebAssembly, more | Whisper in C/C++; Metal, CUDA, Vulkan, OpenVINO and others; quantization | Free | Yes (MIT) | [README](https://github.com/ggml-org/whisper.cpp) |
| Voxtype | Dictation app | Linux (Wayland first, X11) and macOS | Local: Whisper, Parakeet, Moonshine, SenseVoice and others; optional remote Whisper servers; Ollama post-processing | Free | Yes (MIT) | [voxtype.io](https://voxtype.io) |
| Groq speech API | Hosted API | Any (HTTP) | `whisper-large-v3-turbo`, `whisper-large-v3`; OpenAI-compatible endpoints | $0.04/hour (turbo), about $0.111/hour (v3); 10 s billing minimum | No | [Groq docs](https://console.groq.com/docs/speech-to-text) |
| OpenAI speech API | Hosted API | Any (HTTP) | `gpt-transcribe`, `gpt-4o-transcribe`, `gpt-4o-mini-transcribe`, `whisper-1`; 25 MB file limit; streaming for completed files; Realtime for live audio | Per OpenAI pricing | No | [OpenAI docs](https://developers.openai.com/api/docs/guides/speech-to-text) |

The repository's own `ROADMAP.md` also lists TypeWhisper, Handy, OpenWhispr, hyprwhspr, whisrs and others with pricing; those rows were not re-verified here.

## What they do better

- **System-wide dictation.** Wispr Flow, Superwhisper, MacWhisper and Voxtype type into any app; WordInk's SDK only inserts into a field on a page that embeds it (desktop is reached through the gateway and other apps).
- **Cleanup and modes.** Superwhisper offers modes (Voice, Message, Email) and custom prompts; Wispr Flow describes cleaned-up text; Voxtype offers optional LLM post-processing. WordInk exposes a `transform` hook but ships no presets.
- **Local engines on the desktop.** Voxtype and whisper.cpp support GPUs and many models. WordInk's local engine runs in the browser only.
- **Maturity and distribution.** The commercial apps are shipped products. WordInk has no npm release yet.

## What WordInk does better

- **Embeddable and vendor-neutral.** None of the apps above is a component a web developer can embed. The hosted options are APIs where you build capture, UX, insertion and key safety yourself (Groq and OpenAI docs above describe file/stream endpoints, not UI).
- **Key safety by design.** The relay fails closed and keeps provider keys off the browser; the gateway uses per-device revocable tokens.
- **One contract for many clients.** The gateway speaks the OpenAI transcription format, which Groq also offers, so existing apps with a custom-endpoint setting can use provider fallback without changes.
- **Small and open.** MIT; core wasm is 34.7 KB gzip; no telemetry.

## What users expect

From the products above: hold-to-talk or toggle, text appears at the cursor, custom vocabulary (Superwhisper, Wispr Flow free tier), many languages (Wispr Flow lists 100+), a visible recording state, undo, privacy options (local or BYO key), and low latency. WordInk covers insertion with native undo, vocabulary in the gateway, and BYO keys; language and presets coverage is not documented as a product promise.

## What is unique

WordInk is the only item in this set that is both a drop-in web component and a provider gateway under one MIT core. The gap it fills is "dictation as a UI component with safe key handling", not "another dictation app".

## Risks the research surfaced

- Desktop apps change fast; the gateway relies on their OpenAI-compatible engine settings. Voxtype's site mentions optional remote Whisper servers without stating the protocol, so recipes need per-release checks.
- Browser dictation quality depends on the chosen provider; Groq's turbo model has a higher word error rate than `whisper-large-v3` (12% versus 10.3% per Groq's docs) and no translation.
- Pricing pages for Wispr Flow disagree across aggregators; treat figures as approximate.
