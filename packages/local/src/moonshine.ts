// Moonshine model pinning plus the pure logic shared by the page side (index.ts) and the worker
// (worker.ts): device selection, download progress, checksum verification and the message protocol.
// Nothing here imports transformers.js, so it is cheap to load and unit-testable in Node.

/** Hugging Face model id of the ONNX export of Moonshine tiny (English). */
export const MODEL_ID = "onnx-community/moonshine-tiny-ONNX";

/** Pinned Hugging Face commit of {@link MODEL_ID}. Files are fetched from this revision, never `main`. */
export const DEFAULT_REVISION = "a6da1241cd305dcd64eab1edbd615f2bb9aabb95";

/**
 * SHA-256 of the model weights at {@link DEFAULT_REVISION} (the LFS object ids the Hub reports).
 * WASM loads the q8 pair (~28 MB); WebGPU loads an fp32 encoder and a q4 decoder (~76 MB).
 */
export const DEFAULT_SHA256: Readonly<Record<string, string>> = Object.freeze({
  "onnx/encoder_model_quantized.onnx": "c6fc4b7bc5af75c0591fd157a1f3829b533d18e9769a888fd95a62e470dd4f4a",
  "onnx/decoder_model_merged_quantized.onnx": "eed87831c3a6103534aae7d47a5d485025c659a1323901513961c39fe8a1a367",
  "onnx/encoder_model.onnx": "cbbf580f703b2af2137e0f6d14cd87f31cc67bd858bfd8715403a9489982d1a5",
  "onnx/decoder_model_merged_q4.onnx": "451510e9bc0d4d829e50f4feffda384a38fd0a7cc8aee72667b8fdb1fb4c7e08",
});

export type Device = "webgpu" | "wasm";

/** transformers.js dtypes per device, matching the files pinned in {@link DEFAULT_SHA256}. */
export const DTYPES = {
  wasm: { encoder_model: "q8", decoder_model_merged: "q8" },
  webgpu: { encoder_model: "fp32", decoder_model_merged: "q4" },
} as const satisfies Record<Device, { encoder_model: string; decoder_model_merged: string }>;

/** Moonshine's input rate. */
export const SAMPLE_RATE = 16000;

export function defaultModelBaseUrl(revision: string): string {
  return `https://huggingface.co/${MODEL_ID}/resolve/${revision}/`;
}

/** The subset of `navigator.gpu` used for detection. */
export interface GpuLike {
  requestAdapter(): Promise<unknown>;
}

/** WebGPU when the browser exposes it and hands out an adapter; WASM otherwise (KTD5). */
export async function selectDevice(gpu: GpuLike | undefined): Promise<Device> {
  if (!gpu || typeof gpu.requestAdapter !== "function") return "wasm";
  try {
    return (await gpu.requestAdapter()) ? "webgpu" : "wasm";
  } catch {
    return "wasm";
  }
}

/**
 * transformers.js builds file URLs as `remoteHost + remotePathTemplate + file`; split a directory URL
 * into those two parts (an empty template would produce a double slash).
 */
export function splitBaseUrl(base: string): { host: string; path: string } {
  const url = new URL(base);
  const path = url.pathname.replace(/^\/+/, "");
  return { host: `${url.origin}/`, path: path.endsWith("/") || path === "" ? path : `${path}/` };
}

/**
 * Token budget for one utterance. transformers.js uses `floor(seconds) * 6` (the paper's heuristic),
 * which is zero for anything under a second; keep the heuristic but round up and never go below 6.
 */
export function maxNewTokens(samples: number, rate: number): number {
  return Math.max(6, Math.ceil((samples / rate) * 6));
}

/** Inference on one utterance that has not answered by then fails with ProviderDown. */
export const INFERENCE_TIMEOUT_MS = 30_000;

/**
 * The inference deadline for one utterance. It scales with the audio: 60 s of speech measured about
 * 0.06 RTF on a fast WASM device, so one second of allowance per second of audio covers a device an
 * order of magnitude slower; the 30 s floor still bounds short clips.
 */
export function inferenceTimeoutMs(samples: number): number {
  return Math.max(INFERENCE_TIMEOUT_MS, Math.ceil((samples / SAMPLE_RATE) * 1000));
}

/** Concatenates PCM16 chunks into one Float32Array in [-1, 1). */
export function pcm16ToFloat32(chunks: readonly Int16Array[]): Float32Array {
  let n = 0;
  for (const c of chunks) n += c.length;
  const out = new Float32Array(n);
  let o = 0;
  for (const c of chunks) {
    for (let i = 0; i < c.length; i++) out[o++] = c[i]! / 32768;
  }
  return out;
}

/**
 * Aggregates byte progress over every network download made while loading into one 0..100 figure.
 * Emits 0 when the first download starts, rises monotonically, holds at 99 until `finish()` (the model
 * is ready), then emits 100. A load served entirely from the browser cache emits nothing.
 */
export class ProgressTracker {
  private readonly files = new Map<string, { total: number | undefined; loaded: number }>();
  private last = -1;

  constructor(private readonly emit: (percent: number) => void) {}

  begin(id: string, total: number | undefined): void {
    this.files.set(id, { total, loaded: 0 });
    this.report();
  }

  advance(id: string, bytes: number): void {
    const f = this.files.get(id);
    if (!f) return;
    f.loaded += bytes;
    this.report();
  }

  finish(): void {
    if (this.files.size === 0 || this.last === 100) return;
    this.last = 100;
    this.emit(100);
  }

  private report(): void {
    let loaded = 0;
    let total = 0;
    for (const f of this.files.values()) {
      // A download without content-length can't be placed on the scale; it only counts as started.
      if (f.total === undefined) continue;
      loaded += Math.min(f.loaded, f.total);
      total += f.total;
    }
    const raw = total > 0 ? Math.floor((loaded / total) * 100) : 0;
    const percent = Math.min(99, Math.max(this.last, raw));
    if (percent > this.last) {
      this.last = percent;
      this.emit(percent);
    }
  }
}

type FetchLike = (input: string | URL, init?: RequestInit) => Promise<Response>;

export interface VerifiedFetchOptions {
  /** Absolute URL of the model directory; pinned paths are relative to it. */
  baseUrl: string;
  /** Relative path to expected lowercase hex SHA-256. */
  sha256: Readonly<Record<string, string>>;
  onBegin(url: string, total: number | undefined): void;
  onBytes(url: string, bytes: number): void;
  /**
   * Called on a checksum mismatch. The body also errors, but browsers may surface that to
   * transformers.js as a generic "Failed to fetch", so the worker reports this error instead.
   */
  onMismatch?(err: Error): void;
}

/**
 * Wraps `fetch` for transformers.js (`env.fetch`): reports downloaded bytes, and for pinned model
 * files buffers the body and checks its SHA-256 before the final chunk is released. On a mismatch the
 * body errors, so transformers.js fails the load and never writes the file to the browser cache.
 * Files served from the cache never reach fetch, and only verified bytes were ever cached.
 */
export function createVerifiedFetch(base: FetchLike, opts: VerifiedFetchOptions): FetchLike {
  return async (input, init) => {
    const res = await base(input, init);
    if (!res.ok || !res.body) return res;
    const url = new URL(String(input)).href;
    const rel = url.startsWith(opts.baseUrl) ? url.slice(opts.baseUrl.length).split(/[?#]/)[0]! : undefined;
    const expected = rel !== undefined ? opts.sha256[rel] : undefined;
    const length = Number(res.headers.get("content-length"));
    opts.onBegin(url, Number.isFinite(length) && length > 0 ? length : undefined);

    const held: Uint8Array[] = [];
    const body = res.body.pipeThrough(
      new TransformStream<Uint8Array, Uint8Array>({
        transform(chunk, controller) {
          opts.onBytes(url, chunk.byteLength);
          if (expected === undefined) controller.enqueue(chunk);
          else held.push(chunk);
        },
        async flush(controller) {
          if (expected === undefined) return;
          const all = new Uint8Array(held.reduce((n, c) => n + c.byteLength, 0));
          let o = 0;
          for (const c of held) {
            all.set(c, o);
            o += c.byteLength;
          }
          const actual = await sha256Hex(all);
          if (actual !== expected.toLowerCase()) {
            const err = new Error(`checksum mismatch for ${rel}: expected sha256 ${expected}, got ${actual}`);
            opts.onMismatch?.(err);
            throw err;
          }
          controller.enqueue(all);
        },
      }),
    );
    return new Response(body, { status: res.status, statusText: res.statusText, headers: res.headers });
  };
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  if (!globalThis.crypto?.subtle) {
    // Fail closed: without WebCrypto (non-secure context) the model cannot be verified.
    throw new Error("checksum check needs crypto.subtle (serve the page over HTTPS)");
  }
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes as Uint8Array<ArrayBuffer>));
  return Array.from(digest, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Page -> worker. */
export type ToWorker =
  | {
      type: "load";
      device: Device;
      modelBaseUrl: string;
      sha256: Readonly<Record<string, string>>;
      wasmPaths?: string;
    }
  | { type: "transcribe"; id: number; audio: Float32Array };

/** Worker -> page. */
export type FromWorker =
  | { type: "progress"; percent: number }
  | { type: "ready"; device: Device }
  | { type: "load-error"; message: string }
  | { type: "result"; id: number; text: string }
  | { type: "transcribe-error"; id: number; message: string };
