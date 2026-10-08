---
title: WordInk Core and Web SDK - Plan
type: feat
date: 2026-10-04
topic: wordink-web-sdk
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-brainstorm
execution: code
---

# WordInk Core and Web SDK - Plan

## Goal Capsule

- **Objective:** A web developer can add push-to-talk dictation to any text field in their app in under five minutes. It uses their own speech provider key or a local model, and doesn't lock them into one vendor or a WordInk server.
- **Means:** A Rust sans-I/O dictation core compiled to WebAssembly, wrapped by a thin TypeScript browser host, a drop-in web component, a React hook and a reference credential relay, all published as open source (MIT) (KTD1, KTD2, KTD3).
- **Product authority:** Luke (duketopceo). This plan owns only the core and the Web SDK. The standalone desktop app and a Python SDK are surrounding work, described in How This Work Fits Together, and are not active scope.
- **Authority order:** Product Contract, then Key Technical Decisions, then unit Approach text. When they disagree, the earlier wins and the later is corrected.
- **Stop conditions:** Stop and ask if a provider's browser-safe auth path proves impossible (see Risks), if the core's WebAssembly bundle can't be kept under the size budget (KTD1), or if a requirement would need a WordInk-hosted service.
- **Execution profile:** Deep, phased. U1 lands first. Then U2 and U5 proceed in parallel, then U3, then U4, then U6 and U8 in parallel (U7 after U6), then U9.
- **Finish and ship:** Each unit lands as its own PR to `master` (squash-merge, one review). U9 ends with the first npm publish and the docs site live.
- **Open blockers:** None.
- **Product Contract preservation:** Product Contract unchanged except Outstanding Questions: all five planning-owned questions were resolved, into KTD1, KTD3, KTD4/KTD10, KTD5 and KTD8.

## Product Contract

### Summary

WordInk becomes "dictation as a component": one open-source core that turns speech into finished text through any provider. It ships first as a web SDK (a `<wordink-mic>` element and a React hook) that drops into existing inputs. The same core later powers the WordInk desktop app on Linux, Omarchy, Windows and macOS.

### Problem Frame

Speech-to-text APIs are abundant and cheap (Groq Whisper is about $0.04/hr), but adding *dictation* to an app still means building mic capture, push-to-talk UX, silence detection, streaming, error states, text insertion and key safety by hand. The drop-in dictation widgets that exist are either tied to one vendor's backend (Corti, Suki), bundled in paid UI suites (Kendo, Syncfusion), or thin wrappers over the browser's Web Speech API, which Firefox lacks and which needs a network connection. No open, vendor-neutral drop-in exists. That gap is WordInk's wedge, and the current Windows-only Python app can't fill it.

### Key Decisions

- Web/JS is the first SDK surface, ahead of Python and native desktop bindings, because it reaches the most developers and the drop-in gap is widest there. (session-settled: user-directed, chosen over Python-first, both-at-once and desktop-native-first: biggest audience and clearest gap.)
- Open source (MIT) and bring-your-own-key (BYOK), with no WordInk-hosted service. (session-settled: user-directed, chosen over open-core with a hosted tier and over a paid app with a free SDK: no infrastructure or billing to run; paid options stay open for later.)
- Engines are provider-agnostic: cloud providers via the developer's key, plus a local engine. (session-settled: user-directed, chosen over cloud-only and Groq-only: keeps the privacy-first pitch and avoids vendor lock-in.)
- The SDK is the product and the desktop app is its flagship consumer. The app competes in a crowded field (Handy, OpenWhispr, Spokenly, whisrs), so it isn't the wedge. It proves the SDK and carries the Linux/Wayland differentiator.
- The Computer-Using-Agent "probabilistic centering" material is dropped from WordInk's positioning. It dilutes the dictation story.

### Actors

- A1. **Integrating developer:** adds WordInk to their web app and owns the provider key.
- A2. **End user:** speaks into the integrating developer's app.
- A3. **Key holder service:** the integrating developer's own backend, which mints short-lived provider credentials so long-lived keys never reach the browser.

### Requirements

**Drop-in integration**
- R1. A developer can add dictation to an existing input or textarea with one element or one hook and a few lines of configuration.
- R2. Dictated text is inserted at the cursor of the bound field, preserving the existing text and the field's native undo behavior.
- R3. It works in current Chrome, Edge, Safari and Firefox on desktop and mobile.
- R4. The element ships a usable default look that developers can fully theme, or replace with their own button.

**Dictation experience**
- R5. Push-to-talk (hold) and toggle (tap to start or stop) modes, chosen by the developer.
- R6. Visible states for idle, listening, transcribing and error, with an audio-level indicator while listening.
- R7. Final text appears within about one second of the user releasing push-to-talk on a typical connection with the default cloud provider.
- R8. Interim text is shown while speaking when the chosen provider supports streaming, and the SDK degrades to final-only otherwise.
- R9. Clear, actionable errors for a denied microphone, a missing or invalid key, provider failure and no speech detected.

**Providers and keys**
- R10. Pluggable providers, with Groq (default), OpenAI and Deepgram available at launch through one interface.
- R11. A local engine option that runs without any network call, accepting slower first load.
- R12. Long-lived provider keys never have to be shipped to the browser. The SDK supports a short-lived-credential pattern backed by the developer's own server (A3), with a minimal reference implementation.
- R13. Adding a new provider requires no change to the developer's integration code beyond configuration.

**Text quality**
- R14. Optional post-processing hook to clean up or reformat text (punctuation, filler removal, LLM cleanup) using the developer's own model. It's off by default to protect latency.
- R15. A custom vocabulary / prompt hint the developer can pass to providers that support it.

**Developer adoption**
- R16. A docs site with a working live demo, copy-paste quickstarts for plain HTML and React, and a provider comparison table.
- R17. Published to npm under the `wordink` name scope with semantic versioning and a changelog.
- R18. No telemetry leaves the end user's device. Any usage stats stay local or go to the developer's own hooks.

### Key Flows

- F1. **First integration.** **Trigger:** a developer reads the quickstart. They install the package, add `<wordink-mic for="message">`, point it at their credential endpoint (or a dev-mode key on localhost), and dictate into the field. **Covers R1, R2, R12, R16.**
- F2. **Dictate.** **Trigger:** the end user holds the mic button or its shortcut. Listening state with a level meter, then release, then transcribing, then text inserted at the cursor. **Covers R2, R5, R6, R7, R8.**
- F3. **Switch provider.** **Trigger:** the developer changes the provider setting from Groq to Deepgram or to local. No other code changes. **Covers R10, R11, R13.**

### Acceptance Examples

- AE1. A plain HTML page with one textarea gets working dictation with no build step, using a CDN script tag and the element. **Covers R1, R3.**
- AE2. A user with a denied mic permission sees a specific "microphone blocked" message with how to fix it, not a silent failure. **Covers R9.**
- AE3. Inspecting browser network traffic during dictation shows only short-lived credentials, never the developer's long-lived provider key. **Covers R12.**
- AE4. With the local engine selected and the network disconnected after load, dictation still produces text. **Covers R11.**

### Success Criteria

- A new developer goes from zero to working dictation in under five minutes using only the quickstart.
- Same-day usable on all four target browsers, with R7 latency met on the default provider.
- The WordInk desktop app (follow-on work) can be built on this core without forking provider logic.

### Scope Boundaries

**Not in this plan (deferred or later work):**
- Standalone desktop app and its system-wide hotkey and text injection (follow-on area; see below).
- Python SDK (the current Python engine stays as-is until the desktop app replaces it).
- Mobile native SDKs (iOS, Android, React Native).
- Voice commands, editing-by-voice and agent control.
- Any WordInk-hosted service, accounts, billing or sync.
- Meeting or file transcription (long recordings, speakers).

<!-- ce-section: work-relationships -->
### How This Work Fits Together

WordInk is planned as three areas sharing one core:

1. **Core + Web SDK (this plan).** It establishes the provider interface, audio and session behavior, and key-safety pattern everything else reuses.
2. **WordInk Desktop (next).** A standalone, system-wide dictation app (hold hotkey, speak, text typed into any app, optional cleanup) for Linux with first-class Wayland/Hyprland and Omarchy packaging, then Windows and macOS. It depends on the core's provider layer. Its differentiator is a well-tested Wayland backend (portal shortcuts, virtual-keyboard injection, clipboard fallback) plus parity across all four platforms, which no open-source competitor fully has. It replaces the current Windows-only Python app.
3. **Python SDK (tentative, later).** Bindings for agent and CLI developers, built from the same provider interface. Whether it's worth doing depends on demand after the Web SDK ships.

The ordering is tentative beyond area 1. Desktop could start in parallel once the provider interface stabilizes.

### Dependencies / Assumptions

- Groq, OpenAI and Deepgram keep their current speech APIs and browser-usable short-lived credential or proxy paths. Planning verifies the exact mechanism per provider.
- A local engine that runs acceptably in-browser exists (for example a WebAssembly/WebGPU Whisper build), accepting a model download on first use.
- The `wordink` npm name or scope is available. This is unverified.

### Outstanding Questions

None remain. The former planning questions are resolved in KTD1, KTD3, KTD4, KTD5, KTD8 and KTD10. Live-API checks that remain are tracked in Risks & Dependencies.

### Sources / Research

Competitor scan (Oct 2026, aggregator-sourced pricing; see the repo's `ROADMAP.md` for the full table):
- Standalone apps: Wispr Flow, Superwhisper, Aqua, Willow, Typeless (no Linux); Spokenly (Linux, closed); Handy, OpenWhispr (open, cross-platform, partial Wayland); Voxtype, hyprwhspr, whisrs (Linux/Wayland-first).
- Embeddable: Corti and Suki dictation SDKs (vendor-tied), Kendo/Syncfusion speech components (paid suites), react-dictate-button (Web Speech only). Raw speech APIs: Groq, OpenAI, Deepgram, AssemblyAI, ElevenLabs, Speechmatics, Gladia.
- Provider auth facts used by the plan:
  - Groq has no browser-safe temporary token, so it needs a relay.
  - OpenAI mints `ek_` client secrets via `POST /v1/realtime/client_secrets` (default 600 s).
  - Deepgram mints JWTs via `POST /v1/auth/grant` (30-3600 s), passed through the WebSocket subprotocol.
- Browser facts used by the plan:
  - `execCommand('insertText')` is still the only insertion path that keeps native undo.
  - React 19 supports custom elements natively.
  - transformers.js runs Moonshine on WebGPU, with a WASM fallback.

## Planning Contract

### Key Technical Decisions

- KTD1. The dictation core is written in Rust, compiled to WebAssembly for the browser, and reused natively by the desktop app later. The browser bundle budget is 150 KB gzipped for the core. (session-settled: user-directed, chosen over a TypeScript core: one engine shared natively with WordInk Desktop outweighs the heavier build now.)
- KTD2. The core is sans-I/O: it owns session state, audio processing and provider protocol logic, and emits effects ("send this HTTP request", "open this socket", "send this frame") that a host performs. The browser host is TypeScript using getUserMedia, AudioWorklet, fetch and WebSocket. The desktop host will be native Rust. Governs R10, R13.
- KTD3. Long-lived keys stay on the developer's server through `@wordink/server`, a TypeScript relay deployable as one Cloudflare Worker or Node handler. It forwards Groq audio, because Groq has no temporary tokens, and mints short-lived OpenAI client secrets and Deepgram JWTs. The relay fails closed: it refuses every request until the developer supplies an `authorize` hook, because Origin and CORS checks are not authentication. It enforces a built-in per-client rate limit, a body-size cap, and server-fixed token TTLs. A direct-key dev mode works only on `localhost` / `127.0.0.1` origins and warns in the console. The docs state that dev-mode keys are visible to anyone with page access. Governs R12, AE3.
- KTD4. Audio is captured as Float32 in an AudioWorklet and resampled by the core: 16 kHz PCM16 (WAV for batch, linear16 for streaming) for Groq, Deepgram and local, and 24 kHz PCM16 for OpenAI realtime. This avoids MediaRecorder container differences, such as Safari's mp4/aac.
- KTD5. The local engine is a host-implemented provider in TypeScript (`@wordink/local`): Moonshine-tiny via transformers.js, WebGPU when `navigator.gpu` exists, WASM otherwise. It runs in-browser inference in JavaScript, where WebGPU lives. The core treats it like any provider through the same interface. Governs R11, AE4.
- KTD6. `<wordink-mic>` is a vanilla custom element with shadow DOM, CSS custom properties and `part`s, and no Lit, to keep the bundle small. Complex config goes through properties, and events are `CustomEvent`s that bubble and are composed.
- KTD7. Text insertion tries `document.execCommand('insertText')` first to keep native undo, then falls back to `setRangeText` plus a synthetic `input` event. Rich editors use the `transcript` event and insert themselves. Governs R2.
- KTD8. The Python app moves to `legacy/` and stays runnable until WordInk Desktop replaces it. The duplicate pre-rebrand `groq_flow/` package is deleted. (session-settled: user-approved, proposed with keep-in-place as the shown alternative, which was not chosen.)
- KTD9. Packages publish under the `@wordink` npm scope with Changesets. The unscoped `wordink` name is free. If the scope can't be claimed, packages fall back to `wordink-core`, `wordink-web` and so on.
- KTD10. Groq (the default) is batch-only, so it shows no interim text, and R8 degrades to final-only. OpenAI realtime transcription and Deepgram live provide interim text.
- KTD11. The post-processing hook (R14) is an async `transform(text) => text` the developer supplies, run after the final transcript and before insertion, with a timeout that falls back to raw text. It must call the developer's own backend, never an LLM provider with a key shipped to the browser. Docs treat dictated text as untrusted input to any prompt.

### High-Level Technical Design

```mermaid
flowchart LR
  subgraph Browser
    MIC[getUserMedia + AudioWorklet] --> HOST[@wordink/core TS host]
    HOST <--> CORE[wordink-core.wasm<br/>sessions, resample, VAD, WAV,<br/>provider protocols]
    HOST --> UI["<wordink-mic> / useDictation"]
    UI --> FIELD[input / textarea / contenteditable]
    LOCAL[@wordink/local<br/>Moonshine via transformers.js] -.provider.-> HOST
  end
  HOST -- short-lived token or audio --> RELAY[@wordink/server<br/>developer's Worker/Node]
  RELAY -- long-lived key --> P[(Groq / OpenAI / Deepgram)]
  HOST -- WebSocket with short-lived token --> P
```

The session state machine is idle, then requesting-mic, then listening, then transcribing, then idle. From any state it can go to error, and from error back to idle on retry. Listening emits level events. With streaming providers it also sends audio as it arrives and emits interim events (R8). Transcribing starts on release: streaming providers emit any remaining interim events and then the final event, and batch providers emit only the final event. The core drives transitions, and the host only reports I/O results back.

### Output Structure

```
Cargo.toml                 # workspace
crates/wordink-core/       # sans-I/O engine (KTD1, KTD2)
crates/wordink-wasm/       # wasm-bindgen bindings
packages/core/             # @wordink/core: wasm + TS browser host
packages/web/              # @wordink/web: <wordink-mic>
packages/react/            # @wordink/react: useDictation
packages/local/            # @wordink/local: Moonshine provider
packages/server/           # @wordink/server: credential relay
apps/docs/                 # docs site + live demo
examples/html/  examples/react/
legacy/                    # former Python app (KTD8)
pnpm-workspace.yaml  package.json  .changeset/
```

### Risks & Dependencies

| Risk | Mitigation |
|---|---|
| WASM core exceeds the 150 KB gzip budget | Avoid heavy crates (no full serde_json in the hot path if it is too big), `opt-level="z"`, `wasm-opt`, and a CI size check that fails over budget |
| Deepgram JWT via the `['token', jwt]` subprotocol is unverified | U3/U4 integration test against the live API; fall back to `['bearer', jwt]` |
| Browser auth for the OpenAI realtime socket with an `ek_` secret is unverified | U3 live check; fall back to WebRTC |
| OpenAI transcription model names are in flux (`gpt-live-transcribe` vs `gpt-4o-transcribe`) | Model is a config value with a tested default, checked against the live API in U3 |
| Safari ignores a requested `AudioContext` sample rate | The core always resamples from the actual device rate (KTD4) |
| The `@wordink` npm scope may be taken | KTD9 fallback names |
| WebGPU is missing on some Linux and Android browsers | KTD5 WASM fallback, with a slower-model warning |

## Implementation Units

### U1. Repo foundation and legacy move

**Goal:** Turn the repo into a Rust + pnpm monorepo with CI, with the Python app preserved under `legacy/`.
**Requirements:** Enables all. Implements KTD8, KTD9.
**Dependencies:** None.
**Files:** `legacy/` (moved from `wordink/`, `pyproject.toml`, `uv.lock`, `.python-version`, `*.bat`, `*.ps1`, `start_silent.vbs`, `.env.example`, `QUICKSTART.md`), delete `groq_flow/`, `Cargo.toml`, `rust-toolchain.toml`, `package.json`, `pnpm-workspace.yaml`, `.changeset/config.json`, `.github/workflows/ci.yml`, `.github/dependabot.yml` (pip entry moved to `/legacy`, plus cargo and npm entries), `.github/pull_request_template.md`, `AGENTS.md`, `README.md`, `.gitignore`.
**Approach:**
1. `git mv` the Python app into `legacy/` so history follows, and fix its relative paths so `uv run wordink` still works from `legacy/`.
2. Add the Cargo workspace and pnpm workspace skeletons with empty member crates and packages.
3. Replace CI with two jobs: `rust` (fmt, clippy, test, wasm build) and `node` (install, typecheck, test, size check). Keep a `legacy` job running `compileall`.
4. Rewrite README and AGENTS.md for the SDK-first positioning (pointing to ROADMAP.md), dropping the CUA section.
**Test expectation:** none, because this is scaffolding. Verified by CI going green and by `legacy/` still launching.
**Verification:** CI passes on the PR, and `cd legacy && uv run wordink --help` still works.

### U2. Core engine: sessions and audio pipeline

**Goal:** A sans-I/O Rust core that runs the dictation session state machine and turns raw Float32 audio into provider-ready PCM16 and WAV.
**Requirements:** R5, R6, R9, KTD2, KTD4.
**Dependencies:** U1.
**Files:** `crates/wordink-core/src/{lib.rs,session.rs,audio.rs,effects.rs,error.rs}`, `crates/wordink-core/tests/{session.rs,audio.rs}`.
**Approach:**
1. Session states follow the HTD. Push-to-talk and toggle are inputs that map to the same transitions.
2. Audio: resample from any input rate to 16 or 24 kHz, compute the RMS level for meter events at about 20 Hz, detect silence (energy-based) for "no speech detected" (R9), and encode WAV.
3. Effects are an enum the host executes, and results come back as events. The core holds no clocks or sockets: time is passed in.
4. Measure the wasm gzip size as soon as the crate compiles to wasm, not at U4, so a KTD1 budget problem surfaces early.
**Test scenarios:**
- Starting from idle, a mic-granted event moves to listening; release moves to transcribing; a final result returns to idle with a final event.
- Toggle mode: the first press starts and the second press stops, with the same downstream events as push-to-talk.
- A mic-denied event moves to error with a `MicDenied` code.
- Releasing after under 300 ms of audio, or with all-silent audio, ends in a `NoSpeech` error and no provider effect.
- Resampling 48 kHz to 16 kHz keeps a 1 kHz test tone's frequency within 1% and output length within one sample of expected.
- The WAV encoder writes a valid RIFF header (sample rate, mono, 16-bit) that round-trips through a decoder.
**Verification:** `cargo test -p wordink-core` passes, with no I/O dependencies in the crate.

### U3. Provider protocols: Groq, OpenAI, Deepgram

**Goal:** Provider adapters in the core that turn a session into request and frame effects and parse responses into interim and final text.
**Requirements:** R8, R10, R13, R15, KTD10.
**Dependencies:** U2.
**Files:** `crates/wordink-core/src/providers/{mod.rs,groq.rs,openai.rs,deepgram.rs}`, `crates/wordink-core/tests/providers.rs`, `crates/wordink-core/tests/fixtures/`.
**Approach:**
1. One provider trait with capability flags (streaming yes or no, accepted sample rate) and a vocabulary/prompt hint (R15).
2. Groq is a multipart POST to the relay (or directly to Groq in dev mode), with `response_format=text`.
3. OpenAI realtime transcription authenticates the browser socket with the `ek_` client secret (WebSocket subprotocol, or WebRTC if the subprotocol path is unsupported, confirmed by the U3 live check), then sends PCM16 24 kHz `input_audio_buffer.append` frames, then `commit`, and parses delta and completed events.
4. Deepgram live uses `linear16`, 16 kHz and `interim_results=true`, and parses `is_final` results. A WebSocket URL plus subprotocol credential is an effect.
5. A host-delegated provider kind: the core emits "deliver these PCM16 samples to host provider X" effects and accepts interim, final and error events back, with capability flags declared at registration (used by KTD5 and U8).
6. Provider errors map to `AuthFailed`, `RateLimited`, `ProviderDown` and `BadAudio` (R9).
**Test scenarios:**
- Groq: the built request carries the model, prompt hint and WAV part, and a `200` text body yields one final event.
- OpenAI: a recorded delta-then-completed event fixture yields interim events, then exactly one final with the full text.
- Deepgram: an interim-then-final fixture yields matching events, and a final with empty text yields `NoSpeech`.
- A `401` from any provider maps to `AuthFailed`, and a `429` maps to `RateLimited`.
- Switching the provider config with the same session inputs produces that provider's effects and changes nothing else (R13).
- A host-delegated provider receives audio-delivery effects at its declared sample rate, and its final event ends the session like a cloud provider's.
**Verification:** Fixture tests pass. A manual, opt-in live test per provider (env-key gated, not in CI) confirms the real API matches the fixtures, including the Deepgram subprotocol and OpenAI model-name risks.

### U4. WASM bindings and @wordink/core browser host

**Goal:** Ship the core to npm as `@wordink/core`: wasm bindings plus a TypeScript host that executes effects in the browser and exposes a small dictation API.
**Requirements:** R7, R9, R12, R14, KTD1, KTD3, KTD11.
**Dependencies:** U2, U3.
**Files:** `crates/wordink-wasm/src/lib.rs`, `packages/core/src/{index.ts,host.ts,worklet.ts,credentials.ts,errors.ts,providers.ts}`, `packages/core/test/{host.test.ts,credentials.test.ts,providers.test.ts}`, `packages/core/package.json`.
**Approach:**
1. wasm-bindgen exposes a session handle that accepts events and returns effects. The TS host loops by executing effects and feeding back results.
2. An AudioWorklet module streams Float32 frames into the core.
3. Host-implemented providers (used by `@wordink/local`, KTD5) register through a TypeScript `HostProvider` interface in `providers.ts`: `capabilities`, `start(sampleRate)`, `pushAudio(pcm)`, `finish()`, and an event callback for interim and final results. The wasm side forwards that provider's resampled audio effects to it and accepts its results as provider events, so the session state machine is unchanged.
4. Credentials are either `endpoint: url` (talk to `@wordink/server`) or `devKey` (rejected unless `location.hostname` is local, and the console warns).
5. Error codes surface as typed errors with human messages and fix hints (R9, AE2).
6. An optional `transform` hook with timeout (KTD11) runs before the final event is emitted.
7. A size-limit check enforces the KTD1 budget.
**Test scenarios:**
- With a mocked fetch relay returning text, a full start/stop cycle emits listening, transcribing and final events in order.
- `devKey` on a non-localhost hostname throws a configuration error before any network call.
- A `transform` that resolves returns transformed text, and one that exceeds the timeout yields the raw text plus a warning event.
- A denied mic permission (mock rejection) yields a `MicDenied` error whose message names the browser setting (AE2).
- A fake `HostProvider` registered with the host receives resampled audio after start, and its final result is emitted as the session's final event.
- Integration (Playwright, fake media device): a recorded speech clip piped through the real worklet and wasm reaches a mocked Groq relay as a valid WAV.
**Verification:** Unit and Playwright tests pass in Chromium, Firefox and WebKit, and the gzip size check passes.

### U5. @wordink/server credential relay

**Goal:** A tiny, deploy-anywhere relay so developers never ship long-lived keys to browsers.
**Requirements:** R12, AE3, KTD3.
**Dependencies:** U1. It is independent of U2 to U4, apart from the request shapes agreed in U3.
**Files:** `packages/server/src/{index.ts,groq.ts,openai.ts,deepgram.ts,cloudflare.ts,node.ts}`, `packages/server/test/relay.test.ts`, `examples/server-cloudflare/`, `examples/server-node/`.
**Approach:**
1. Groq audio is forwarded with the key from env.
2. OpenAI: `POST /v1/realtime/client_secrets` with `session.type=transcription` and a short TTL.
3. Deepgram: `POST /v1/auth/grant` with a short TTL.
4. `authorize(request)` is mandatory, with no permissive default. Without it, every route returns `403` and the server logs a setup error. The docs show binding it to the app's own user session.
5. A built-in per-client rate limit and a daily forward/mint cap are on by default and overridable via `rateLimit`.
6. Groq forwards reject bodies over a configured maximum (default 2 MB, about 60 s of 16 kHz PCM16) with `413` before any upstream call.
7. Mint TTLs are fixed server-side (default 120 s) and any client-supplied TTL is ignored.
8. CORS is restricted to configured origins (browser hygiene only, not authentication).
9. Request bodies are never logged by default.
**Test scenarios:**
- A Groq relay request forwards the audio body and sets the Authorization header from env, and the response never echoes the key.
- The OpenAI mint returns only the `ek_` secret and expiry, never the source key.
- The Deepgram mint returns the JWT and TTL.
- An `authorize` hook returning false yields `403`, with no upstream call.
- With no `authorize` hook configured, every route returns `403`, including a request with a spoofed allowed Origin.
- Exceeding the default rate limit yields `429`, with no upstream call.
- A Groq forward over the body-size cap yields `413`, with no upstream call.
- A mint request carrying a client-supplied TTL of 3600 s still receives a token with the server-fixed TTL.
- A request from a non-allowed origin gets no CORS allow header.
**Verification:** Tests pass, and the two examples deploy locally (wrangler dev / node) and serve the U4 integration test.

### U6. @wordink/web: the <wordink-mic> element

**Goal:** The drop-in web component developers add next to a field.
**Requirements:** R1, R2, R3, R4, R5, R6, AE1, KTD6, KTD7.
**Dependencies:** U4.
**Files:** `packages/web/src/{wordink-mic.ts,insert.ts,styles.ts,index.ts}`, `packages/web/test/{wordink-mic.test.ts,insert.test.ts}`, `examples/html/index.html`.
**Approach:**
1. Attributes: `for` (target id), `mode` (`hold` or `toggle`), `provider`, `endpoint`, and an optional keyboard `shortcut`. Complex config goes through properties.
2. States render as a `data-state` attribute plus `part`s (button, meter, status), and the default theme uses CSS custom properties.
3. Insertion follows KTD7 at the saved caret position, re-focusing the field first.
4. Events: `wordink-start`, `wordink-interim`, `wordink-transcript`, `wordink-error`. A default slot lets developers replace the button (R4).
5. Builds as an ES module plus a CDN build (AE1). The AudioWorklet loads from a Blob URL built from an inlined string, and the wasm is fetched relative to `import.meta.url`, so it isn't base64-inlined. The KTD1 150 KB budget measures the core wasm alone, and the CDN JavaScript has its own 40 KB gzip budget.
**Test scenarios:**
- Bound to a textarea containing "Hello |world" with the caret at the bar, a final "there" yields "Hello there world", and undo restores the original (Chromium and WebKit).
- When `execCommand` returns false (forced in test), `setRangeText` inserts the text and an `input` event fires, so a React-controlled input updates.
- Hold mode: pointerdown starts and pointerup stops. Toggle mode: two clicks start then stop.
- `data-state` cycles idle, listening, transcribing, idle, and an error sets `error` with the message visible in the status part.
- A slotted custom button replaces the default and still drives start and stop.
- AE1: `examples/html` loaded from the CDN bundle with no build step dictates into its textarea (Playwright, fake mic, mocked relay).
**Verification:** Tests pass on Chromium, Firefox and WebKit, and the CDN example works opened as a static file served over localhost.

### U7. @wordink/react: useDictation

**Goal:** An idiomatic React API over the core and element.
**Requirements:** R1, R3, R6.
**Dependencies:** U4, U6.
**Files:** `packages/react/src/{useDictation.ts,WordInkMic.tsx,index.ts}`, `packages/react/test/useDictation.test.tsx`, `examples/react/`.
**Approach:** `useDictation({ provider, endpoint, onTranscript })` returns `{ state, level, interim, start, stop, error }`. `WordInkMic` is a thin wrapper rendering `<wordink-mic>` with typed props (React 19 custom-element support).
**Test scenarios:**
- A hook driven by a mocked core moves `state` through listening, transcribing and idle, and calls `onTranscript` once with the final text.
- Unmounting during listening stops capture and releases the mic track.
- `WordInkMic` forwards `onTranscript` from the element's `wordink-transcript` event.
**Verification:** Tests pass, and `examples/react` dictates into a controlled input.

### U8. @wordink/local: offline Moonshine provider

**Goal:** A local, no-network engine developers can opt into.
**Requirements:** R11, AE4, KTD5.
**Dependencies:** U4.
**Files:** `packages/local/src/{index.ts,moonshine.ts,worker.ts}`, `packages/local/test/local.test.ts`.
**Approach:**
1. Runs transformers.js in a Web Worker so the UI stays responsive.
2. Feature-detects WebGPU and falls back to WASM.
3. The model is pinned to a specific Hugging Face revision with a checksum, and its URL is overridable so integrators can self-host it.
4. The model download emits progress events the element can show, and is cached by the browser for repeat visits.
5. Registers as a host-implemented provider (U4's `HostProvider`).
**Test scenarios:**
- With WebGPU absent (forced), the provider initializes on WASM and still transcribes a short fixture clip containing "hello world".
- Progress events go from 0 to 100 during the first load, and a second load emits no download.
- AE4: after the model loads, disabling the network (Playwright offline) still produces a transcript.
**Verification:** Tests pass in Chromium. Firefox and WebKit pass on the WASM path.

### U9. Docs site, live demo and first release

**Goal:** Developers can discover, try and install WordInk in minutes.
**Requirements:** R16, R17, R18, the five-minute success criterion, KTD9.
**Dependencies:** U5, U6, U7, U8.
**Files:** `apps/docs/` (quickstarts for HTML, React and the relay; provider comparison table; live demo page), `.changeset/`, `.github/workflows/release.yml`, `packages/*/README.md`, `packages/*/CHANGELOG.md`.
**Approach:**
1. Static docs site, with the hosted live demo using local Moonshine only, so it needs no key. A pasted dev key is offered only when the docs run from localhost (KTD3).
2. Changesets release workflow publishes on merge of the version PR, using npm trusted publishing (OIDC) with provenance, with no long-lived npm token stored. The CDN quickstart pins an exact version with a Subresource Integrity hash.
3. The README of each package carries its quickstart.
4. A privacy note lists every data flow: audio to the chosen provider (via the developer's relay for Groq), the local engine's model download host, and the recommendation that relays not log request bodies (R18).
**Test expectation:** none for docs content. The release workflow is verified by a dry-run publish.
**Verification:** A fresh-machine run-through of the HTML quickstart reaches working dictation in under five minutes. `changeset publish --dry-run` lists all five packages.

## Verification Contract

- Rust: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- WASM: `wasm-pack build crates/wordink-wasm --target web --release`, then the gzip size check (150 KB budget, KTD1).
- Node: `pnpm install --frozen-lockfile`, `pnpm -r typecheck`, `pnpm -r test` (Vitest).
- Browser end-to-end: `pnpm test:e2e` (Playwright, Chromium + Firefox + WebKit, mocked relay). Chromium and Firefox use the browser fake media device. WebKit injects a stubbed `getUserMedia` returning a MediaStreamDestination fed from the recorded clip. The denied-permission path is covered by the unit-level mock.
- Legacy: `cd legacy && uv sync && uv run python -m compileall wordink`.
- Live provider checks (manual, env-key gated, not CI): one dictation round trip per provider through the example relay.

## Definition of Done

- All units' verification outcomes pass, and CI is green on `master`.
- R1 to R18 and AE1 to AE4 are each covered by a test or a documented manual check.
- The five packages are published to npm, the docs site with live demo is live, and ROADMAP.md marks Phase 1 shipped.
- The size budget holds, and no long-lived key appears in any browser network trace (AE3).
- Abandoned experiments and dead code from discarded approaches are removed from the diff.
