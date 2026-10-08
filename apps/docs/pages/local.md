# Local engine

`@wordink/local` runs [Moonshine tiny](https://huggingface.co/onnx-community/moonshine-tiny-ONNX)
(English) in the browser through [transformers.js](https://huggingface.co/docs/transformers.js), in a
Web Worker so the page stays responsive. It uses WebGPU when the browser has it and WebAssembly
otherwise. Once the model has loaded, dictation makes no network calls and audio never leaves the
device. The [live demo](./#try-it) uses it.

```ts
import { createDictation } from "@wordink/core";
import { createLocalProvider } from "@wordink/local";

const local = createLocalProvider();
local.on("progress", (percent) => showProgress(percent)); // 0..100, first visit only
local.on("ready", ({ device }) => console.log("model ready on", device)); // "webgpu" | "wasm"
local.on("error", (e) => console.error(e));
void local.load(); // optional: start the download now instead of on the first press

const dictation = createDictation({ provider: local });
```

With the element, set it as a property: `document.querySelector("wordink-mic").provider = local`.
Plain HTML pages need the two files served from their own origin; see the
[HTML quickstart](quickstart-html.html#no-key-at-all-the-local-engine).

It is a batch provider: the core hands it the whole utterance on release and it reports one final
text. A model or inference failure fails that utterance with a `ProviderDown` error.

## Size, speed and caching

| | WASM | WebGPU |
|---|---|---|
| First download | about 32 MB (q8 encoder and decoder, tokenizer) | about 80 MB (fp32 encoder, q4 decoder) |
| Speed | Noticeably slower on long utterances | Fast |

Files go into the browser's Cache API (`transformers-cache`), so later visits load from disk and emit
no progress events. WebGPU is missing on some Linux and Android browsers, which then use WASM.

## Pinning and checksums

The model is fetched from a pinned Hugging Face commit (`DEFAULT_REVISION`), never `main`. The ONNX
weights are checked against pinned SHA-256 checksums (`DEFAULT_SHA256`) as they download: a mismatch
fails the load before the file reaches the cache. The small JSON files (config, tokenizer) are covered
by the revision pin only. The check needs WebCrypto, so the page must be a secure context (HTTPS or
localhost).

## Self-hosting

By default the model comes from huggingface.co and the onnxruntime-web runtime from cdn.jsdelivr.net.
To serve everything from your own origin:

| Option | Default | |
|---|---|---|
| `modelBaseUrl` | `https://huggingface.co/onnx-community/moonshine-tiny-ONNX/resolve/<revision>/` | Directory holding `config.json`, `tokenizer.json`, `onnx/…`. Relative URLs resolve against the page |
| `wasmPaths` | cdn.jsdelivr.net | Directory with the onnxruntime-web runtime (`ort-wasm-simd-threaded*.mjs` and `.wasm` from `onnxruntime-web/dist`) |
| `revision` | `DEFAULT_REVISION` | Hub commit for the default URL. A different revision needs `sha256` too |
| `sha256` | `DEFAULT_SHA256` | Expected SHA-256 per file, relative to `modelBaseUrl` |

Copy the pinned revision's files (keeping the `onnx/` folder) to your server and point `modelBaseUrl`
at them; the default checksums still apply. Set `wasmPaths` as well, or the runtime code still comes
from jsDelivr.

```ts
const local = createLocalProvider({
  modelBaseUrl: "/models/moonshine-tiny/",
  wasmPaths: "/ort/",
});
```

## Bundlers

The provider starts its worker with `new Worker(new URL("./worker.js", import.meta.url), { type: "module" })`,
which Vite and webpack 5 recognize. If a dev server pre-bundles dependencies and the worker fails to
load, exclude the package (Vite: `optimizeDeps: { exclude: ["@wordink/local"] }`).
