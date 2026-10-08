---
"@wordink/server": minor
---

The desktop gateway now also serves `GET {basePath}/v1/models` — the OpenAI model-list shape, behind the same `wdk_` device-token gate and per-token limiter as transcriptions. It reports each configured provider's effective model, so OpenAI-compatible clients with model discovery (e.g. TypeWhisper's bundled "OpenAI Compatible" engine) populate their picker instead of requiring a manual model id. `GATEWAY_MODELS_PATH` is exported next to `GATEWAY_TRANSCRIPTIONS_PATH`.
