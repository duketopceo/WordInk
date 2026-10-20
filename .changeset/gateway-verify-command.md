---
"@wordink/server": minor
---

The gateway CLI gains `wordink-gateway verify [--live] [--config <path>]` — a one-command smoke test that proves the mechanics (config, `check`, `serve` bind, token mint/scope/revoke, `/v1/models`, a multipart transcription, vocabulary write+read) against a loopback fake provider with no keys required, inside a temp XDG sandbox that never touches the operator's real config or token store. `--live` additionally transcribes the bundled `hello-world.wav` through each real provider entry that resolves a key — one isolated `serve` per entry, so the fallback chain can't mask a dead provider.
