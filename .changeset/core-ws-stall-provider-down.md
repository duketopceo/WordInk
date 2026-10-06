---
"@wordink/core": patch
---

A provider socket that never opens before the connect timeout now fails with `ProviderDown` instead of `AuthFailed`. The host reports the stall with a private WebSocket close code the core maps to `ProviderDown`, while a real rejected handshake — which browsers still surface only as a 1006 close before open — keeps mapping to `AuthFailed`.
