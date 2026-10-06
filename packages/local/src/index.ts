// @wordink/local: an offline speech engine for WordInk. Moonshine tiny runs through transformers.js
// in a Web Worker (WebGPU when available, WASM otherwise) and plugs into @wordink/core as a batch
// HostProvider (KTD5). After the model has loaded, transcription needs no network (R11, AE4).
import type { HostProvider, HostProviderResult } from "@wordink/core";
import {
  DEFAULT_REVISION,
  DEFAULT_SHA256,
  SAMPLE_RATE,
  type Device,
  type FromWorker,
  type GpuLike,
  type ToWorker,
  defaultModelBaseUrl,
  inferenceTimeoutMs,
  pcm16ToFloat32,
  selectDevice,
} from "./moonshine.js";

export { DEFAULT_REVISION, DEFAULT_SHA256, MODEL_ID, type Device } from "./moonshine.js";

export interface LocalProviderOptions {
  /**
   * Directory URL holding the model files (`config.json`, `onnx/...`), for self-hosting. Relative URLs
   * resolve against the page. Default: the Hugging Face Hub at `revision`.
   */
  modelBaseUrl?: string;
  /** Hugging Face commit to load from the Hub. Default: the pinned {@link DEFAULT_REVISION}. */
  revision?: string;
  /**
   * Expected SHA-256 of the model files, by path relative to `modelBaseUrl`. Defaults to the pinned
   * checksums, which also hold for a self-hosted copy of the pinned revision. Required with `revision`.
   */
  sha256?: Record<string, string>;
  /**
   * Directory URL of the onnxruntime-web runtime files (`ort-wasm-simd-threaded*.{mjs,wasm}`).
   * Default: transformers.js loads them from cdn.jsdelivr.net.
   */
  wasmPaths?: string;
}

export interface LocalProviderEvents {
  /** Download progress 0..100 during a load that hits the network. A cached load emits none. */
  progress: number;
  /** The model is loaded and ready on `device`. */
  ready: { device: Device };
  /** The model failed to load (network, checksum, runtime). */
  error: Error;
}

export interface LocalProvider extends HostProvider {
  /** Loads the model (once; later calls return the same promise). Dictation also loads it on demand. */
  load(): Promise<void>;
  /** Subscribe to load events. Returns an unsubscribe function. */
  on<K extends keyof LocalProviderEvents>(type: K, cb: (value: LocalProviderEvents[K]) => void): () => void;
}

/**
 * A model load that goes this long without reporting download progress fails, so a later utterance can
 * retry. The window restarts on every progress step (each 1% of the download), so a slow but moving
 * download is never cut off; it also covers the stretches without progress (a cached load, and building
 * the inference session after the download).
 */
const LOAD_STALL_TIMEOUT_MS = 120_000;
/**
 * The host watchdog for one utterance. A stall timeout has no total bound, so this is what bounds the
 * wait when the first utterance includes the model download: 10 minutes covers the ~28 MB download at
 * about 50 KB/s. On a slower link that utterance fails with ProviderDown, but the load keeps going (the
 * host's cancel() does not stop it) and the next utterance picks it up.
 */
const FIRST_RESULT_TIMEOUT_MS = 600_000;

/** Rejects with `error` if `promise` has not settled within `ms`. */
function withTimeout<T>(promise: Promise<T>, ms: number, error: Error): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(error), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

type Listeners = { [K in keyof LocalProviderEvents]: Set<(v: LocalProviderEvents[K]) => void> };

/** Creates the local Moonshine provider. Pass it as `provider` to `createDictation`. */
export function createLocalProvider(options: LocalProviderOptions = {}): LocalProvider {
  if (options.revision !== undefined && options.revision !== DEFAULT_REVISION && !options.sha256) {
    throw new Error(
      "@wordink/local: pass `sha256` with a custom `revision` (the pinned checksums only match the default revision).",
    );
  }
  const revision = options.revision ?? DEFAULT_REVISION;
  const sha256 = options.sha256 ?? { ...DEFAULT_SHA256 };

  const listeners: Listeners = { progress: new Set(), ready: new Set(), error: new Set() };
  const resultListeners = new Set<(r: HostProviderResult) => void>();
  const emit = <K extends keyof LocalProviderEvents>(type: K, value: LocalProviderEvents[K]) => {
    for (const cb of listeners[type]) cb(value);
  };
  const report = (r: HostProviderResult) => {
    for (const cb of resultListeners) cb(r);
  };

  let worker: Worker | undefined;
  let loading: Promise<void> | undefined;
  let loadSettle: { resolve: () => void; reject: (e: Error) => void } | undefined;
  /** Restarts the load's stall timer; set while a load is in flight. */
  let loadProgressed: (() => void) | undefined;
  const pending = new Map<number, { resolve: (text: string) => void; reject: (e: Error) => void }>();
  let nextId = 0;

  // Utterance state. `generation` changes on start/cancel so late results of an abandoned utterance are dropped.
  let chunks: Int16Array[] = [];
  let generation = 0;

  const failAll = (err: Error) => {
    loadSettle?.reject(err);
    loadSettle = undefined;
    for (const p of pending.values()) p.reject(err);
    pending.clear();
  };

  const onMessage = (msg: FromWorker) => {
    switch (msg.type) {
      case "progress":
        loadProgressed?.();
        emit("progress", msg.percent);
        break;
      case "ready":
        loadSettle?.resolve();
        loadSettle = undefined;
        emit("ready", { device: msg.device });
        break;
      case "load-error":
        failAll(new Error(msg.message));
        break;
      case "result":
        pending.get(msg.id)?.resolve(msg.text);
        pending.delete(msg.id);
        break;
      case "transcribe-error":
        pending.get(msg.id)?.reject(new Error(msg.message));
        pending.delete(msg.id);
        break;
    }
  };

  const load = (): Promise<void> => {
    if (loading) return loading;
    const attempt: Promise<void> = (async () => {
      const device = await selectDevice((navigator as { gpu?: GpuLike }).gpu);
      const w = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
      worker = w;
      w.onmessage = (e: MessageEvent<FromWorker>) => onMessage(e.data);
      w.onerror = (e: ErrorEvent) => {
        // A dead worker never answers: fail what is in flight and let the next utterance start a new one.
        failAll(new Error(`@wordink/local worker failed: ${e.message}`));
        if (worker === w) {
          w.terminate();
          worker = undefined;
          loading = undefined;
        }
      };
      let rejectReady!: (e: Error) => void;
      const ready = new Promise<void>((resolve, reject) => {
        rejectReady = reject;
        loadSettle = { resolve, reject };
      });
      let stallTimer: ReturnType<typeof setTimeout> | undefined;
      const armStallTimer = () => {
        clearTimeout(stallTimer);
        stallTimer = setTimeout(
          () => rejectReady(new Error(`@wordink/local: model load made no progress for ${LOAD_STALL_TIMEOUT_MS} ms`)),
          LOAD_STALL_TIMEOUT_MS,
        );
      };
      loadProgressed = armStallTimer;
      armStallTimer();
      const msg: ToWorker = {
        type: "load",
        device,
        modelBaseUrl: new URL(options.modelBaseUrl ?? defaultModelBaseUrl(revision), location.href).href,
        sha256,
        ...(options.wasmPaths !== undefined && { wasmPaths: new URL(options.wasmPaths, location.href).href }),
      };
      w.postMessage(msg);
      try {
        await ready;
      } finally {
        clearTimeout(stallTimer);
        if (loadProgressed === armStallTimer) loadProgressed = undefined;
        loadSettle = undefined;
      }
    })();
    loading = attempt;
    attempt.catch((err: unknown) => {
      // Let a later utterance retry the load (e.g. once the network is back).
      if (loading === attempt) {
        loading = undefined;
        worker?.terminate();
        worker = undefined;
      }
      emit("error", err instanceof Error ? err : new Error(String(err)));
    });
    return attempt;
  };

  const transcribe = async (audio: Float32Array): Promise<string> => {
    await load();
    const w = worker;
    if (!w) throw new Error("@wordink/local worker is not running");
    const id = nextId++;
    const text = new Promise<string>((resolve, reject) => pending.set(id, { resolve, reject }));
    const msg: ToWorker = { type: "transcribe", id, audio };
    w.postMessage(msg, [audio.buffer]);
    const timeoutMs = inferenceTimeoutMs(audio.length);
    const timedOut = new Error(`@wordink/local: transcription timed out after ${timeoutMs} ms`);
    try {
      return await withTimeout(text, timeoutMs, timedOut);
    } catch (err) {
      // A worker that stops answering is likely wedged: drop it so the next utterance starts a fresh one.
      if (err === timedOut && worker === w) {
        pending.delete(id);
        w.terminate();
        worker = undefined;
        loading = undefined;
        failAll(timedOut);
      }
      throw err;
    }
  };

  return {
    id: "local",
    // The first result can include the model download, which this provider bounds only by stalls.
    capabilities: { streaming: false, sampleRate: SAMPLE_RATE, timeoutMs: FIRST_RESULT_TIMEOUT_MS },

    start(sampleRate: number) {
      if (sampleRate !== SAMPLE_RATE) {
        throw new Error(`@wordink/local expects ${SAMPLE_RATE} Hz audio, got ${sampleRate}`);
      }
      generation++;
      chunks = [];
      // Warm up while the user speaks; failures surface on finish().
      load().catch(() => {});
    },

    pushAudio(pcm: Int16Array) {
      chunks.push(pcm.slice());
    },

    async finish() {
      const mine = generation;
      const audio = pcm16ToFloat32(chunks);
      chunks = [];
      let result: HostProviderResult;
      try {
        result = { type: "final", text: (await transcribe(audio)).trim() };
      } catch (err) {
        console.error("[@wordink/local] transcription failed:", err);
        result = { type: "error", code: "ProviderDown" };
      }
      if (mine === generation) report(result);
    },

    cancel() {
      generation++;
      chunks = [];
    },

    onResult(cb) {
      resultListeners.add(cb);
      return () => resultListeners.delete(cb);
    },

    load,

    on(type, cb) {
      const set = listeners[type] as Set<typeof cb>;
      set.add(cb);
      return () => set.delete(cb);
    },
  };
}
