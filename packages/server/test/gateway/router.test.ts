import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AudioFile, ResolvedEntry } from "../../src/gateway/providers.js";
import { createRouter, type RouterOptions } from "../../src/gateway/router.js";
import { hang, makeUpstream, ok, statusWith, type FetchMock, type Responder } from "./fakes.js";

const KEY_A = "gsk_test_KEY_A_0123456789";
const KEY_B = "gsk_test_KEY_B_0123456789";
const KEY_C = "sk-test-KEY_C_0123456789";

const A: ResolvedEntry = { provider: "groq", key: KEY_A };
const B: ResolvedEntry = { provider: "groq", key: KEY_B };
const C: ResolvedEntry = { provider: "openai", key: KEY_C };

const audio: AudioFile = { bytes: new Uint8Array([1, 2, 3]), type: "audio/wav", name: "audio.wav" };

const status = statusWith(KEY_A);

let fetchMock: FetchMock;
let on: (key: string, ...rs: Responder[]) => void;
let keysCalled: () => string[];
let clock: number;
let errorSpy: ReturnType<typeof vi.spyOn>;

function router(entries: ResolvedEntry[], options: RouterOptions = {}) {
  return createRouter(entries, { now: () => clock, ...options });
}

beforeEach(() => {
  clock = 1_000_000;
  ({ fetchMock, on, keysCalled } = makeUpstream());
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

  it("tries an entry whose cooldown expired mid-request instead of answering 502", async () => {
    on(KEY_A, status(429, { "retry-after": "4" }), ok("from A"));
    on(
      KEY_B,
      status(400),
      () => {
        clock += 5_000; // this attempt spans A's cooldown expiry
        return status(500)(undefined);
      },
    );
    const r = router([A, B]);
    // Request 1: A 429s (cooling 4 s); B 400s — a client error, so B never cools.
    expect(await r.transcribe(audio, {})).toMatchObject({ ok: false, status: 400 });
    // Request 2: only B is ready at request start; its attempt advances the clock
    // past A's expiry, and A is tried inside the same request.
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from A" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_B, KEY_A]);
  });

  it("an attempt clipped by the shared deadline does not cool the entry", async () => {
    vi.useFakeTimers();
    on(KEY_A, hang);
    on(KEY_B, () => new Promise((resolve) => setTimeout(() => resolve(status(500)(undefined)), 14_000)));
    on(KEY_C, hang);
    const r = router([A, B, C], { now: () => Date.now(), deadlineMs: 30_000 });
    const pending = r.transcribe(audio, {});
    // A hangs its full 15 s budget; B fails at 14 s; C gets the ~1 s left and is clipped.
    await vi.advanceTimersByTimeAsync(30_000);
    expect(await pending).toMatchObject({ ok: false, status: 504 });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_C]);

    // C proved nothing in 1 s, so it is not cooling: it serves the next request while
    // A and B still cool.
    on(KEY_C, ok("from C"));
    expect(await r.transcribe(audio, {})).toEqual({ ok: true, text: "from C" });
    expect(keysCalled()).toEqual([KEY_A, KEY_B, KEY_C, KEY_C]);
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
