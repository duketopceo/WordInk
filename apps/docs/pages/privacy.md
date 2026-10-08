# Privacy and data flows

WordInk sends no telemetry. There is no WordInk server, no analytics, no crash reporting and no usage
pings. These are every network flow the SDK makes, by provider.

## Audio and transcripts

| Provider | Where audio goes | What else crosses the network |
|---|---|---|
| Groq | Browser → **your relay** → Groq. The relay forwards the audio upload with your key, because Groq has no temporary tokens | The transcript comes back the same way |
| OpenAI | Browser → your relay for a short-lived `ek_` client secret (no audio). Then audio streams from the browser **directly to OpenAI** over a WebSocket authenticated with that secret | Interim and final text come back on the WebSocket |
| Deepgram | Browser → your relay for a short-lived JWT (no audio). Then audio streams from the browser **directly to Deepgram** over a WebSocket authenticated with that token | Interim and final text come back on the WebSocket |
| Local | Nowhere. Recognition runs in the browser | Only the one-time model download below |

Your long-lived provider keys stay on your relay. The browser only ever sees short-lived credentials
(OpenAI, Deepgram) or none at all (Groq). Minted credentials live for 120 seconds by default
(`tokenTtlSeconds`, fixed on the server).

The exception is [dev-key mode](dev-keys.html), where the page sends your key to the provider
directly. It works only on localhost.

Each provider's own retention policy applies to the audio it receives. Read it before you choose one
for sensitive speech.

## The local engine's downloads

On first use `@wordink/local` downloads:

- the Moonshine model files from **huggingface.co** (a pinned commit, checksum-verified), or from your
  `modelBaseUrl`;
- the onnxruntime-web runtime from **cdn.jsdelivr.net**, or from your `wasmPaths`.

Both are cached by the browser. After that, dictation makes no network requests. Self-host both
(see [Local engine](local.html#self-hosting)) and no third party sees even the download.

## The CDN script

The [plain HTML quickstart](quickstart-html.html) loads `@wordink/web` and its WebAssembly file from
cdn.jsdelivr.net, which sees an ordinary file request. Self-host the two files to avoid that.

## Your relay

The relay sees audio (Groq) and the requests that mint tokens. `@wordink/server` never logs request or
response bodies (audio, transcripts, tokens) or keys; it logs only a setup error, upstream HTTP status
codes and error names. If you put it behind your own logging, proxy or middleware, keep request bodies
out of your logs too.

## Post-processing

A [`transform`](transform.html) sends the transcript wherever your function sends it. It is off by
default.

## Usage numbers

The SDK keeps no usage statistics. If you want them, count `wordink-transcript` events (or
`onTranscript` calls) in your own code and send them to your own backend.
