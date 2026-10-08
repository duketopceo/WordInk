---
"@wordink/core": minor
---

First release of `@wordink/core`: the WordInk dictation core as WebAssembly plus a TypeScript browser host. `createDictation()` captures the microphone through an AudioWorklet, runs Groq (via `@wordink/server` or a localhost-only dev key), OpenAI and Deepgram (short-lived tokens minted by the relay per session) or a JavaScript `HostProvider`, and reports state, level, interim and final text. Errors are typed `WordInkError`s with fix hints, and an optional `transform` hook post-processes text with a timeout fallback.
