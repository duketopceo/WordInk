---
"@wordink/local": patch
---

Model loads that haven't finished within 120 s, and transcriptions that haven't answered within 30 s, now fail with `ProviderDown` instead of hanging. The worker is reset so the next utterance retries with a fresh one.
