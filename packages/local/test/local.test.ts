import { createHash } from "node:crypto";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HostProvider, HostProviderResult } from "@wordink/core";
import { createLocalProvider } from "../src/index.js";
import {
  DEFAULT_REVISION,
  DEFAULT_SHA256,
  ProgressTracker,
  createVerifiedFetch,
  defaultModelBaseUrl,
  maxNewTokens,
  pcm16ToFloat32,
  selectDevice,
  splitBaseUrl,
  type ToWorker,
} from "../src/moonshine.js";

// --- device selection --------------------------------------------------------------------------

describe("selectDevice", () => {
  it("uses WASM when navigator.gpu is absent", async () => {
    expect(await selectDevice(undefined)).toBe("wasm");
  });
  it("uses WASM when no adapter is available", async () => {
    expect(await selectDevice({ requestAdapter: async () => null })).toBe("wasm");
  });
  it("uses WASM when the adapter request throws", async () => {
    expect(await selectDevice({ requestAdapter: async () => Promise.reject(new Error("no")) })).toBe("wasm");
  });
  it("uses WebGPU when an adapter is available", async () => {
    expect(await selectDevice({ requestAdapter: async () => ({}) })).toBe("webgpu");
  });
});

// --- progress aggregation ----------------------------------------------------------------------

describe("ProgressTracker", () => {
  it("goes 0 to 100 across concurrent downloads, monotonically", () => {
    const seen: number[] = [];
    const t = new ProgressTracker((p) => seen.push(p));
    t.begin("a", 100);
    t.advance("a", 50); // 50%
    t.begin("b", 300); // total grows: 50/400 would drop to 12, but progress never goes back
    t.advance("b", 300);
    t.advance("a", 50);
    t.finish();
    expect(seen[0]).toBe(0);
    expect(seen.at(-1)).toBe(100);
    for (let i = 1; i < seen.length; i++) expect(seen[i]!).toBeGreaterThan(seen[i - 1]!);
    expect(seen.filter((p) => p === 100)).toHaveLength(1);
  });
  it("holds at 99 until the model is ready", () => {
    const seen: number[] = [];
    const t = new ProgressTracker((p) => seen.push(p));
    t.begin("a", 10);
    t.advance("a", 10);
    expect(seen.at(-1)).toBe(99);
    t.finish();
    expect(seen.at(-1)).toBe(100);
  });
  it("emits nothing when nothing was downloaded (cached load)", () => {
    const seen: number[] = [];
    const t = new ProgressTracker((p) => seen.push(p));
    t.finish();
    expect(seen).toEqual([]);
  });
  it("counts a download without content-length by bytes seen", () => {
    const seen: number[] = [];
    const t = new ProgressTracker((p) => seen.push(p));
    t.begin("a", undefined);
    t.advance("a", 5);
    t.finish();
    expect(seen).toEqual([0, 100]);
  });
});

// --- checksum-verifying fetch ------------------------------------------------------------------

const sha = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");
const BASE = "https://models.example/moonshine/";

function fakeFetch(bodies: Record<string, Uint8Array>) {
  return vi.fn(async (input: string | URL) => {
    const body = bodies[String(input)];
    if (!body) return new Response("missing", { status: 404 });
    // Two chunks, so progress sees more than one step.
    const half = Math.floor(body.length / 2);
    const stream = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(body.slice(0, half));
        c.enqueue(body.slice(half));
        c.close();
      },
    });
    return new Response(stream, { headers: { "content-length": String(body.length) } });
  });
}

describe("createVerifiedFetch", () => {
  const model = new TextEncoder().encode("pretend onnx weights");

  it("passes a model file through when its sha256 matches, reporting bytes", async () => {
    const begin = vi.fn();
    const advance = vi.fn();
    const f = createVerifiedFetch(fakeFetch({ [`${BASE}onnx/m.onnx`]: model }), {
      baseUrl: BASE,
      sha256: { "onnx/m.onnx": sha(model) },
      onBegin: begin,
      onBytes: advance,
    });
    const res = await f(`${BASE}onnx/m.onnx`);
    expect(new Uint8Array(await res.arrayBuffer())).toEqual(model);
    expect(begin).toHaveBeenCalledWith(`${BASE}onnx/m.onnx`, model.length);
    expect(advance.mock.calls.reduce((n, [, b]) => n + (b as number), 0)).toBe(model.length);
  });

  it("fails the body read when a pinned file's sha256 does not match", async () => {
    const onMismatch = vi.fn();
    const f = createVerifiedFetch(fakeFetch({ [`${BASE}onnx/m.onnx`]: model }), {
      baseUrl: BASE,
      sha256: { "onnx/m.onnx": "0".repeat(64) },
      onBegin: () => {},
      onBytes: () => {},
      onMismatch,
    });
    const res = await f(`${BASE}onnx/m.onnx`);
    await expect(res.arrayBuffer()).rejects.toThrow(/checksum/i);
    expect(onMismatch.mock.calls[0]?.[0].message).toMatch(/checksum mismatch for onnx\/m\.onnx/);
  });

  it("does not hash files that are not pinned, or that live elsewhere", async () => {
    const other = "https://cdn.example/ort.wasm";
    const f = createVerifiedFetch(
      fakeFetch({ [`${BASE}config.json`]: model, [other]: model }),
      { baseUrl: BASE, sha256: { "onnx/m.onnx": "0".repeat(64) }, onBegin: () => {}, onBytes: () => {} },
    );
    expect(new Uint8Array(await (await f(`${BASE}config.json`)).arrayBuffer())).toEqual(model);
    expect(new Uint8Array(await (await f(other)).arrayBuffer())).toEqual(model);
  });

  it("normalizes the URL before matching pinned paths, so a dot-segment cannot skip the check", async () => {
    const f = createVerifiedFetch(fakeFetch({ [`${BASE}x/../onnx/m.onnx`]: model }), {
      baseUrl: BASE,
      sha256: { "onnx/m.onnx": "0".repeat(64) },
      onBegin: () => {},
      onBytes: () => {},
    });
    const res = await f(`${BASE}x/../onnx/m.onnx`);
    await expect(res.arrayBuffer()).rejects.toThrow(/checksum/i);
  });

  it("does not report progress for error responses", async () => {
    const begin = vi.fn();
    const f = createVerifiedFetch(fakeFetch({}), { baseUrl: BASE, sha256: {}, onBegin: begin, onBytes: () => {} });
    expect((await f(`${BASE}nope.json`)).status).toBe(404);
    expect(begin).not.toHaveBeenCalled();
  });
});

// --- small pure helpers ------------------------------------------------------------------------

describe("helpers", () => {
  it("splits a model base URL into the host and path transformers.js expects", () => {
    expect(splitBaseUrl("https://huggingface.co/onnx-community/m/resolve/abc/")).toEqual({
      host: "https://huggingface.co/",
      path: "onnx-community/m/resolve/abc/",
    });
    expect(splitBaseUrl("https://cdn.example.com/models/moonshine")).toEqual({
      host: "https://cdn.example.com/",
      path: "models/moonshine/",
    });
  });

  it("pins the default model to a commit sha and checksums both model files per device", () => {
    expect(DEFAULT_REVISION).toMatch(/^[0-9a-f]{40}$/);
    expect(defaultModelBaseUrl(DEFAULT_REVISION)).toBe(
      `https://huggingface.co/onnx-community/moonshine-tiny-ONNX/resolve/${DEFAULT_REVISION}/`,
    );
    for (const f of [
      "onnx/encoder_model_quantized.onnx",
      "onnx/decoder_model_merged_quantized.onnx",
      "onnx/encoder_model.onnx",
      "onnx/decoder_model_merged_q4.onnx",
    ]) {
      expect(DEFAULT_SHA256[f]).toMatch(/^[0-9a-f]{64}$/);
    }
  });

  it("allows enough tokens for utterances under a second", () => {
    expect(maxNewTokens(8000, 16000)).toBeGreaterThanOrEqual(6);
    expect(maxNewTokens(16000 * 10, 16000)).toBe(60);
  });

  it("joins PCM16 chunks into normalized float samples", () => {
    const out = pcm16ToFloat32([Int16Array.of(0, 16384), Int16Array.of(-32768)]);
    expect(Array.from(out)).toEqual([0, 0.5, -1]);
  });
});

// --- the HostProvider, with a fake worker ------------------------------------------------------

class FakeWorker {
  static last: FakeWorker | undefined;
  sent: ToWorker[] = [];
  onmessage: ((e: MessageEvent) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  terminated = false;
  constructor(
    public url: URL | string,
    public opts?: WorkerOptions,
  ) {
    FakeWorker.last = this;
  }
  postMessage(msg: ToWorker) {
    this.sent.push(msg);
  }
  terminate() {
    this.terminated = true;
  }
  emit(data: unknown) {
    this.onmessage?.({ data } as MessageEvent);
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("createLocalProvider", () => {
  beforeEach(() => {
    FakeWorker.last = undefined;
    vi.stubGlobal("Worker", FakeWorker);
    vi.stubGlobal("location", { href: "https://app.example/page" });
    vi.stubGlobal("navigator", {});
  });
  afterEach(() => vi.unstubAllGlobals());

  it("is a batch 16 kHz HostProvider", () => {
    const p: HostProvider = createLocalProvider();
    expect(p.capabilities).toEqual({ streaming: false, sampleRate: 16000 });
    expect(p.id).toBe("local");
  });

  it("loads in a module worker on WASM without WebGPU, reporting progress and ready", async () => {
    const p = createLocalProvider();
    const progress: number[] = [];
    const ready = vi.fn();
    p.on("progress", (n) => progress.push(n));
    p.on("ready", ready);
    const loaded = p.load();
    await flush();
    const w = FakeWorker.last!;
    expect(w.opts).toEqual({ type: "module" });
    expect(w.sent[0]).toMatchObject({
      type: "load",
      device: "wasm",
      modelBaseUrl: defaultModelBaseUrl(DEFAULT_REVISION),
      sha256: DEFAULT_SHA256,
    });
    w.emit({ type: "progress", percent: 0 });
    w.emit({ type: "progress", percent: 100 });
    w.emit({ type: "ready", device: "wasm" });
    await loaded;
    expect(progress).toEqual([0, 100]);
    expect(ready).toHaveBeenCalledWith({ device: "wasm" });
    expect(p.load()).toBe(loaded); // memoized: one worker, one model
  });

  it("resolves a relative modelBaseUrl against the page", async () => {
    const p = createLocalProvider({ modelBaseUrl: "/models/moonshine/", sha256: { "onnx/x.onnx": "a".repeat(64) } });
    void p.load();
    await flush();
    expect(FakeWorker.last!.sent[0]).toMatchObject({
      modelBaseUrl: "https://app.example/models/moonshine/",
      sha256: { "onnx/x.onnx": "a".repeat(64) },
    });
  });

  it("requires checksums when the revision is overridden", () => {
    expect(() => createLocalProvider({ revision: "f".repeat(40) })).toThrow(/sha256/);
    expect(() => createLocalProvider({ revision: "f".repeat(40), sha256: {} })).not.toThrow();
  });

  async function readyProvider() {
    const p = createLocalProvider();
    const results: HostProviderResult[] = [];
    p.onResult((r) => results.push(r));
    const loaded = p.load();
    await flush();
    const w = FakeWorker.last!;
    w.emit({ type: "ready", device: "wasm" });
    await loaded;
    return { p, w, results };
  }

  it("transcribes the whole utterance on finish and reports one final", async () => {
    const { p, w, results } = await readyProvider();
    await p.start(16000);
    await p.pushAudio(Int16Array.of(0, 16384));
    await p.pushAudio(Int16Array.of(-32768));
    const done = p.finish();
    await flush();
    const msg = w.sent.at(-1) as Extract<ToWorker, { type: "transcribe" }>;
    expect(msg.type).toBe("transcribe");
    expect(Array.from(msg.audio)).toEqual([0, 0.5, -1]);
    w.emit({ type: "result", id: msg.id, text: "  Hello world. " });
    await done;
    expect(results).toEqual([{ type: "final", text: "Hello world." }]);
  });

  it("maps an inference error to ProviderDown", async () => {
    const { p, w, results } = await readyProvider();
    await p.start(16000);
    await p.pushAudio(Int16Array.of(1, 2, 3));
    const done = p.finish();
    await flush();
    const msg = w.sent.at(-1) as Extract<ToWorker, { type: "transcribe" }>;
    w.emit({ type: "transcribe-error", id: msg.id, message: "boom" });
    await done;
    expect(results).toEqual([{ type: "error", code: "ProviderDown" }]);
  });

  it("maps a model load failure to ProviderDown and an error event", async () => {
    const p = createLocalProvider();
    const results: HostProviderResult[] = [];
    const errors: Error[] = [];
    p.onResult((r) => results.push(r));
    p.on("error", (e) => errors.push(e));
    await p.start(16000);
    await p.pushAudio(Int16Array.of(1));
    const done = p.finish();
    await flush();
    FakeWorker.last!.emit({ type: "load-error", message: "checksum mismatch for onnx/encoder_model_quantized.onnx" });
    await done;
    expect(results).toEqual([{ type: "error", code: "ProviderDown" }]);
    expect(errors[0]?.message).toMatch(/checksum/);
  });

  it("maps a worker script failure to ProviderDown", async () => {
    const p = createLocalProvider();
    const results: HostProviderResult[] = [];
    p.onResult((r) => results.push(r));
    await p.start(16000);
    const done = p.finish();
    await flush();
    FakeWorker.last!.onerror?.({ message: "SyntaxError" } as ErrorEvent);
    await done;
    expect(results).toEqual([{ type: "error", code: "ProviderDown" }]);
  });

  it("drops the result of a cancelled utterance", async () => {
    const { p, w, results } = await readyProvider();
    await p.start(16000);
    await p.pushAudio(Int16Array.of(1));
    const done = p.finish();
    await flush();
    const msg = w.sent.at(-1) as Extract<ToWorker, { type: "transcribe" }>;
    await p.cancel?.();
    w.emit({ type: "result", id: msg.id, text: "stale" });
    await done;
    expect(results).toEqual([]);
  });

  it("a model load that never finishes times out, reports ProviderDown, and a later utterance retries", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      const p = createLocalProvider();
      const results: HostProviderResult[] = [];
      const errors: Error[] = [];
      p.onResult((r) => results.push(r));
      p.on("error", (e) => errors.push(e));
      await p.start(16000);
      await p.pushAudio(Int16Array.of(1));
      const done = p.finish();
      await vi.advanceTimersByTimeAsync(0);
      const stalled = FakeWorker.last!;
      await vi.advanceTimersByTimeAsync(119_000);
      expect(results).toEqual([]);
      await vi.advanceTimersByTimeAsync(1_000);
      await done;
      expect(results).toEqual([{ type: "error", code: "ProviderDown" }]);
      expect(errors[0]?.message).toMatch(/timed out/i);
      expect(stalled.terminated).toBe(true);

      // The next utterance starts a fresh worker and load.
      await p.start(16000);
      await vi.advanceTimersByTimeAsync(0);
      expect(FakeWorker.last).not.toBe(stalled);
    } finally {
      vi.useRealTimers();
    }
  });

  it("inference that never answers times out with ProviderDown and the next utterance gets a fresh worker", async () => {
    const { p, w, results } = await readyProvider();
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      await p.start(16000);
      await p.pushAudio(Int16Array.of(1));
      const done = p.finish();
      await vi.advanceTimersByTimeAsync(29_000);
      expect(results).toEqual([]);
      await vi.advanceTimersByTimeAsync(1_000);
      await done;
      expect(results).toEqual([{ type: "error", code: "ProviderDown" }]);
      expect(w.terminated).toBe(true);

      await p.start(16000);
      await vi.advanceTimersByTimeAsync(0);
      expect(FakeWorker.last).not.toBe(w);
    } finally {
      vi.useRealTimers();
    }
  });

  it("rejects a sample rate other than the declared 16 kHz", () => {
    const p = createLocalProvider();
    expect(() => p.start(48000)).toThrow(/16000/);
  });

  it("stops delivering results after unsubscribe", async () => {
    const { p, w } = await readyProvider();
    const seen: HostProviderResult[] = [];
    const off = p.onResult((r) => seen.push(r));
    if (typeof off === "function") off();
    await p.start(16000);
    const done = p.finish();
    await flush();
    const msg = w.sent.at(-1) as Extract<ToWorker, { type: "transcribe" }>;
    w.emit({ type: "result", id: msg.id, text: "hi" });
    await done;
    expect(seen).toEqual([]);
  });
});
