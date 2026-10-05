# @wordink/local

An offline speech engine for WordInk. [Moonshine tiny](https://huggingface.co/onnx-community/moonshine-tiny-ONNX)
(English) runs in the browser through [transformers.js](https://huggingface.co/docs/transformers.js) in a
Web Worker, on WebGPU when the browser has it and on WebAssembly otherwise. Once the model has loaded,
dictation makes no network calls, and audio never leaves the device.

```ts
import { createDictation } from "@wordink/core";
import { createLocalProvider } from "@wordink/local";

const local = createLocalProvider();
local.on("progress", (percent) => showProgress(percent)); // 0..100 on the first visit only
local.on("ready", ({ device }) => console.log("model ready on", device)); // "webgpu" | "wasm"
local.on("error", (e) => console.error(e));
void local.load(); // optional: start the download now instead of on the first utterance

const dictation = createDictation({ provider: local });
dictation.on("final", (text) => insertAtCursor(text));
```

It is a batch provider (`streaming: false`, 16 kHz): the core hands it the whole utterance on release and
it reports one `final`. Any model or inference failure fails that utterance with `ProviderDown`.

## First load and caching

The first load downloads about 32 MB on WASM (q8 encoder and decoder plus tokenizer) or about 80 MB on
WebGPU (fp32 encoder, q4 decoder), with `progress` events from 0 to 100. Files go into the browser's
Cache API (transformers.js's `transformers-cache`), so later visits load from disk and emit no progress.
WASM is noticeably slower than WebGPU on long utterances; WebGPU is missing on some Linux and Android
browsers.

## Model pinning and integrity

Files are fetched from a pinned Hugging Face commit (`DEFAULT_REVISION`), never `main`. The ONNX weights
are checked against pinned SHA-256 checksums (`DEFAULT_SHA256`) as they download: a mismatch fails the
load before the file reaches the cache. The small JSON files (config, tokenizer) are covered by the
revision pin only. The check needs WebCrypto, so the page must be a secure context (HTTPS or localhost).

## Self-hosting

| Option | Default | |
|---|---|---|
| `modelBaseUrl` | `https://huggingface.co/onnx-community/moonshine-tiny-ONNX/resolve/<revision>/` | Directory holding `config.json`, `tokenizer.json`, `onnx/…` (relative URLs resolve against the page) |
| `revision` | `DEFAULT_REVISION` | Hub commit for the default URL. A different revision needs `sha256` too |
| `sha256` | `DEFAULT_SHA256` | Expected SHA-256 per file, relative to `modelBaseUrl` |
| `wasmPaths` | cdn.jsdelivr.net | Directory with the onnxruntime-web runtime (`ort-wasm-simd-threaded*.mjs` / `.wasm` from `onnxruntime-web/dist`) |

To serve everything yourself, copy the pinned revision's files (keeping the `onnx/` folder) to your
server and point `modelBaseUrl` at it; the default checksums still apply. Set `wasmPaths` too, or the
runtime code comes from jsDelivr.

## Bundling

The provider starts its worker with `new Worker(new URL("./worker.js", import.meta.url), { type: "module" })`,
the pattern Vite and webpack 5 recognize. If your dev server pre-bundles dependencies and the worker
fails to load, exclude the package from pre-bundling (Vite: `optimizeDeps: { exclude: ["@wordink/local"] }`).
The e2e suite bundles it with esbuild (see `playwright.config.ts`).

## Privacy

The only network traffic is the model download (from Hugging Face, or your `modelBaseUrl`) and the
onnxruntime-web runtime (jsDelivr, or your `wasmPaths`). No telemetry.

Full docs, including a plain-HTML setup and a live demo: <https://duketopceo.github.io/WordInk/local.html>.
