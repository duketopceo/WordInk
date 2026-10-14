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
  CODE_SIGN_IDENTITY="-" -derivedDataPath build test
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

## Shortcuts, Siri, Action Button

Two App Intents ship in the app:

- **Dictate** (`StartDictationIntent`) opens the app and starts recording — use it from a
  lock-screen/Control Center widget (iOS 18+), an Action Button shortcut, or "Hey Siri, dictate
  with WordInk Remote".
- **Transcribe Audio** (`TranscribeAudioIntent`) is headless: it takes an audio file and returns
  the transcript text. Since an intent has no microphone, the no-UI capture flow is a Shortcuts
  composition: *Record Audio → Transcribe Audio → Copy to Clipboard / Paste*, then assign that
  shortcut to the Action Button.

## App Store

- Display name: `WordInk Remote`. Bundle ID: `com.wordink.remote` (widgets `…remote.widgets`).
- Privacy policy URL: the deployed `apps/docs` privacy page (`/privacy.html`) — it carries the
  iOS data-flow section.
- App Review note to include: the app transcribes only through a user-configured, self-hosted
  WordInk gateway (OpenAI-compatible `POST /v1/audio/transcriptions`); there is no account system
  and nothing is sent to the developer.
- Signing and App Store Connect keys are not in this repo; archive locally with `xcodegen` +
  Xcode or an unsigned CI build for compile checks.
