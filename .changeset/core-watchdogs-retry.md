---
"@wordink/core": patch
---

A stalled provider no longer leaves dictation stuck in `transcribing`: a provider socket that hasn't opened within 10 s is reported as failed to open, and an utterance still transcribing after 30 s (a silent socket or host provider) fails with `ProviderDown`, so the user can retry. Relay token mints now time out after 30 s and are aborted by `destroy()`. Presses while a `transform` is still running are ignored instead of opening the mic late and raising a spurious `NoSpeech`. A failed wasm load is no longer cached for the life of the page: `press()` reports it as a `ProviderDown` error and the next press retries the load.
