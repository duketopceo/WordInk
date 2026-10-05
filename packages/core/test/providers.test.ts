import { describe, expect, it, vi } from "vitest";
import { createDictation, type HostProvider, type HostProviderResult } from "../src/index.js";
import { FakeWebSocket, installFakeAudio, record } from "./fakes.js";

/** A fake host provider (the shape `@wordink/local` implements) that records what it receives. */
function fakeHostProvider(streaming: boolean, sampleRate = 16_000) {
  let emit: ((r: HostProviderResult) => void) | undefined;
  const calls: string[] = [];
  const chunks: Int16Array[] = [];
  const provider: HostProvider = {
    id: "fake-local",
    capabilities: { streaming, sampleRate },
    start: vi.fn((rate: number, hint?: string) => {
      calls.push(`start:${rate}:${hint ?? ""}`);
    }),
    pushAudio: vi.fn((pcm: Int16Array) => {
      chunks.push(pcm);
    }),
    finish: vi.fn(() => {
      calls.push("finish");
    }),
    cancel: vi.fn(() => {
      calls.push("cancel");
    }),
    onResult(cb) {
      emit = cb;
      return () => (emit = undefined);
    },
  };
  const samples = () => chunks.reduce((n, c) => n + c.length, 0);
  return { provider, calls, chunks, samples, result: (r: HostProviderResult) => emit?.(r) };
}

describe("HostProvider (KTD5)", () => {
  it("a streaming host provider receives resampled audio while listening, and its final ends the session", async () => {
    const audio = installFakeAudio({ sampleRate: 48_000 });
    const fake = fakeHostProvider(true);
    const d = createDictation({ provider: fake.provider, hint: "Omarchy" });
    const log = record(d);

    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    expect(fake.calls).toEqual(["start:16000:Omarchy"]);

    audio.speak(0.5); // 24 000 samples at 48 kHz
    // Delivered while speaking, at the provider's 16 kHz: about a third as many samples.
    expect(fake.samples()).toBeGreaterThan(7_000);
    expect(fake.samples()).toBeLessThanOrEqual(8_000);
    expect(fake.chunks.every((c) => c instanceof Int16Array)).toBe(true);
    expect(Math.max(...fake.chunks.flatMap((c) => Array.from(c)))).toBeGreaterThan(8_000); // ~0.3 * 32767

    fake.result({ type: "interim", text: "hello" });
    await d.stop();
    expect(fake.calls.at(-1)).toBe("finish");
    expect(fake.samples()).toBeGreaterThan(7_900); // the tail is flushed on release
    expect(d.state).toBe("transcribing");

    fake.result({ type: "final", text: "hello world" });
    expect(log).toEqual([
      "state:requesting-mic",
      "state:listening",
      "interim:hello",
      "state:transcribing",
      "final:hello world",
      "state:idle",
    ]);
    d.destroy();
  });

  it("a batch host provider gets start, the whole utterance and finish on release", async () => {
    const audio = installFakeAudio({ sampleRate: 44_100 });
    const fake = fakeHostProvider(false, 16_000);
    const d = createDictation({ provider: fake.provider });
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    expect(fake.calls).toEqual([]);
    await d.stop();
    expect(fake.calls).toEqual(["start:16000:", "finish"]);
    expect(Math.abs(fake.samples() - 8_000)).toBeLessThanOrEqual(50);
    fake.result({ type: "error", code: "BadAudio" });
    expect(d.state).toBe("error");
    d.destroy();
  });

  it("destroy() during an utterance cancels the host provider", async () => {
    const audio = installFakeAudio();
    const fake = fakeHostProvider(true);
    const d = createDictation({ provider: fake.provider });
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.2);
    d.destroy();
    await vi.waitFor(() => expect(fake.calls).toContain("cancel"));
  });
});

describe("streaming cloud providers through the relay (KTD3, AE3)", () => {
  it("Deepgram: mints a JWT per session and opens the socket with it, never a long-lived key", async () => {
    const audio = installFakeAudio();
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const fetchMock = vi.fn(async (_u: string | URL | Request, _i?: RequestInit) =>
      Response.json({ access_token: `jwt-${fetchMock.mock.calls.length}`, expires_in: 120 }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const d = createDictation({ provider: "deepgram", endpoint: "https://app.test/relay" });
    const log = record(d);

    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1));
    const ws = FakeWebSocket.instances[0]!;
    expect(fetchMock.mock.calls[0]![0]).toBe("https://app.test/relay/deepgram/token");
    expect(ws.url).toMatch(/^wss:\/\/api\.deepgram\.com\/v1\/listen\?/);
    expect(ws.protocols).toEqual(["token", "jwt-1"]);
    expect(ws.binaryType).toBe("arraybuffer");

    ws.serverOpen();
    audio.speak(0.5);
    expect(ws.sent.some((m) => m instanceof Uint8Array && m.byteLength > 0)).toBe(true);
    ws.serverMessage(
      JSON.stringify({ type: "Results", is_final: false, channel: { alternatives: [{ transcript: "hello" }] } }),
    );
    await d.stop();
    expect(ws.sent.at(-1)).toBe('{"type":"CloseStream"}');
    ws.serverMessage(
      JSON.stringify({ type: "Results", is_final: true, channel: { alternatives: [{ transcript: "hello world" }] } }),
    );
    ws.serverClose(1000);
    expect(log).toEqual([
      "state:requesting-mic",
      "state:listening",
      "interim:hello",
      "state:transcribing",
      "interim:hello world",
      "final:hello world",
      "state:idle",
    ]);

    // The next utterance gets a fresh token and a fresh socket.
    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2));
    expect(FakeWebSocket.instances[1]!.protocols).toEqual(["token", "jwt-2"]);
    d.destroy();
  });

  it("OpenAI: a failed token mint surfaces as an error without opening the microphone", async () => {
    const audio = installFakeAudio();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({ error: "forbidden" }, { status: 403 })));
    const d = createDictation({ provider: "openai", endpoint: "https://app.test/relay" });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    expect(d.state).toBe("error");
    expect(errors).toEqual(["AuthFailed"]);
    expect(audio.getUserMedia).not.toHaveBeenCalled();
    d.destroy();
  });
});

describe("watchdogs: a stalled provider ends in an error the user can retry from", () => {
  function deepgram() {
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({ access_token: "jwt", expires_in: 120 })));
    return createDictation({ provider: "deepgram", endpoint: "https://app.test/relay" });
  }

  it("an open socket that never delivers a final fails with ProviderDown after the transcribing watchdog", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const audio = installFakeAudio();
    const d = deepgram();
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1));
    const ws = FakeWebSocket.instances[0]!;
    ws.serverOpen();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    await d.stop();
    expect(d.state).toBe("transcribing");

    await vi.advanceTimersByTimeAsync(29_000);
    expect(d.state).toBe("transcribing");
    await vi.advanceTimersByTimeAsync(1_000);
    expect(d.state).toBe("error");
    expect(errors).toEqual(["ProviderDown"]);
    expect(ws.readyState).toBe(FakeWebSocket.CLOSED);

    // The user can retry: a fresh socket opens.
    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2));
    d.destroy();
  });

  it("a socket that never opens is reported failed after the connect timeout", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    installFakeAudio();
    const d = deepgram();
    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1));
    // Each subprotocol candidate gets its own connect timeout; none ever opens.
    await vi.advanceTimersByTimeAsync(10_000);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(d.state).toBe("error");
    expect(FakeWebSocket.instances.length).toBeGreaterThan(0);
    expect(FakeWebSocket.instances.every((w) => w.readyState === FakeWebSocket.CLOSED)).toBe(true);
    d.destroy();
  });

  it("a host provider that never reports a result fails with ProviderDown and is cancelled", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const audio = installFakeAudio();
    const fake = fakeHostProvider(false);
    const d = createDictation({ provider: fake.provider });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    await d.stop();
    expect(d.state).toBe("transcribing");
    await vi.advanceTimersByTimeAsync(30_000);
    expect(d.state).toBe("error");
    expect(errors).toEqual(["ProviderDown"]);
    expect(fake.calls.at(-1)).toBe("cancel");

    // A late result is ignored, and the next utterance works.
    fake.result({ type: "final", text: "stale" });
    expect(d.state).toBe("error");
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    d.destroy();
  });

  it("a host provider that declares a longer timeoutMs is given that long", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const audio = installFakeAudio();
    const fake = fakeHostProvider(false);
    (fake.provider.capabilities as { timeoutMs?: number }).timeoutMs = 150_000;
    const d = createDictation({ provider: fake.provider });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    await d.stop();
    // A first-use model download can outlast the default 30 s watchdog.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(d.state).toBe("transcribing");
    fake.result({ type: "final", text: "slow but fine" });
    expect(d.state).toBe("idle");
    expect(errors).toEqual([]);
    d.destroy();
  });

  it("the watchdog is cleared when the utterance completes", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const audio = installFakeAudio();
    const fake = fakeHostProvider(false);
    const d = createDictation({ provider: fake.provider });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    await d.stop();
    fake.result({ type: "final", text: "done" });
    expect(d.state).toBe("idle");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(d.state).toBe("idle");
    expect(errors).toEqual([]);
    d.destroy();
  });
});

describe("token mint timeout (relay)", () => {
  /** A relay that accepts the request and never answers, until the request is aborted. */
  function hungRelay() {
    const signals: AbortSignal[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(
        (_u: string | URL | Request, init?: RequestInit) =>
          new Promise<Response>((_, reject) => {
            const signal = init?.signal;
            if (!signal) return;
            signals.push(signal);
            signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
          }),
      ),
    );
    return signals;
  }

  it("a hung mint is aborted after the HTTP timeout and ends in ProviderDown", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    installFakeAudio();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const signals = hungRelay();
    const d = createDictation({ provider: "openai", endpoint: "https://app.test/relay" });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    const started = d.start();
    await vi.waitFor(() => expect(signals).toHaveLength(1));
    expect(d.state).toBe("requesting-mic");
    await vi.advanceTimersByTimeAsync(30_000);
    await started;
    expect(signals[0]!.aborted).toBe(true);
    expect(d.state).toBe("error");
    expect(errors).toEqual(["ProviderDown"]);
    d.destroy();
  });

  it("destroy() aborts a pending mint", async () => {
    installFakeAudio();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const signals = hungRelay();
    const d = createDictation({ provider: "deepgram", endpoint: "https://app.test/relay" });
    const started = d.start();
    await vi.waitFor(() => expect(signals).toHaveLength(1));
    d.destroy();
    expect(signals[0]!.aborted).toBe(true);
    await started;
  });
});
