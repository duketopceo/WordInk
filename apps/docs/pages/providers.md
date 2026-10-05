# Providers

Every provider goes through the same interface, so switching is a configuration change:
`provider="groq"` (the default), `"openai"`, `"deepgram"`, or a provider object such as
[`@wordink/local`](local.html). Nothing else in your integration changes.

| | Groq | OpenAI | Deepgram | Local |
|---|---|---|---|---|
| Default model | `whisper-large-v3-turbo` | `gpt-live-transcribe` | `nova-3` | Moonshine tiny (English) |
| Streaming (interim text) | No: final text only | Yes | Yes | No: final text only |
| Latency class | Batch: one upload on release, fast | Streaming over a WebSocket | Streaming over a WebSocket | Batch, on the user's device; slower on WASM, first use waits for the model download |
| Approximate cost per hour of audio | ~$0.04 | Varies by model (`gpt-4o-transcribe` ~$0.36; check current pricing for `gpt-live-transcribe`) | ~$0.46 (streaming) | Free (runs on the user's CPU or GPU) |
| Works offline | No | No | No | Yes, after the first load |
| Key path | Your relay forwards the audio with your key (Groq has no temporary tokens) | Your relay mints a short-lived `ek_` client secret; the browser connects to OpenAI directly | Your relay mints a short-lived JWT; the browser connects to Deepgram directly | No key |
| `hint` (custom vocabulary) | Sent as the prompt | Sent as the prompt | Sent as key terms (split on commas and new lines) | Ignored |

Costs are approximate list prices from public pricing pages and aggregators as of October 2026. Check
each provider's pricing before you budget.

## Choosing

- **Groq** is the default: the cheapest cloud option and fast, but you see no text until the user
  releases the button.
- **OpenAI** and **Deepgram** show interim text while the user speaks. Pick on price, accuracy for
  your users' speech, and which account you already have.
- **Local** costs nothing per minute and keeps audio on the device, at the price of a first download
  (about 32 MB) and slower transcription on machines without WebGPU. English only.

Every cloud provider needs your [relay](relay.html) in production. During local development you can
use a [dev key](dev-keys.html) instead.

## Model override

`model` replaces the default model for the chosen provider, for example
`<wordink-mic provider="groq">` with `mic.model = "whisper-large-v3"`.
