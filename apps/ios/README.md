# WordInk Remote (iOS)

Hold-to-talk on iPhone, transcribed by your own WordInk gateway. Audio goes to
`POST /v1/audio/transcriptions` on the gateway you configure; the transcript lands on the
clipboard and in on-device history. App Intents surfaces cover Shortcuts, Siri, lock-screen
Control, and the Action Button.

## Build and test

The Xcode project is generated — it is never committed.

```bash
brew install xcodegen
cd apps/ios
xcodegen --spec project.yml          # produces WordInk.xcodeproj
open WordInk.xcodeproj
```

CI runs the same flow headlessly:

```bash
xcodegen --spec project.yml
xcodebuild -scheme WordInkRemote \
  -destination 'platform=iOS Simulator,name=iPhone 17' \
  CODE_SIGNING_ALLOWED=NO -derivedDataPath build test
```

Targets: `WordInkRemote` (app, iOS 17+), `WordInkWidgets` (Control Center widget, iOS 18+),
`WordInkRemoteTests`. Sources under `Shared/` compile into both binary targets.

## Reachability

The app needs to reach your gateway over HTTPS, or over HTTP only for local-network
endpoints (loopback/RFC-1918 IPs, `.local`/unqualified hosts, and Tailscale's
100.64.0.0/10 range — the tailnet is WireGuard-encrypted in transit).

- **Tailscale (recommended):** `tailscale serve --bg 8941` on the gateway host exposes
  `https://<host>.<tailnet>.ts.net` -> `http://127.0.0.1:8941` with a real certificate.
- **LAN:** enter `http://192.168.x.x:8941` — works on the same network only.
- **Public/HTTPS:** put the gateway behind any TLS-terminating reverse proxy.

## Setup

1. Mint a device token on the gateway: `wordink-gateway tokens create`.
2. In the app, enter the gateway URL and the token; it validates against `/v1/models`.
3. Hold (or tap) the capture control, dictate, release — the transcript is on the clipboard.
