# @wordink/core

## 0.1.0

### Minor Changes

- 10a10af: First release of `@wordink/core`: the WordInk dictation core as WebAssembly plus a TypeScript browser host. `createDictation()` captures the microphone through an AudioWorklet, runs Groq (via `@wordink/server` or a localhost-only dev key), OpenAI and Deepgram (short-lived tokens minted by the relay per session) or a JavaScript `HostProvider`, and reports state, level, interim and final text. Errors are typed `WordInkError`s with fix hints, and an optional `transform` hook post-processes text with a timeout fallback.

### Patch Changes

- 10a10af: OpenAI realtime now sends the GA transcription `session.update` (matching the GA client secrets the relay mints) and defaults to `gpt-live-transcribe`. A transcript that completes while the button is still held no longer leaves the session stuck in Transcribing: OpenAI keeps the socket open and the commit on release produces the final, and a streaming host provider that reports its final early completes on release. Recordings now stop on their own at 60 s and transcribe what was captured, keeping Groq uploads under the relay's 2 MB body cap.
- 10a10af: A stalled provider no longer leaves dictation stuck in `transcribing`: a provider socket that hasn't opened within 10 s is reported as failed to open, and an utterance still transcribing after 30 s (a silent socket or host provider) fails with `ProviderDown`, so the user can retry. Relay token mints now time out after 30 s and are aborted by `destroy()`. Presses while a `transform` is still running are ignored instead of opening the mic late and raising a spurious `NoSpeech`. A failed wasm load is no longer cached for the life of the page: `press()` reports it as a `ProviderDown` error and the next press retries the load. A host provider's `capabilities.timeoutMs` is now validated: `createDictation` throws a `Config` error unless it is a finite number between 1 and 2147483647 ms, since setTimeout would otherwise fire the watchdog almost at once.
- 10a10af: A provider socket that never opens before the connect timeout now fails with `ProviderDown` instead of `AuthFailed`. The host reports the stall with a private WebSocket close code the core maps to `ProviderDown`, while a real rejected handshake — which browsers still surface only as a 1006 close before open — keeps mapping to `AuthFailed`.
