---
title: "iOS Remote Mic - Plan"
type: feat
date: 2026-10-13
topic: ios-remote-mic
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-brainstorm
execution: code
deepened: 2026-10-13
---

# iOS Remote Mic - Plan

> **Superseded in part (Oct 24, 2026).** The U9 owner decision (`ROADMAP.md`, program plan U9) makes the iOS app a standalone product that works on default open-source configuration: usable with a provider key and no required self-hosted gateway. The "gateway-only transcription path" decision and the exclusion of direct-provider mode below no longer govern; BYO-gateway pairing stays as an optional mode. This plan still describes the gateway-pairing flow accurately. A follow-up plan is needed for the direct-provider path.

## Goal Capsule

- **Objective:** An iPhone user who runs a WordInk gateway dictates anywhere on the device and gets text back from their own transcription pipeline — Groq-backed, with their live runtime vocabulary — on the clipboard in seconds, ready to paste into any app. The operator's daily driver is SSH/agent dictation from Termius; a public user's install-to-first-dictation takes under five minutes.
- **Product authority:** the public-App-Store BYO-gateway thin client is the settled shape. Surrounding mobile areas — dictation keyboard extension, direct-provider mode, in-app vocabulary UI, Android, guided gateway bootstrap — are not active scope.
- **Open blockers:** none against the requirements. Listing requires an Apple privacy-policy URL and App Store review before the product is publicly installable.

---

## Product Contract

### Summary

A thin first-party iOS companion for a self-hosted WordInk gateway, distributed on the public App Store: hold-to-talk → `POST /v1/audio/transcriptions` on the user's own gateway → transcript on the clipboard, with local history and App Intents capture surfaces (Shortcuts `Dictate` action, lock-screen/Control entry, Action Button/Siri) that make capture one press away.

### Problem Frame

On the desktop, the operator dictates through WordInk (voxtype → gateway → Groq) — fast, and aware of the runtime vocabulary his agents manage. Away from the desk he dictates into Termius over SSH using Apple's built-in dictation: a different engine, no access to terms like "Kurultai" or "Omarchy", and no path to Groq's speed. Agent prompts come out mangled exactly where precision matters most.

Zoomed out, the gateway is a server-side product with browser and CLI hosts but no mobile host — its pipeline dies at the desk. The capability to fix this publicly is now paid for: an Apple Developer membership unlocks TestFlight and App Store distribution. The demand the project gated mobile on is, for now, the operator's own Termius loop plus the App Store listing that makes the client real for the small set of people who already run a gateway.

### Key Decisions

- **Thin remote-mic client, not a dictation app.** The product is a mobile host for the gateway, not a competitor in iOS dictation keyboards — TypeWhisper already owns that slot with local engines and system integration. (session-settled: user-directed — chosen over a full iOS dictation app and a Shortcut-only probe: the wedge is gateway-companion dictation, where runtime vocabulary is the differentiator no generic client has.) Governs R1–R5.
- **Public App Store audience.** Strangers install it and point it at their own gateway. (session-settled: user-directed — chosen over personal-only and TestFlight-circle distribution: the $99 program is only rational if the listing is public.) Governs R6, R9.
- **Gateway-only transcription path.** No direct-provider-key mode. (session-settled: user-directed — chosen over a bring-your-own-key fallback: direct mode drifts toward generic dictation apps and loses runtime vocabulary, which lives on the gateway.) Governs R8.
- **Clipboard output, no keyboard extension in v1.** iOS forbids microphone access in keyboard extensions — even a v1 keyboard could only do the app-switch bounce — so clipboard + App Intents is the smallest honest surface. (session-settled: user-directed — chosen over a v1 keyboard extension and share extension.) Governs R2, R4.
- **Device token only on the phone.** No vocabulary management UI: admin tokens are for agents and operators, not daily dictation devices. (session-settled: user-directed — chosen over shipping vocab management with an admin credential on the device.) Governs R6, R7.
- **App Intents as the capture surface.** Shortcuts/Siri/lock-screen entry points substitute for the deferred keyboard — they remove the open-app friction that kills dictation apps, and they compose with Shortcuts automations like dictate→paste-into-Termius. (session-settled: user-directed — chosen over a bare app and over guided-gateway bootstrap.)

### Actors

- A1. **Operator** — the gateway's owner; dictates agent prompts from Termius and other iOS apps daily.
- A2. **Public iOS user** — installs from the App Store; runs (or will run) their own WordInk gateway.
- A3. **WordInk gateway** — the user's self-hosted OpenAI-compatible endpoint: `/v1/audio/transcriptions` and `/v1/models` under device-token auth, `/v1/vocabulary` under admin.
- A4. **Upstream provider** — e.g. Groq; resolved by the gateway's provider entries, invisible to the app.

### Requirements

**Capture and output**

- R1. Hold-to-talk capture: press-and-hold records audio; releasing sends it to the configured gateway and returns the transcript.
- R2. A successful transcription lands on the iOS clipboard automatically and visibly, so the user can paste into any app — the primary target being terminal/SSH apps.
- R3. The app keeps a local history of recent transcripts for re-copy; audio and history never leave the device except to the configured gateway.
- R4. App Intents surfaces let capture start without opening the main UI: a Shortcuts `Dictate` action that returns the transcript as text, plus lock-screen/Control and Action Button/Siri entry points.
- R5. On transcription failure the app shows the gateway's error plainly, retains the audio for retry, and never silently drops a dictation.

**Gateway connection**

- R6. Onboarding collects a gateway base URL and a device token, then verifies the gateway is reachable and the token works before first use, with a named error for each failure kind.
- R7. The device token lives in the platform secure credential store, never in plain app storage.
- R8. Provider selection never exists in the app: it sends audio and receives text; which upstream served the request is the gateway's business.
- R9. Reachability is the user's responsibility, honestly surfaced: the app assumes the gateway is reachable from the phone's network and guides the user toward Tailscale/LAN/exposed-HTTPS paths when validation fails.
- R10. Device tokens never cross plaintext networks: non-loopback gateway endpoints require HTTPS.

### Key Flows

- F1. First-run onboarding
  - **Trigger:** Fresh install or cleared configuration.
  - **Actors:** A2, A3
  - **Steps:** Enter gateway URL + device token → app validates reachability and auth → ready state with a first-dictation hint.
  - **Covered by:** R6, R7, R9, R10
- F2. Dictate and paste
  - **Trigger:** Hold on the capture control (in-app or via R4 entry point).
  - **Actors:** A1 or A2, A3
  - **Steps:** Hold → record → release → gateway transcription → clipboard + history row → paste into the target app.
  - **Covered by:** R1, R2, R3, R5
- F3. Shortcuts automation
  - **Trigger:** A user-built Shortcut invokes the `Dictate` action.
  - **Actors:** A1 or A2, A3
  - **Steps:** Shortcut runs the action → app records and transcribes → transcript returns to the Shortcut → chained step pastes or routes it.
  - **Covered by:** R4
- F4. Failure and retry
  - **Trigger:** Gateway unreachable, token rejected, or transcription error.
  - **Actors:** A1 or A2, A3
  - **Steps:** Error surfaces with its cause (per R5, R9) → audio retained → user retries, rotates the token, or discards.
  - **Covered by:** R5, R6, R9

```mermaid
flowchart TB
  H[hold / intent entry] --> REC[record]
  REC -->|release| GW[POST /v1/audio/transcriptions]
  GW -->|200| CB[clipboard + history]
  GW -->|error| ERR[named error + retain audio]
  ERR -->|retry| GW
```

### Acceptance Examples

- AE1. Termius dictation end-to-end
  - **Covers R1, R2, R8**
  - **Given** the operator's gateway configured and reachable over Tailscale, **when** they hold the Action Button, dictate "kurultai deploy restart", and release, **then** the Groq-backed transcript — with runtime-vocabulary casing — is on the clipboard and pastes cleanly into Termius.
- AE2. Revoked or rotated token
  - **Covers R5, R6**
  - **Given** a token revoked on the gateway, **when** the user dictates, **then** the app reports the auth failure distinctly and accepts the rotated token without reinstall.
- AE3. Unreachable gateway on cellular
  - **Covers R9**
  - **Given** a gateway that only listens on the LAN, **when** the phone is on cellular, **then** validation or dictation fails with a reachability-named error that points at VPN/Tailscale rather than a generic network error.
- AE4. Chained Shortcut
  - **Covers R4**
  - **Given** a Shortcut "Dictate → paste into focused field", **when** it runs from the lock screen, **then** the dictated text lands in the foreground app's field without the user opening the app UI.

### Success Criteria

- The operator dictates into Termius from his phone daily within a week of his own install.
- A cold install with a valid gateway URL + token reaches a first successful transcription in under five minutes.
- At least one capture entry point (lock-screen, Control, or Action Button) completes a dictation without opening the main app UI.
- The listing clears App Store review, including a working privacy-policy URL.

### Scope Boundaries

**Deferred for later**

- Dictation keyboard extension (the app-switch bounce UX) — the real "type into any app" path and the natural v2 surface.
- In-app vocabulary management — blocked on a vocabulary-scoped token narrower than admin so a phone never carries operator credentials.
- Guided gateway bootstrap and QR/URL pairing — the adoption bridge for public users; worthwhile once the gateway's own install story is published.
- Share extension, iPad-optimized layouts, Mac Catalyst, Apple Watch, Android.

**Outside this product's identity**

- Direct-provider-key mode — a standalone dictation client is a different product; the gateway is the product here.
- On-device transcription — the engine lives behind the gateway by design.
- TypeWhisper-style system-wide dictation feature depth — this app stays thin on purpose.

### Dependencies / Assumptions

- The app's public value rides on the gateway being distributable and documented; npm publishing is still gated (`NPM_PUBLISH_ENABLED`), so early audience is operator + existing gateway operators.
- Consumed endpoints exist and behave as assumed: `/v1/audio/transcriptions` and `/v1/models` under device-token auth (`packages/server/src/gateway`).
- The operator reaches his gateway over Tailscale; public users self-manage reachability (R9).
- The operator's gateway runs Groq as its provider — "Groq backed" is a property of his gateway config, not of the app (per R8).

### Outstanding Questions

Resolved during planning — stack choice is KTD1, audio format is KTD8, display name is an assumption under Planning Contract.

<!-- ce-section: work-relationships -->
### How This Work Fits Together

This plan covers the iOS companion client only; the surrounding picture is current understanding, not a committed roadmap.

- Gateway distribution (npm/PyPI publish, docker story) — **enables** public adoption; the app is installable without it but its audience stays tiny until it lands.
- iOS Shortcut probe (record → POST → clipboard in Apple's Shortcuts app) — **can proceed independently**; it delivers the operator's Termius value today with zero code and remains useful alongside the app.
- Dictation keyboard extension — **deferred candidate** that builds on this app's capture and onboarding; it is the known path to in-field typing under iOS's mic restriction.
- Vocabulary-scoped token + in-app vocab UI — **still to decide**; would let the phone surface the runtime vocabulary without an admin credential.

### Sources / Research

- `CONCEPTS.md` — gateway, device-token and admin-token scopes, runtime vocabulary semantics this app inherits.
- `docs/brainstorms/2026-10-10---desktop-surfaces-requirements.md` — the mobile deferral this plan supersedes for the companion-client scope.
- `docs/brainstorms/2026-10-09---phase-4-python-tooling-requirements.md` — the observation that the gateway's OpenAI-compatible contract already makes any HTTP client a working dictation client; this app packages that fact for iOS.
- `ROADMAP.md` — Phase 4 mobile row and the TypeWhisper competitive row that bounds this product's identity.
- `packages/server/src/gateway/` — the OpenAI-compatible endpoint contract (device/admin scopes, `/v1/vocabulary`) the app consumes.
- `packages/server/src/gateway/index.ts` — transcription accepts a multipart `file` field plus `model`, which the gateway accepts and ignores; errors use the OpenAI `{"error": {...}}` JSON shape; `/v1/models` answers under device-token auth.
- `packages/server/src/bin/wordink-gateway.ts` — the gateway is HTTP-only, binds `127.0.0.1:8941` by default, and requires an explicit opt-in to bind non-loopback addresses.
- `docs/solutions/best-practices/read-the-target-apps-openai-compatible-engine-first.md` — verify the OpenAI-compatible surface before writing a client against it; applied to the endpoint facts above.
- Apple documentation: `NSAppTransportSecurity` / `NSAllowsLocalNetworking` (IP-literal HTTP loads need this key on iOS 17+); `AppIntent.openAppWhenRun` and `ForegroundContinuableIntent` (background vs. foreground execution); Control Center widgets require iOS 18.
- Tailscale `tailscale serve --bg <port>` — provisions `https://<host>.<tailnet>.ts.net` terminating to a local HTTP port; the documented hostname path for the gateway.

---

## Planning Contract

### Key Technical Decisions

KTD1. **Native Swift + SwiftUI, iOS 17.0 deployment floor.** Over React Native: App Intents, Control Center widgets, and Keychain are native-first APIs, and the repo has no RN toolchain to inherit. iOS 17 covers App Intents, Shortcuts, and Action Button assignment; the Control widget is availability-gated to iOS 18 (its extension target deploys iOS 18.0). Resolves the origin's deferred stack question.

KTD2. **XcodeGen `project.yml` is the project definition.** The generated `.xcodeproj` is gitignored; CI and any Mac generate it identically. Hand-authored pbxproj diffs are unreviewable and Tuist is heavier than a two-target app warrants.

KTD3. **`apps/ios/` monorepo placement, outside pnpm.** Sibling of `apps/docs`, not a workspace member, no changeset — the app ships via App Store, not npm. Root JS tooling is untouched.

KTD4. **Wire contract: `POST {base}/v1/audio/transcriptions`** with multipart `file` and a fixed `model` value (accepted and ignored by the gateway); onboarding validates with `GET {base}/v1/models` under the device bearer. OpenAI `{"error": {...}}` bodies and HTTP status map into the R5 error taxonomy.

KTD5. **Endpoint URL policy.** R10's HTTPS rule binds public-routable endpoints; R9's LAN path holds for the local-network class, where HTTP is acceptable because the transport is trusted or WireGuard-encrypted (Tailscale). The app accepts `http://` only for local-network endpoints — loopback or RFC-1918 IP literals, `.local` and unqualified host names, and CGNAT-range IPs (100.64.0.0/10); every other endpoint requires `https://`. URLs carrying userinfo, a query, or a fragment are rejected — the endpoint is origin plus port only. `NSAllowsLocalNetworking` in Info.plist enables these loads on iOS 17+; `NSAllowsArbitraryLoads` is never set. `tailscale serve --bg <port>` is the documented path to a real `https://` tailnet hostname. Governs R9, R10.

KTD6. **Two intents, two execution contexts.** `TranscribeAudioIntent` (`openAppWhenRun = false`) takes an audio file and returns transcript text headless in the app process — the Shortcuts primitive. `StartDictationIntent` (`openAppWhenRun = true`) opens the app straight into recording, surfaced by a `ControlButton` Control widget (iOS 18+, widget extension target) and by Action Button through a one-step user Shortcut. Gateway-touching sources live in `Shared/`, compiled into both targets. A headless intent cannot record — the mic belongs to the app process — so the no-UI path (AE4) is a Shortcuts composition: built-in `Record Audio` -> `TranscribeAudioIntent` -> paste. Instantiates the settled capture-surface decision (Governs R4).

KTD7. **Keychain for the device token; zero third-party dependencies.** `Security` framework generic-password item (R7); gateway URL and UI state in `AppStorage`. No SwiftPM or CocoaPods dependencies.

KTD8. **Audio capture is `.m4a` (AAC) via `AVAudioRecorder`.** Accepted by the OpenAI-compatible contract and every provider type the gateway supports; mono. Resolves the origin's deferred format question.

### High-Level Technical Design

```mermaid
flowchart LR
    subgraph iPhone
        UI[DictationView hold-to-talk] --> AR[AudioRecorder .m4a]
        AR --> GC[GatewayClient]
        TI[TranscribeAudioIntent headless] --> GC
        SDI[StartDictationIntent opens app] --> UI
        CW[ControlButton widget iOS 18] --> SDI
        GC --> KC[(Keychain device token)]
        GC --> HS[(HistoryStore JSON)]
    end
    GC -->|multipart POST /v1/audio/transcriptions| GW[WordInk gateway]
    GC -->|GET /v1/models onboarding| GW
    GW --> PR[Configured provider e.g. Groq]
    GW -.->|runtime vocabulary| PR
```

Recording itself requires the app process (mic permission lives there), which is why capture intents open the app while the file-in/text-out intent can stay headless. Failed uploads retain the `.m4a` in the app container; retry reuses the file and discard deletes it (R5).

### Output Structure

```text
apps/ios/
├── project.yml                     # XcodeGen definition: app + widget + test targets
├── README.md                       # generate/build/test, reachability, store metadata
├── .gitignore                      # *.xcodeproj, build/, DerivedData/
├── Shared/                         # compiled into app AND widget targets (KTD6)
│   ├── GatewayClient.swift         #   /v1/models + /v1/audio/transcriptions
│   ├── GatewayError.swift          #   R5 taxonomy
│   ├── GatewayConfig.swift         #   URL normalize + KTD5 policy, AppStorage
│   ├── KeychainStore.swift         #   Security-framework token item
│   ├── TranscribeService.swift     #   file -> transcript, injectable for tests
│   ├── TranscribeAudioIntent.swift
│   ├── StartDictationIntent.swift
│   └── WordInkShortcuts.swift      #   AppShortcutsProvider + Siri phrases
├── WordInkRemote/
│   ├── WordInkRemoteApp.swift
│   ├── Info.plist                  # mic usage string, NSAllowsLocalNetworking
│   ├── PrivacyInfo.xcprivacy
│   ├── Assets.xcassets/            # icon placeholder + accent color
│   ├── OnboardingView.swift
│   ├── DictationView.swift
│   ├── DictationViewModel.swift
│   ├── AudioRecorder.swift         # AVAudioRecorder behind a protocol seam
│   ├── HistoryStore.swift          # Codable JSON in Application Support, capped
│   └── HistoryView.swift
├── WordInkWidgets/
│   ├── Info.plist
│   └── DictateControl.swift        # ControlWidget + ControlButton, iOS 18+
└── WordInkRemoteTests/
    ├── ScaffoldSmokeTests.swift    # proves the test target runs under xcodebuild
    ├── GatewayConfigTests.swift
    ├── GatewayClientTests.swift    # URLProtocol-mocked URLSession
    ├── KeychainStoreTests.swift
    ├── HistoryStoreTests.swift
    ├── DictationViewModelTests.swift
    ├── TranscribeServiceTests.swift
    └── IntentTests.swift
```

### Assumptions

- Working identity: display name **WordInk Remote**, bundle id `com.wordink.remote`; both adjustable at signing time without code changes.
- XcodeGen arrives via Homebrew in CI (`brew install xcodegen`) and in `apps/ios/README.md` for local Mac builds.
- CI destination is the `iPhone 17` simulator on the `macos-15` image; the exact simulator name may need adjusting to the image's installed set — a CI-config detail, not a plan change.
- No App Group entitlement in v1: the widget only launches an intent, and headless intents run in the app process, so nothing needs cross-process config sharing yet.

### Deferred Implementation Notes

- Exact `IntentFile`/UTType parameter spelling for `TranscribeAudioIntent` — verified against the SDK at build time.
- AAC bitrate/sample-rate constants — tuned against real file sizes during implementation.
- App Store Connect API keys and signing assets — operator supplies at submission time; nothing is committed.

### Risks & Dependencies

- This host has no Xcode: the `ios` CI job is the compile-and-test gate, and first on-device install needs a Mac (or a CI-produced unsigned archive) plus the operator's Apple Developer account. CI verifies build + unit tests; device flows stay a manual checklist.
- App Review may question `NSAllowsLocalNetworking` — mitigated by keeping the exception narrow, never setting `NSAllowsArbitraryLoads`, and shipping a reviewer note in the store metadata draft (U5).
- The Control widget needs iOS 18 — availability-gated so iOS 17 installs get every other surface.
- `macos-15` runner minutes are free on this public repo; no cost guard needed.

---

## Implementation Units

### U1. iOS project scaffold and CI job

**Goal:** `apps/ios/` exists as a compilable two-target Swift project generated by XcodeGen, with a stub app UI, a stub Control widget, and a passing `ios` CI job.

**Requirements:** foundation for R1-R10; F1's install path.

**Dependencies:** none.

**Files:**

- `apps/ios/project.yml` (create)
- `apps/ios/.gitignore` (create — `*.xcodeproj`, `build/`, `DerivedData/`)
- `apps/ios/README.md` (create — generate/build/test commands per KTD2)
- `apps/ios/WordInkRemote/WordInkRemoteApp.swift` (create — `App` + stub `ContentView`)
- `apps/ios/WordInkRemote/Info.plist` (create — `NSMicrophoneUsageDescription`, `NSAppTransportSecurity.NSAllowsLocalNetworking=YES`)
- `apps/ios/WordInkRemote/PrivacyInfo.xcprivacy` (create — no tracked data types)
- `apps/ios/WordInkRemote/Assets.xcassets/` (create — placeholder icon + accent color)
- `apps/ios/WordInkWidgets/Info.plist`, `apps/ios/WordInkWidgets/DictateControl.swift` (create — stub `ControlWidget`)
- `apps/ios/Shared/` (create — empty placeholder the later units fill; target membership covers app + widget)
- `apps/ios/WordInkRemoteTests/ScaffoldSmokeTests.swift` (create)
- `.github/workflows/ci.yml` (modify — add `ios` job)

**Approach:**

1. `project.yml` defines `WordInkRemote` (iOS 17.0 min), `WordInkWidgets` (iOS 18.0 min extension), and `WordInkRemoteTests`; `Shared/` enters both binary targets' sources. It declares a shared `schemes:` entry for `WordInkRemote` with the test target attached — XcodeGen only emits shared schemes it declares, and `xcodebuild -scheme` needs one.
2. The CI job runs on `macos-15`: `brew install xcodegen`, `xcodegen --spec apps/ios/project.yml --project apps/ios`, then `xcodebuild -scheme WordInkRemote -destination 'platform=iOS Simulator,name=iPhone 17' CODE_SIGNING_ALLOWED=NO -derivedDataPath apps/ios/build test`.
3. Job ordering is independent — `ios` needs nothing from the node/rust jobs.

**Test scenarios:**

- Scaffold smoke test: the test bundle links and a trivial assertion passes — proves the test target runs under `xcodebuild test`, not that features exist.

**Verification:** the `ios` CI job is green on the PR; every pre-existing job is untouched.

### U2. Gateway configuration, client, and onboarding

**Goal:** The app can take a gateway URL plus device token, validate them against `/v1/models`, persist the token in Keychain and the URL in `AppStorage`, and present the R5 error taxonomy when validation fails.

**Requirements:** R6, R7, R9, R10, R5 (network/auth/server legs), R8 (absence of provider UI is enforced by design — only URL + token fields exist).

**Dependencies:** U1.

**Files:**

- `apps/ios/Shared/GatewayConfig.swift` (create — URL parse/normalize + KTD5 policy)
- `apps/ios/Shared/KeychainStore.swift` (create)
- `apps/ios/Shared/GatewayClient.swift` (create — `URLSession`, injectable for tests)
- `apps/ios/Shared/GatewayError.swift` (create — `unreachable | invalidEndpoint | auth | server | decoding` cases with user-facing copy per R9)
- `apps/ios/Shared/TranscribeService.swift` (create — file -> transcript orchestration; U3's upload path reuses it)
- `apps/ios/WordInkRemote/OnboardingView.swift` (create — URL + token fields, validate button, named-error display, VPN/Tailscale hint text)
- `apps/ios/WordInkRemote/WordInkRemoteApp.swift` (modify — route onboarding vs. main by config presence)
- `apps/ios/WordInkRemoteTests/GatewayConfigTests.swift`, `apps/ios/WordInkRemoteTests/GatewayClientTests.swift`, `apps/ios/WordInkRemoteTests/KeychainStoreTests.swift`, `apps/ios/WordInkRemoteTests/TranscribeServiceTests.swift` (create)

**Approach:**

1. `GatewayConfig.normalizedEndpoint(_:)` trims input, accepts `host[:port]` or a full URL, then applies KTD5: `https` always allowed; `http` allowed only when the host parses as a loopback/RFC-1918/100.64.0.0-10 IP literal. Anything else throws `invalidEndpoint` with copy naming the rule.
2. `GatewayClient.validate(config)` issues `GET {base}/v1/models` with `Authorization: Bearer <token>`; 200 means configured, 401 maps to `auth`, transport failure maps to `unreachable` with R9 guidance copy, 5xx maps to `server`.
3. `GatewayClient.transcribe(fileURL:config:)` builds the multipart body (`file` field, fixed `model` value) per KTD4 and maps OpenAI `{"error"}` bodies into `GatewayError`.
4. `KeychainStore` wraps `SecItemAdd`/`SecItemCopyMatching`/`SecItemDelete` for a generic-password item keyed by service `com.wordink.remote.gateway` — `kSecAttrAccessibleAfterFirstUnlock` so background intents can read it.
5. `OnboardingView` persists URL on successful validation, writes the token to Keychain, and shows the R9 hint block when `unreachable` fires. Token rotation is the same screen revisited from settings — same write path, no reinstall (R6).

**Test scenarios:**

- URL policy matrix: `https://gw.ts.net` accepted; `http://192.168.1.10:8941` accepted; `http://100.64.5.9:8941` accepted; `http://example.com` rejected as `invalidEndpoint`; `http://8.8.8.8` rejected; bare `gw.local:8941` normalizes to `http://` accepted (.local is an unqualified local host); `https://user:pass@gw.ts.net` rejected (userinfo), as are URLs with a query or fragment.
- `validate` on mocked 200 returns success and the request carried `Authorization: Bearer <token>` (URLProtocol stub asserts the header).
- 401 maps to `.auth`, timeout/DNS maps to `.unreachable`, 502 maps to `.server`, malformed JSON maps to `.decoding`.
- Transcribe happy path on mocked 200 returns the `text` field; mocked OpenAI error body maps to the taxonomy.
- Keychain write/read/delete round-trips on the simulator; overwriting a token returns the new value, not the old.
- Covers F1: fresh install with no config routes to onboarding; validated config routes to main.

**Verification:** `xcodebuild test` runs the new suites green in the `ios` CI job.

### U3. Hold-to-talk dictation, clipboard, and history

**Goal:** Press-and-hold records `.m4a`, release uploads through `TranscribeService`, success copies the transcript to the clipboard and appends a history row, failure retains the audio for retry or discard.

**Requirements:** R1, R2, R3, R5 (recording/transcription legs), F2, F4.

**Dependencies:** U2 (client, error taxonomy, service).

**Files:**

- `apps/ios/WordInkRemote/AudioRecorder.swift` (create — `AVAudioRecorder` behind a `Recording` protocol seam for tests)
- `apps/ios/WordInkRemote/DictationViewModel.swift` (create — `idle | recording | uploading | done(text) | failed(GatewayError, retainedURL)`)
- `apps/ios/WordInkRemote/DictationView.swift` (create — hold-to-talk button, per-state UI, retry/discard affordances, haptics)
- `apps/ios/WordInkRemote/HistoryStore.swift` (create — Codable JSON in Application Support, newest-first, cap 100 entries)
- `apps/ios/WordInkRemote/HistoryView.swift` (create — list, copy-again, delete row)
- `apps/ios/WordInkRemoteTests/HistoryStoreTests.swift` (create)
- `apps/ios/WordInkRemoteTests/DictationViewModelTests.swift` (create)

**Approach:**

1. Hold gestures drive `DictationViewModel.beginRecording()` / `endRecordingAndUpload()`; a tap toggles record/stop as the accessibility alternative — VoiceOver and motor-limited users cannot press-and-hold. `AVAudioSession` permission is requested on first capture, and denial surfaces as a named `recording` error (R5).
2. On success the transcript goes to `UIPasteboard.general.string` and `HistoryStore.insert(text:)`; the temp `.m4a` is deleted.
3. On `GatewayError` the file is moved next to a retained-record marker; the failed state offers Retry (re-uploads the same file — covers revoked-token rotation per F4) and Discard (deletes it).
4. `HistoryStore` persists `[{id, timestamp, text}]` as JSON; writes are atomic (write-temp-then-move); the 100-entry cap evicts oldest — a stored-data bound, not a feature.

**Test scenarios:**

- Happy path: stub recorder yields a file, stub service returns `text` — view model lands in `.done`, `UIPasteboard.general.string == text`, one history row exists with a timestamp.
- Auth failure leaves state `.failed(.auth, url)` with the file still on disk; `retry()` after rotating the stub client to 200 succeeds and deletes the retained file.
- `discard()` removes the retained file and returns to `.idle`.
- History round-trips across a store re-init; the 101st insert evicts the oldest row.
- Mic-permission denial maps to a `recording`-flavored error state, not a crash.

**Verification:** unit tests green in CI; manual hold-to-talk sanity is part of the operator device checklist in Verification Contract.

### U4. App Intents and Control widget

**Goal:** `TranscribeAudioIntent` and `StartDictationIntent` work from Shortcuts, Siri, lock-screen Control, and Action Button, per KTD6.

**Requirements:** R4, F3, AE4.

**Dependencies:** U2 (`Shared/` client + config), U3 (recording state the app can open into).

**Files:**

- `apps/ios/Shared/TranscribeAudioIntent.swift` (create — audio `IntentFile` in, `ReturnsValue<String>` out)
- `apps/ios/Shared/StartDictationIntent.swift` (create — `openAppWhenRun = true`, sets pending-record flag)
- `apps/ios/Shared/WordInkShortcuts.swift` (create — `AppShortcutsProvider`, Siri phrase "Dictate in WordInk Remote")
- `apps/ios/WordInkWidgets/DictateControl.swift` (modify stub — `ControlButton` bound to `StartDictationIntent`)
- `apps/ios/WordInkRemote/WordInkRemoteApp.swift` (modify — consume the pending-record flag into `DictationViewModel.beginRecording()`)
- `apps/ios/WordInkRemoteTests/IntentTests.swift` (create)

**Approach:**

1. `TranscribeAudioIntent.perform()` delegates to `TranscribeService` with config read the same way the app reads it; errors propagate as typed failures Shortcuts displays. The no-UI lock-screen flow (AE4) is a user-built two-step Shortcut — built-in `Record Audio` into this intent, transcript out to a paste step — per KTD6; the README carries the recipe.
2. `StartDictationIntent.perform()` flips a `PendingAction` flag in `AppStorage` then returns; `WordInkRemoteApp` observes it and drives `DictationViewModel` into recording — the flag survives cold launch since `AppStorage` persists.
3. `DictateControl` is an iOS-18 `ControlWidget` whose `ControlButton` runs `StartDictationIntent`; Action Button and lock-screen entry come free through Shortcuts assignment — the README (U5) carries the user steps.
4. Both intent files compile into app and widget targets via the `Shared/` membership from U1.

**Test scenarios:**

- `TranscribeAudioIntent` with a fixture `.m4a` and stub service returns the transcript string without touching UI.
- Service failure propagates as an `IntentError` rather than an empty string.
- `StartDictationIntent` sets `PendingAction == .dictate`; the app's flag consumer starts recording exactly once and clears the flag (relaunch doesn't double-fire).

**Verification:** intent suites green in CI; Shortcuts/Control/Action-Button wiring verified on device per the manual checklist (unverifiable on simulator alone).

### U5. Privacy documentation and store-readiness notes

**Goal:** Public-facing privacy copy exists for the App Store listing, and the app ships with a README covering build, reachability, and capture-surface setup.

**Requirements:** Dependencies/Assumptions (privacy policy URL), R9 (reachability guidance carried in docs).

**Dependencies:** U1-U4 (documents what exists).

**Files:**

- `apps/docs/pages/privacy.md` (modify — iOS section: mic audio goes only to the user's own gateway, token lives in Keychain, no analytics/accounts/crash reporting, history and retained audio stay on-device)
- `apps/ios/README.md` (modify — reachability guide: LAN IP, `tailscale serve --bg 8941`, reverse-proxy HTTPS; Action Button + lock-screen Control setup steps; App Store metadata draft including the `NSAllowsLocalNetworking` reviewer note and mic justification)
- `ROADMAP.md` (modify — Phase 4 mobile row notes the companion client and links this plan)

**Approach:** the deployed `privacy.html` page becomes the App Store privacy-policy URL, so the iOS section states flows in the same table style as existing providers. The README is the operator and public install path — it must let a cold reader reach a working setup without this repo's context.

**Test scenarios:**

- Test expectation: none — documentation-only unit; `pnpm typecheck` plus the docs build staying green is the check (no `site.ts` change, no nav-count change).

**Verification:** docs site builds; the privacy page renders the new section in `dist/`.

---

## Verification Contract

- **iOS gate (runs on CI, not this host):** `cd apps/ios && xcodegen --spec project.yml && xcodebuild -scheme WordInkRemote -destination 'platform=iOS Simulator,name=iPhone 17' CODE_SIGNING_ALLOWED=NO -derivedDataPath build test`. The new `ios` job in `.github/workflows/ci.yml` executes exactly this.
- **Repo regression:** `pnpm install && pnpm typecheck && pnpm test` stays green — U5 touches docs markdown only, but the suite runs regardless.
- **Rust:** untouched by this plan; CI's existing `rust` job still runs for completeness.
- **Manual device checklist (operator, post-merge):** fresh install → F1 onboarding over `http://<tailscale-ip>` and over `https://<host>.ts.net` (via `tailscale serve`) → F2 hold-to-talk → paste into Termius → F3 Shortcut `Dictate` → F4 revoked token, rotate, retry — covers AE1-AE4, which cannot run on a simulator.
- **Not required:** browser e2e (`pnpm test:e2e` covers `apps/docs`, which gains only a markdown section — no behavioral change; the pre-existing `demo.spec` flake is tracked as issue #40).

## Definition of Done

- All five units landed on the feature branch; the `ios` CI job and every pre-existing job are green on the PR.
- Requirements trace: R1-R3 -> U3; R4 -> U4; R5 -> U2+U3; R6, R7, R9, R10 -> U2; R8 -> U2 (enforced by UI absence, verified in review).
- The app contains no provider selection, no vocabulary UI, and no direct provider keys — the settled scope boundaries hold in the diff, not just the plan.
- Generated artifacts (`*.xcodeproj`, `build/`, `DerivedData/`) are gitignored; no abandoned-attempt or experimental code remains.
- Residuals recorded in the final report: device install, signing, TestFlight, and App Store submission are operator-side prerequisites (Apple Developer account + a Mac or CI archive pipeline); the manual device checklist above is the shipping gate, not the merge gate.
