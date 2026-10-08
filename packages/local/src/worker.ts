// The Web Worker that owns transformers.js and the Moonshine pipeline, so model loading and inference
// never block the page.
import { type AutomaticSpeechRecognitionPipeline, env, pipeline } from "@huggingface/transformers";
import {
  DTYPES,
  type Device,
  type FromWorker,
  MODEL_ID,
  ProgressTracker,
  SAMPLE_RATE,
  type ToWorker,
  createVerifiedFetch,
  maxNewTokens,
  splitBaseUrl,
} from "./moonshine.js";

// Typed against the DOM lib (the package also builds the page side); only these two members are used.
const scope = self as unknown as {
  postMessage(msg: FromWorker): void;
  onmessage: ((e: MessageEvent<ToWorker>) => void) | null;
};
const post = (msg: FromWorker) => scope.postMessage(msg);
const message = (err: unknown) => (err instanceof Error ? err.message : String(err));

let asr: Promise<AutomaticSpeechRecognitionPipeline> | undefined;
let mismatch: Error | undefined;

async function load(msg: Extract<ToWorker, { type: "load" }>): Promise<Device> {
  // The page only saw navigator.gpu on the main thread; make sure this worker has it too.
  const device: Device = msg.device === "webgpu" && "gpu" in navigator ? "webgpu" : "wasm";
  const { host, path } = splitBaseUrl(msg.modelBaseUrl);
  const progress = new ProgressTracker((percent) => post({ type: "progress", percent }));

  env.allowLocalModels = false;
  env.allowRemoteModels = true;
  env.remoteHost = host;
  env.remotePathTemplate = path;
  env.fetch = createVerifiedFetch(fetch.bind(globalThis), {
    baseUrl: host + path,
    sha256: msg.sha256,
    onBegin: (url, total) => progress.begin(url, total),
    onBytes: (url, bytes) => progress.advance(url, bytes),
    onMismatch: (err) => (mismatch ??= err),
  });
  const onnxWasm = env.backends.onnx.wasm;
  if (msg.wasmPaths !== undefined && onnxWasm) onnxWasm.wasmPaths = msg.wasmPaths;

  asr = pipeline("automatic-speech-recognition", MODEL_ID, { device, dtype: DTYPES[device] });
  await asr;
  progress.finish();
  return device;
}

async function transcribe(audio: Float32Array): Promise<string> {
  if (!asr) throw new Error("model not loaded");
  const p = await asr;
  if (audio.length === 0) return "";
  const out = await p(audio, { max_new_tokens: maxNewTokens(audio.length, SAMPLE_RATE) });
  return (Array.isArray(out) ? out[0]?.text : out.text) ?? "";
}

scope.onmessage = async (e: MessageEvent<ToWorker>) => {
  const msg = e.data;
  if (msg.type === "load") {
    try {
      post({ type: "ready", device: await load(msg) });
    } catch (err) {
      asr = undefined;
      post({ type: "load-error", message: message(mismatch ?? err) });
    }
  } else if (msg.type === "transcribe") {
    try {
      post({ type: "result", id: msg.id, text: await transcribe(msg.audio) });
    } catch (err) {
      post({ type: "transcribe-error", id: msg.id, message: message(err) });
    }
  }
};
