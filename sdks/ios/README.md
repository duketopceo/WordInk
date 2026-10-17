# WordInkIOS

Lightweight Swift 5.9+ package (`iOS 17+`, `macOS 14+`) implementing the Wispr Flow two-process dictation architecture for iOS custom keyboard extensions and companion host applications.

## Why a Two-Process Architecture Is Required on iOS

Apple enforces strict sandbox and memory limits on custom keyboard extensions (`UIInputViewController`):

1. **48 MB Memory Ceiling (`jetsam`):** A keyboard extension cannot load local Whisper/Moonshine neural models or retain large PCM buffers in-process without being terminated by the operating system.
2. **Microphone & Background Audio Sandbox:** iOS blocks keyboard extensions from starting `AVAudioEngine` / `AVAudioSession` recording unless `RequestsOpenAccess = YES` is granted, and even with Open Access, iOS restricts background microphone activation directly from an unprimed extension process.
3. **`RequestsOpenAccess` + Deep-Link Warm Lease:** Like Wispr Flow, `WordInkIOS` splits responsibility across two targets sharing an App Group container (`group.io.wordink.shared`):
   - **Host App (`WordInkHost`):** Activated once via deep link (`wordink://flow/warm?tone=work`) when the mic lease is cold or expired. The host app configures an active `AVAudioSession` with `.playAndRecord` + `UIBackgroundModes = [audio]`, starts a Dynamic Island / Lock Screen **Live Activity** (`ActivityKit`), and keeps a warm microphone lease (`FlowLease`, default TTL 300 seconds) alive in the background.
   - **Keyboard Extension (`WordInkKeyboard`):** Runs a zero-allocation state machine (`KeyboardFlowController`) well under a few megabytes of RAM. When the user taps the mic key:
     - If the `FlowLease` in `group.io.wordink.shared` is **cold or expired**, `onMicTap(now:)` returns `.openHostApp(URL)` so the keyboard opens the host app to establish the background audio session and returns immediately.
     - If the `FlowLease` is **warm**, `onMicTap(now:)` returns `.startRecordingIPC`, posting a Darwin notification (`io.wordink.darwin.startRecording`) to the background host app without leaving the current app.
     - When the user releases the mic key, `onMicRelease(now:)` posts `io.wordink.darwin.stopRecording`. The host app streams or uploads the audio to the `@wordink/server` gateway (`/v1/audio/transcriptions` with `flow` cleanup enabled), writes the cleaned transcript and monotonic sequence number (`seq`) into `SharedFlowPayload` inside `group.io.wordink.shared`, and posts `io.wordink.darwin.transcriptReady`.
     - The keyboard calls `pollInsertion(lastSeq:)`, inserts the new utterance via `textDocumentProxy.insertText(_:)` exactly once (guarded by `seq > lastSeq`), and monitors `documentContextBeforeInput` for 20 seconds via `EditVocabularyLearner` to auto-learn any proper nouns or spelling corrections the user makes.

## Modules

| File | Responsibility |
|---|---|
| `AppGroupIPC.swift` | `FlowSessionState`, `FlowLease` (default 300 s warm-mic TTL), `SharedFlowPayload` (JSON codable over `group.io.wordink.shared` with monotonic `seq`), and Darwin notification names. |
| `KeyboardFlowController.swift` | Pure state machine for the keyboard extension (`onMicTap(now:)`, `onMicRelease(now:)`, `pollInsertion(lastSeq:)`). |
| `ContextToneMapper.swift` | Maps host app bundle IDs (`com.apple.MobileSMS` -> `.casual`, `com.tinyspeck.chatlyio` -> `.work`, `com.apple.mobilemail` -> `.email`, `com.Termius` / code traits -> `.code`) and keyboard traits to `FlowTone`. |
| `EditVocabularyLearner.swift` | Post-insertion token-diff learner that detects user corrections in `documentContextBeforeInput` within a 20-second window and appends learned terms to the shared App Group dictionary. |
