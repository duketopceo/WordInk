import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioFile, ResolvedEntry } from "../../src/gateway/providers.js";
import { createRouter, type RouterOptions } from "../../src/gateway/router.js";

const KEY_A = "gsk_test_KEY_A_0123456789";
const KEY_B = "gsk_test_KEY_B_0123456789";
const KEY_C = "sk-test-KEY_C_0123456789";

const A: ResolvedEntry = { provider: "groq", key: KEY_A };
const B: ResolvedEntry = { provider: "groq", key: KEY_B };
const C: ResolvedEntry = { provider: "openai", key: KEY_C };

const audio: AudioFile = { bytes: new Uint8Array([1, 2, 3]), type: "audio/wav", name: "audio.wav" };

type Responder = (signal: AbortSignal | undefined) => Response | Promise<Response>;
const ok = (text: string): Responder => () => Response.json({ text });
const status =
  (code: number, headers: Record<string, string> = {}): Responder =>
  () =>
    Response.json({ error: { message: `upstream detail with key ${KEY_A}` } }, { status: code, headers });
/** Never answers; rejects with the abort reason like real fetch does. */
const hang: Responder = (signal) =>
  new Promise((_resolve, reject) => {
    signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
  });

let responders: Map<string, Responder[]>;
let fetchMock: ReturnType<typeof vi.fn<typeof fetch>>;
let clock: number;
let errorSpy: ReturnType<typeof vi.spyOn>;

/** Queue responses per key; the last one repeats. */
function on(key: string, ...rs: Responder[]): void {
  responders.set(key, rs);
}

function keysCalled(): string[] {
  return fetchMock.mock.calls.map(([, init]) => {
    const auth = new Headers(init?.headers).get("authorization") ?? "";
    return auth.replace(/^(Bearer|Token) /, "");
  });
}

function router(entries: ResolvedEntry[], options: RouterOptions = {}) {
  return createRouter(entries, { now: () => clock, ...options });
}

beforeEach(() => {
  clock = 1_000_000;
  responders = new Map();
  fetchMock = vi.fn<typeof fetch>(async (_input, init) => {
    const key = (new Headers(init?.headers).get("authorization") ?? "").replace(/^(Bearer|Token) /, "");
    const queue = responders.get(key);
    if (!queue || queue.length === 0) throw new Error(`no responder for ${key}`);
    const r = queue.length > 1 ? queue.shift()! : queue[0]!;
    return r(init?.signal ?? undefined);
  });
  vi.stubGlobal("fetch", fetchMock);
  errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("fallback", () => {
  it("serves from the first entry when it answers", async () => {
    on(KEY_A, ok("from A"));
    on(KEY_B, ok("from B"));
    const res = await router([A, B]).transcribe(audio, {});
    expect(res).toEqual({ ok: true, text: "from A" });
    expect(keysCalled()).toEqual([KEY_A]);
  });

  it("A returns 429: B serves it, and A is skipped on the next request until 30 s pass", async () => {
    on(KEY_A, status(429), ok("from A"));
    on(KEY_B, ok("from B"));
    const r = router([A, B]);

    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B]);

    clock += 29_999;
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_B]);

    clock += 1;
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from A" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_B, KEY_A]);
  });

  it("Retry-After: 120 cools A for 120 s", async () => {
    on(KEY_A, status(429, { "retry-after": "120" }), ok("from A"));
    on(KEY_B, ok("from B"));
    const r = router([A, B]);
    await r.transcribe(audio, {});

    clock += 119_000;
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
    clock += 1_000;
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from A" });
  });

  it("Retry-After: 900 is capped at 300 s", async () => {
    on(KEY_A, status(503, { "retry-after": "900" }), ok("from A"));
    on(KEY_B, ok("from B"));
    const r = router([A, B]);
    await r.transcribe(audio, {});

    clock += 299_000;
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
    clock += 1_000;
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from A" });
  });

  it("A times out (fake timer): B serves", async () => {
    vi.useFakeTimers();
    on(KEY_A, hang);
    on(KEY_B, ok("from B"));
    const pending = router([A, B], { now: () => Date.now() }).transcribe(audio, {});
    await vi.advanceTimersByTimeAsync(14_999);
    expect(keysCalled()).toEqual([KEY_A]);
    await vi.advanceTimersByTimeAsync(1);
    expect(await pending).toEqual({ ok: true, text: "from B" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B]);
  });

  it("honours a configured upstreamTimeoutMs", async () => {
    vi.useFakeTimers();
    on(KEY_A, hang);
    on(KEY_B, ok("from B"));
    const pending = router([A, B], { now: () => Date.now(), upstreamTimeoutMs: 2_000 }).transcribe(audio, {});
    await vi.advanceTimersByTimeAsync(2_000);
    expect(await pending).toEqual({ ok: true, text: "from B" });
  });

  it("a network error falls through", async () => {
    on(KEY_A, () => Promise.reject(new TypeError("fetch failed")));
    on(KEY_B, ok("from B"));
    expect(await router([A, B]).transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
  });

  it.each([401, 403])("A returns %i (bad operator key): it falls through (AE1)", async (code) => {
    on(KEY_A, status(code));
    on(KEY_C, ok("from OpenAI"));
    expect(await router([A, C]).transcribe(audio, {})).toEqual({ ok: true, text: "from OpenAI" });
    expect(keysCalled()).toEqual([KEY_A, KEY_C]);
  });

  it.each([500, 502, 503])("A returns %i: it falls through", async (code) => {
    on(KEY_A, status(code));
    on(KEY_B, ok("from B"));
    expect(await router([A, B]).transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
  });

  it.each([400, 413, 415])("A returns %i: no fall-through; the error is returned", async (code) => {
    on(KEY_A, status(code));
    on(KEY_B, ok("from B"));
    const r = router([A, B]);
    const res = await r.transcribe(audio, {});
    expect(res.ok).toBe(false);
    if (res.ok) return;
    expect(res.status).toBe(code);
    expect(res.body).toEqual({ error: { message: expect.any(String), type: "invalid_request_error" } });
    expect(JSON.stringify(res)).not.toContain(KEY_A);
    expect(JSON.stringify(res)).not.toContain("upstream detail");
    expect(keysCalled()).toEqual([KEY_A]);

    // A client error does not cool the entry down.
    on(KEY_A, ok("from A"));
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from A" });
  });

  it("all entries are cooling down: the earliest-expiring one is tried", async () => {
    on(KEY_A, status(429, { "retry-after": "200" }), ok("from A"));
    on(KEY_B, status(429, { "retry-after": "60" }), ok("from B"));
    const r = router([A, B]);
    const first = await r.transcribe(audio, {});
    expect(first.ok).toBe(false);
    expect(keysCalled()).toEqual([KEY_A, KEY_B]);

    clock += 10_000; // both still cooling; B ends first
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_B]);

    // B's success clears its cooldown; it stays first choice while A cools.
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from B" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_B, KEY_B]);
  });

  it("every entry fails: the client gets 502 upstream_unavailable in the OpenAI shape", async () => {
    on(KEY_A, status(429));
    on(KEY_B, status(500));
    on(KEY_C, status(401));
    const res = await router([A, B, C]).transcribe(audio, {});
    expect(res).toEqual({
      ok: false,
      status: 502,
      body: { error: { message: expect.any(String), type: "upstream_unavailable" } },
    });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_C]);
    const everything = JSON.stringify(res) + JSON.stringify(errorSpy.mock.calls);
    for (const key of [KEY_A, KEY_B, KEY_C]) expect(everything).not.toContain(key);
    expect(everything).not.toContain("upstream detail");
  });

  it("the client-facing error never names a provider (R7)", async () => {
    on(KEY_A, status(429));
    on(KEY_C, status(400));
    const r = router([A, C]);
    const res = await r.transcribe(audio, {});
    expect(JSON.stringify(res).toLowerCase()).not.toMatch(/groq|openai|deepgram/);
  });

  it("the deadline bounds the whole chain: 504 when every provider hangs", async () => {
    vi.useFakeTimers();
    on(KEY_A, hang);
    on(KEY_B, hang);
    on(KEY_C, hang);
    let settled = false;
    const pending = router([A, B, C], { now: () => Date.now() })
      .transcribe(audio, {})
      .finally(() => {
        settled = true;
      });
    await vi.advanceTimersByTimeAsync(29_999);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(await pending).toEqual({
      ok: false,
      status: 504,
      body: { error: { message: expect.any(String), type: "upstream_timeout" } },
    });
    // A got 15 s, B got the remaining 15 s, C was never started.
    expect(keysCalled()).toEqual([KEY_A, KEY_B]);
  });

  it("passes the request options to each attempt", async () => {
    on(KEY_A, status(500));
    on(KEY_B, ok("from B"));
    await router([A, B], { vocabulary: ["Omarchy"] }).transcribe(audio, { language: "en" });
    for (const [, init] of fetchMock.mock.calls) {
      const form = init?.body as FormData;
      expect(form.get("prompt")).toBe("Omarchy");
      expect(form.get("language")).toBe("en");
    }
  });

  it("logs failures by entry and status only", async () => {
    on(KEY_A, status(429));
    on(KEY_B, ok("from B"));
    await router([A, B]).transcribe(audio, {});
    const logged = errorSpy.mock.calls.flat().join(" ");
    expect(logged).toContain("groq#0");
    expect(logged).toContain("429");
    expect(logged).not.toContain(KEY_A);
  });

  it("refuses an empty entry list", () => {
    expect(() => createRouter([])).toThrow();
  });
});
