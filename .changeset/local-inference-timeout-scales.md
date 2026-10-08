---
"@wordink/local": patch
---

The inference deadline now scales with utterance length instead of a flat 30 s, so a 60-second utterance no longer risks timing out on slow devices. Measured RTF on a fast WASM device is ~0.06 (60 s of audio transcribes in ~3.4 s); the deadline is one second per second of audio with a 30 s floor for short clips.
