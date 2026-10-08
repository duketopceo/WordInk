---
"@wordink/local": patch
---

Model loads that report no download progress for 120 s, and transcriptions that haven't answered within 30 s, now fail with `ProviderDown` instead of hanging. The worker is reset so the next utterance retries with a fresh one. The load timeout restarts on every progress step, so a slow download that is still moving is never cut off. The host watchdog for an utterance is 10 minutes (`capabilities.timeoutMs`), enough for the first-use download at about 50 KB/s; on a slower link that utterance fails but the download carries on for the next one.
