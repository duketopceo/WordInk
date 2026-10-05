---
"@wordink/server": minor
---

Desktop gateway: an OpenAI-compatible `POST /v1/audio/transcriptions` route, enabled with the new `gateway` option. It adds provider fallback with cooldowns, per-device bearer tokens and server-side vocabulary. A `wordink-gateway` CLI serves it locally (`serve`, `check`, `tokens create|list|revoke`). Desktop dictation apps such as Voxtype can use WordInk as their backend.
