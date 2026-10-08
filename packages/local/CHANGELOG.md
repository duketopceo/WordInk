# @wordink/local

## 1.0.0

### Minor Changes

- 10a10af: First release of `@wordink/local`: an offline WordInk speech engine. `createLocalProvider()` returns a batch `HostProvider` for `createDictation` that runs Moonshine tiny through transformers.js in a Web Worker, on WebGPU when available and WASM otherwise. The model is pinned to a Hugging Face commit and its ONNX weights are SHA-256 checked as they download; `modelBaseUrl` and `wasmPaths` allow self-hosting. `load()` and `on("progress" | "ready" | "error")` report the first download (cached by the browser for repeat visits), and once loaded, dictation works offline.

### Patch Changes

- 10a10af: The inference deadline now scales with utterance length instead of a flat 30 s, so a 60-second utterance no longer risks timing out on slow devices. Measured RTF on a fast WASM device is ~0.06 (60 s of audio transcribes in ~3.4 s); the deadline is one second per second of audio with a 30 s floor for short clips.
- 10a10af: Model loads that report no download progress for 120 s, and transcriptions that haven't answered within 30 s, now fail with `ProviderDown` instead of hanging. The worker is reset so the next utterance retries with a fresh one. The load timeout restarts on every progress step, so a slow download that is still moving is never cut off. The host watchdog for an utterance is 10 minutes (`capabilities.timeoutMs`), enough for the first-use download at about 50 KB/s; on a slower link that utterance fails but the download carries on for the next one.
- Updated dependencies [10a10af]
- Updated dependencies [10a10af]
- Updated dependencies [10a10af]
- Updated dependencies [10a10af]
  - @wordink/core@0.1.0
