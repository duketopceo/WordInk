---
"@wordink/server": patch
---

Reject requests whose `Origin` is neither the relay's own origin nor in `allowedOrigins` with `403 origin_not_allowed` before `authorize` runs, so a cross-site page can't spend provider quota with a signed-in user's cookie (CSRF). Cross-origin cookie deployments must list the app's origin in `allowedOrigins`. Upstream provider calls now time out after `upstreamTimeoutMs` (default 15 000 ms) and answer `502 upstream_unreachable`.
