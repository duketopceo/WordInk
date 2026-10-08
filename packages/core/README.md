# @wordink/core

The WordInk dictation engine for the browser: a Rust sans-I/O core compiled to WebAssembly plus a
small TypeScript host that captures the microphone (AudioWorklet), talks to the speech provider, and
reports state, levels, interim and final text. Most apps use `<wordink-mic>` (`@wordink/web`) or the
React hook instead; use this package directly to build your own UI.

```ts
import { createDictation } from "@wordink/core";

const dictation = createDictation({
  provider: "groq",          // "groq" | "openai" | "deepgram" | a HostProvider
  endpoint: "/api/wordink",  // your @wordink/server relay
  mode: "hold",              // or "toggle"
});

dictation.on("state", (s) => console.log(s)); // idle | requesting-mic | listening | transcribing | error
dictation.on("final", (text) => insertAtCursor(text));
dictation.on("error", (e) => showError(e.message, e.hint));

button.onpointerdown = () => dictation.press();
button.onpointerup = () => dictation.release();
```

## Credentials

Long-lived provider keys never belong in the page. Point `endpoint` at
[`@wordink/server`](../server): it forwards Groq audio with your key and mints short-lived
OpenAI client secrets and Deepgram JWTs, one per dictation session. Relay requests are sent with
`credentials: "include"` so your relay's `authorize` hook can check your app's session cookie.

`devKey` sends a provider key straight from the page. It works only when the page is served from
`localhost`, `127.0.0.1` or `[::1]`, logs a console warning, and anyone with access to the page can
read the key. On any other hostname `createDictation` throws a `Config` error before any network call.

## Options

| Option | Default | |
|---|---|---|
| `provider` | (required) | `"groq"`, `"openai"`, `"deepgram"`, or a `HostProvider` |
| `endpoint` | | Relay base URL |
| `devKey` | | Localhost-only provider key |
| `mode` | `"hold"` | `"hold"` (push-to-talk) or `"toggle"` |
| `hint` | | Vocabulary / prompt hint |
| `model` | provider default | Model override |
| `transform` | | `async (text) => text`, run before `final` (call your own backend) |
| `transformTimeoutMs` | `3000` | On timeout or failure the raw text is used and a `warning` fires |

## API

- `start()` / `stop()`: start listening if idle; stop and transcribe if listening.
- `press()` / `release()`: raw button input; the mode decides what they do.
- `on(type, cb)` returns an unsubscribe function. Events: `state`, `level` (RMS 0..1),
  `interim` (whole transcript so far), `final`, `error` (`WordInkError`), `warning`.
- `destroy()` releases the microphone, closes sockets, aborts requests and drops listeners.
- `ready` resolves when the wasm has loaded.

Call `start()` / `press()` from a user gesture: the AudioContext is created there.

## Errors

`WordInkError` has a `code`, a human `message` and a fix `hint`:
`MicDenied`, `NoSpeech`, `AuthFailed`, `RateLimited`, `ProviderDown`, `BadAudio`, and `Config` for
invalid options.

## Host providers

A `HostProvider` is a provider implemented in JavaScript (such as `@wordink/local`). The core resamples
audio to its declared rate and calls `start(sampleRate, hint)`, `pushAudio(pcm: Int16Array)`,
`finish()` and `cancel()`; the provider reports `{ type: "interim" | "final", text }` or
`{ type: "error", code }` through `onResult`.

## Bundling

The wasm is loaded from `wasm/wordink_core_bg.wasm` relative to the module (`new URL(..., import.meta.url)`),
which Vite, webpack 5 and plain ES-module CDNs resolve. The core wasm is about 35 KB gzipped
(budget: 150 KB). No telemetry is sent anywhere.

Full docs and a live demo: <https://duketopceo.github.io/WordInk/>.
