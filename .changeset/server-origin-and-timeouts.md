---
"@wordink/server": patch
---

Reject requests whose `Origin` is neither the relay's own origin nor in `allowedOrigins` with `403 origin_not_allowed` before `authorize` runs, so a cross-site page can't spend provider quota with a signed-in user's cookie (CSRF). Cross-origin cookie deployments must list the app's origin in `allowedOrigins`. Upstream provider calls now time out after `upstreamTimeoutMs` (default 15 000 ms) and answer `502 upstream_unreachable`, including a Groq reply whose body stalls after its headers.

The Node adapter now builds the request URL from its own origin plus the request-target as a path, so a protocol-relative target like `//evil.com/openai/token` can no longer pose as the attacker's origin. Its origin is `https` on a TLS socket, and with `trustProxy: true` it comes from `X-Forwarded-Proto` and `X-Forwarded-Host` (ignored otherwise), so same-origin HTTPS requests pass the `Origin` check.
