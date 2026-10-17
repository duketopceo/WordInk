# iOS, iPadOS & Multi-OS Architecture

WordInk provides a unified dictation and Wispr Flow post-processing stack across **iOS**, **iPadOS**, **Linux**, **macOS**, **Windows**, **Web**, and **Python** backed by a single self-hostable `@wordink/server` gateway (`POST /v1/audio/transcriptions`).

---

## Multi-OS Support Matrix

| Platform | Client / Integration | Audio & VAD Engine | Flow Cleanup & Learning |
|---|---|---|---|
| **iOS & iPadOS** | `apps/ios/WordInkApp` + `apps/ios/WordInkKeyboard` (`sdks/ios/WordInkIOS`) | Two-process App Group warm lease (`AVAudioSession` + `ActivityKit` Live Activity + `+20 dB` Whisper Gain) | Gateway `flow.ts` + on-device `LocalFallbackTranscriber` (`SFSpeechRecognizer` / `WhisperKit`) + 20s `EditVocabularyLearner` |
| **Linux (Wayland / X11)** | `Voxtype` (`~/.config/voxtype/config.toml`) & `wisp` / `wordink` CLI | PipeWire / PulseAudio / SoundDevice 16 kHz mono PCM16 | Gateway `flow.ts` + Python `wordink.flow` (`--flow-cleanup --tone`) |
| **macOS & Windows** | `TypeWhisper` (`openai-compatible` engine) | Native CoreAudio / WASAPI | Gateway `flow.ts` (`POST /v1/audio/transcriptions`) + shared vocabulary |
| **Web (Browser / React)** | `@wordink/web` (`<wordink-mic>`), `@wordink/react`, `@wordink/local` | Rust/WASM `wordink-core` (`SPEECH_RMS_THRESHOLD = 0.01`, 16 kHz resampler) + WebGPU/WASM Moonshine/Whisper | `@wordink/server` relay/gateway + `transform` hook |
| **Python / MCP** | `sdks/python` (`wordink` CLI + FastMCP server) | `sounddevice` 16 kHz WAV capture | `wordink.flow` (`apply_flow_cleanup`, `learn_vocabulary_from_edit`) verified against `tests/fixtures/flow-contract.json` |

---

## Why iOS & iPadOS Require a Two-Process Architecture

Apple enforces two strict sandbox constraints on custom keyboard extensions (`UIInputViewController`):

1. **No Direct Microphone Access in Keyboard Extensions:** Even when the user grants `RequestsOpenAccess = true` (**Allow Full Access**), iOS blocks a keyboard extension from cold-starting `AVAudioEngine` / `AVAudioSession` microphone capture on its own.
2. **~48 MB Process Memory Ceiling (`jetsam`):** Loading neural ASR models or holding large uncompressed audio buffers inside the keyboard extension process triggers immediate termination by iOS.

WordInk solves both constraints using the **Two-Process Warm-Lease Flow Architecture** across `apps/ios/WordInkApp`, `apps/ios/WordInkKeyboard`, and `sdks/ios/WordInkIOS`:

1. **Companion Host App (`apps/ios/WordInkApp`):**
   - Declares `UIBackgroundModes = [audio]` and URL scheme `wordink://` in `apps/ios/WordInkApp/Info.plist`.
   - When the keyboard's lease is cold or expired, tapping the mic button opens `wordink://flow/activate?tone=work&bundleId=...`.
   - `WordInkApp` activates `AVAudioSession` (`.playAndRecord`, mode `.measurement`, options `[.mixWithOthers, .allowBluetooth, .defaultToSpeaker]`), starts a **300-second (5-minute) warm microphone lease** (`FlowLease`), launches a Dynamic Island & Lock Screen **Live Activity** (`FlowLiveActivityWidget`), and jumps back to the source app if a `returnScheme` is provided.
   - Listens for cross-process Darwin notifications (`io.wordink.darwin.startRecording` / `io.wordink.darwin.stopRecording`).
   - When **Whispering Mode** is toggled on, applies a **+20 dB (10× linear) soft-speech software gain boost** (`WhisperGainBooster.boostPCMFloat32`) with soft-knee limiting at `0.85` so quiet speech in shared offices is cleanly amplified without digital clipping.
   - Uploads 16 kHz mono PCM16 WAV audio to `/v1/audio/transcriptions` via `TranscriberRouter`: uses `GatewayTranscriptionClient` (`Bearer wdk_...` token stored in the iOS Keychain) and automatically falls back to `LocalFallbackTranscriber` (`SFSpeechRecognizer` / `WhisperKit`) when offline or when the gateway returns `5xx`.

2. **Universal Keyboard Extension (`apps/ios/WordInkKeyboard`):**
   - Runs a lightweight state machine (`KeyboardFlowController`) well under the 48 MB extension ceiling.
   - Renders an adaptive `FlowKeyboardBarView`: a compact hold-to-talk / tap-to-toggle pill bar on iPhone (`horizontalSizeClass == .compact`) and a full-width shortcut + style preset bar on iPadOS (`horizontalSizeClass == .regular`) with a **Whisper +20 dB** badge and **Command Mode** chip.
   - Reads `textDocumentProxy.selectedText` before recording so spoken instructions can rewrite highlighted text in Command Mode.
   - Consumes completed transcripts from `group.io.wordink.shared` using a monotonically increasing sequence number (`seq > lastSeq`) so no utterance is ever inserted twice.
   - Runs `EditVocabularyLearner` for **20 seconds** after `textDocumentProxy.insertText(_:)`: if the user corrects a proper noun or spelling in `documentContextBeforeInput` (e.g. `"word ink"` -> `"WordInk"`), the correction is automatically persisted to the shared App Group Personal Dictionary.

---

## Cross-Language Deterministic Flow Contract (`tests/fixtures/flow-contract.json`)

Every platform shares the exact same deterministic post-processing rules verified against `tests/fixtures/flow-contract.json`:

- `stripFillers`: Removes spoken hesitations (`um`, `uh`, `erm`, `ah`) and comma-flanked discourse fillers (`like`, `you know`) while preserving sentence capitalization and real words.
- `resolveCourseCorrections`: Resolves mid-sentence backtracking markers (`no wait`, `wait no`, `scratch that`, `actually no`); ordinary phrases such as `I mean` are left alone so only the intended replacement phrase remains.
- `formatSpokenLists`: Formats spoken ordinal sequences (`first ... second ... third ...`) and spoken bullets (`bullet one ... bullet two ...`) into numbered or bulleted lists.
- `expandSnippets`: Expands case-insensitive spoken triggers (`"my calendly"` -> `"https://cal.com/luke"`) on word boundaries with longest-match precedence.
- `applyTonePreset`: Formats output for `casual`, `work`, `email`, `code` (`camel case`, `snake case`, `pascal case`, `kebab case`, `screaming snake case`), or `verbatim`.
- `learnVocabularyFromEdit`: Token-level LCS diff learner that extracts corrected proper nouns and custom spellings while ignoring pure deletions and large rewrites (> 40% changed tokens).
