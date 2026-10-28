---
"@wordink/server": minor
---

Add `wordink-gateway pair`: mints a device token and prints a `wordink-remote://pair?url=…&token=…` deep link (rendered as a camera-scannable terminal QR) so the iOS WordInk Remote app onboards in one tap — scan, validate, done. `--url` defaults to the new `publicUrl` gateway.json field; LAN/Tailscale transport policy is enforced at mint time.
