---
"@wordink/local": minor
---

First release of `@wordink/local`: an offline WordInk speech engine. `createLocalProvider()` returns a batch `HostProvider` for `createDictation` that runs Moonshine tiny through transformers.js in a Web Worker, on WebGPU when available and WASM otherwise. The model is pinned to a Hugging Face commit and its ONNX weights are SHA-256 checked as they download; `modelBaseUrl` and `wasmPaths` allow self-hosting. `load()` and `on("progress" | "ready" | "error")` report the first download (cached by the browser for repeat visits), and once loaded, dictation works offline.
