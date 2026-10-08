import { describe, expect, it, vi } from "vitest";
import { createDictation, type Dictation, WordInkError } from "../src/index.js";
import { FakeWebSocket, installFakeAudio, record } from "./fakes.js";

const ENDPOINT = "https://app.test/api/wordink";

function relayReturning(body: string | (() => Promise<Response>), status = 200) {
  const fetchMock = vi.fn(async (_url: string | URL | Request, _init?: RequestInit) =>
    typeof body === "string" ? new Response(body, { status }) : body(),
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

/** Holds to talk for `seconds` of speech, then releases. */
async function dictate(d: Dictation, audio: ReturnType<typeof installFakeAudio>, seconds = 0.6) {
  await d.start();
  await vi.waitFor(() => expect(d.state).toBe("listening"));
  audio.speak(seconds);
  await d.stop();
}

describe("createDictation with the Groq relay", () => {
  it("runs a full start/stop cycle: listening, transcribing, then final, in order", async () => {
    const audio = installFakeAudio();
    const fetchMock = relayReturning("hello world\n");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    const log = record(d);
    const levels: number[] = [];
    d.on("level", (l) => levels.push(l));

    await dictate(d, audio);
    await vi.waitFor(() => expect(d.state).toBe("idle"));

    expect(log).toEqual([
      "state:requesting-mic",
      "state:listening",
      "state:transcribing",
      "final:hello world",
      "state:idle",
    ]);
    // ~20 level events per second of audio, at the tone's RMS (0.3 / sqrt 2).
    expect(levels.length).toBeGreaterThanOrEqual(10);
    expect(levels[0]).toBeCloseTo(0.212, 2);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe(`${ENDPOINT}/groq/transcriptions`);
    expect(init?.method).toBe("POST");
    expect(init?.credentials).toBe("include");
    const form = init?.body as FormData;
    expect(form.get("model")).toBe("whisper-large-v3-turbo");
    expect(form.get("response_format")).toBe("text");
    const file = form.get("file") as File;
    expect(file.name).toBe("audio.wav");
    expect(file.type).toBe("audio/wav");
    const wav = new DataView(await file.arrayBuffer());
    expect(String.fromCharCode(wav.getUint8(0), wav.getUint8(1), wav.getUint8(2), wav.getUint8(3))).toBe("RIFF");
    expect(wav.getUint32(24, true)).toBe(16_000);
    // The microphone is released when listening ends.
    expect(audio.tracks.every((t) => t.stopped)).toBe(true);
    d.destroy();
  });

  it("passes hint and model to the provider request", async () => {
    const audio = installFakeAudio();
    const fetchMock = relayReturning("ok");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT, hint: "WordInk, Omarchy", model: "whisper-large-v3" });
    await dictate(d, audio);
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const form = fetchMock.mock.calls[0]![1]?.body as FormData;
    expect(form.get("prompt")).toBe("WordInk, Omarchy");
    expect(form.get("model")).toBe("whisper-large-v3");
    d.destroy();
  });

  it("toggle mode: press starts and a second press stops", async () => {
    const audio = installFakeAudio();
    relayReturning("toggled");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT, mode: "toggle" });
    const log = record(d);
    await d.press();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    await d.release(); // ignored in toggle mode
    expect(d.state).toBe("listening");
    await d.press();
    await vi.waitFor(() => expect(d.state).toBe("idle"));
    expect(log).toContain("final:toggled");
    d.destroy();
  });

  it("maps a relay 401 to an AuthFailed error", async () => {
    const audio = installFakeAudio();
    relayReturning('{"error":"forbidden"}', 401);
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    const errors: WordInkError[] = [];
    d.on("error", (e) => errors.push(e));
    await dictate(d, audio);
    await vi.waitFor(() => expect(d.state).toBe("error"));
    expect(errors.map((e) => e.code)).toEqual(["AuthFailed"]);
    d.destroy();
  });
});

describe("transform hook (KTD11)", () => {
  it("emits the transformed text when transform resolves", async () => {
    const audio = installFakeAudio();
    relayReturning("hello world");
    const transform = vi.fn(async (t: string) => t.toUpperCase() + ".");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT, transform });
    const log = record(d);
    await dictate(d, audio);
    await vi.waitFor(() => expect(d.state).toBe("idle"));
    expect(transform).toHaveBeenCalledWith("hello world");
    expect(log.filter((l) => !l.startsWith("state:requesting") && !l.startsWith("state:listening"))).toEqual([
      "state:transcribing",
      "final:HELLO WORLD.",
      "state:idle",
    ]);
    d.destroy();
  });

  it("falls back to the raw text plus a warning when transform exceeds the timeout", async () => {
    const audio = installFakeAudio();
    relayReturning("hello world");
    const d = createDictation({
      provider: "groq",
      endpoint: ENDPOINT,
      transform: () => new Promise<string>(() => {}),
      transformTimeoutMs: 50,
    });
    const log = record(d);
    await dictate(d, audio);
    // The session stays "transcribing" while the transform runs.
    await vi.waitFor(() => expect(log).toContain("state:transcribing"));
    expect(d.state).toBe("transcribing");
    await vi.waitFor(() => expect(d.state).toBe("idle"));
    expect(log.slice(-3)).toEqual(["warning:TransformTimeout", "final:hello world", "state:idle"]);
    d.destroy();
  });

  it("falls back to the raw text plus a warning when transform throws", async () => {
    const audio = installFakeAudio();
    relayReturning("hello world");
    const d = createDictation({
      provider: "groq",
      endpoint: ENDPOINT,
      transform: async () => {
        throw new Error("backend down");
      },
    });
    const log = record(d);
    await dictate(d, audio);
    await vi.waitFor(() => expect(d.state).toBe("idle"));
    expect(log.slice(-3)).toEqual(["warning:TransformFailed", "final:hello world", "state:idle"]);
    d.destroy();
  });

  it("ignores a press and release while the transform is pending (still visibly transcribing)", async () => {
    const audio = installFakeAudio();
    relayReturning("hello world");
    let settle: (text: string) => void = () => {};
    const transform = vi.fn(() => new Promise<string>((resolve) => (settle = resolve)));
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT, transform, transformTimeoutMs: 10_000 });
    const log = record(d);
    await dictate(d, audio);
    await vi.waitFor(() => expect(transform).toHaveBeenCalled());
    expect(d.state).toBe("transcribing");

    await d.press();
    await d.release();
    settle("Hello world.");
    await vi.waitFor(() => expect(d.state).toBe("idle"));
    // Let any deferred effects of the ignored press run.
    await new Promise((r) => setTimeout(r, 10));

    expect(audio.getUserMedia).toHaveBeenCalledTimes(1);
    expect(log.slice(-3)).toEqual(["state:transcribing", "final:Hello world.", "state:idle"]);
    expect(d.state).toBe("idle");
    d.destroy();
  });
});

describe("microphone errors (R9, AE2)", () => {
  it("a denied permission yields MicDenied with a message naming the browser setting", async () => {
    const audio = installFakeAudio({ deny: "NotAllowedError" });
    const fetchMock = relayReturning("unused");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    const errors: WordInkError[] = [];
    d.on("error", (e) => errors.push(e));
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("error"));

    expect(audio.getUserMedia).toHaveBeenCalledTimes(1);
    expect(errors).toHaveLength(1);
    const err = errors[0]!;
    expect(err).toBeInstanceOf(WordInkError);
    expect(err.code).toBe("MicDenied");
    expect(err.message).toMatch(/microphone blocked/i);
    // Names the per-browser site-permission setting to change.
    expect(err.hint).toContain("Chrome/Edge: Site settings > Microphone");
    expect(err.hint).toContain("Firefox: Permissions");
    expect(err.hint).toContain("Safari: Settings > Websites > Microphone");
    expect(fetchMock).not.toHaveBeenCalled();
    d.destroy();
  });

  it("a missing microphone says so instead of blaming permissions", async () => {
    installFakeAudio({ deny: "NotFoundError" });
    relayReturning("unused");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    const errors: WordInkError[] = [];
    d.on("error", (e) => errors.push(e));
    await d.start();
    await vi.waitFor(() => expect(errors).toHaveLength(1));
    expect(errors[0]!.code).toBe("MicDenied");
    expect(errors[0]!.hint).toMatch(/no microphone was found/i);
    d.destroy();
  });

  it("releasing before any speech ends in NoSpeech without calling the provider", async () => {
    const audio = installFakeAudio();
    const fetchMock = relayReturning("unused");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.6, 0); // silence
    await d.stop();
    expect(errors).toEqual(["NoSpeech"]);
    expect(fetchMock).not.toHaveBeenCalled();
    d.destroy();
  });
});

describe("socket connect failures (R9, AE3)", () => {
  /** A relay that always mints a token, and a dictation wired to Deepgram's two-candidate socket. */
  function deepgram() {
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({ access_token: "jwt", expires_in: 120 })));
    return createDictation({ provider: "deepgram", endpoint: "https://app.test/relay" });
  }

  it("a socket that never opens before the connect timeout ends in ProviderDown, not AuthFailed", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    installFakeAudio();
    const d = deepgram();
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1));

    // A stall feeds the private stall code; for a close before open only that code
    // reaches ProviderDown — a real rejection (fed as 1006) stays AuthFailed.
    await vi.advanceTimersByTimeAsync(10_000);
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2)); // the next candidate still retries
    await vi.advanceTimersByTimeAsync(10_000);
    await vi.waitFor(() => expect(d.state).toBe("error"));
    expect(errors).toEqual(["ProviderDown"]);
    d.destroy();
  });

  it("a socket whose onclose fires with 1006 before open still ends in AuthFailed", async () => {
    installFakeAudio();
    const d = deepgram();
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(1));
    FakeWebSocket.instances[0]!.serverClose(1006); // a rejected handshake looks like this
    await vi.waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2));
    FakeWebSocket.instances[1]!.serverClose(1006);
    await vi.waitFor(() => expect(d.state).toBe("error"));
    expect(errors).toEqual(["AuthFailed"]);
    d.destroy();
  });
});

describe("lifecycle", () => {
  it("destroy() while listening releases the microphone and emits nothing more", async () => {
    const audio = installFakeAudio();
    relayReturning("unused");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    const log = record(d);
    d.destroy();
    expect(audio.tracks.every((t) => t.stopped)).toBe(true);
    await d.stop();
    expect(log).toEqual([]);
  });

  it("release while the mic grant is still pending closes the late grant, and the next press works", async () => {
    // Blur during a pending start (wasm load, token mint or the permission prompt) must not leave
    // the mic open: the release lands on RequestingMic, and a getUserMedia that resolves afterwards
    // hits the micGeneration guard (R11).
    const audio = installFakeAudio();
    const realImpl = audio.getUserMedia.getMockImplementation()!;
    let grant: ((stream: MediaStream) => void) | undefined;
    audio.getUserMedia.mockImplementation(() => new Promise<MediaStream>((resolve) => (grant = resolve)));
    relayReturning("unused");
    const d = createDictation({ provider: "groq", endpoint: ENDPOINT });
    const log = record(d);

    void d.press();
    await vi.waitFor(() => expect(audio.getUserMedia).toHaveBeenCalled());
    await d.release(); // lands on RequestingMic: stop-mic + NoSpeech
    await vi.waitFor(() => expect(d.state).toBe("error"));
    expect(log).toContain("error:NoSpeech");

    // The prompt resolves after the release: the generation guard closes it at once.
    const track = { stopped: false, stop() { this.stopped = true; } };
    grant!({ getTracks: () => [track] } as unknown as MediaStream);
    await vi.waitFor(() => expect(track.stopped).toBe(true));

    // A new press still starts cleanly on the same dictation.
    audio.getUserMedia.mockImplementation(realImpl);
    await d.press();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    d.destroy();
  });

  it("rejects invalid options with a Config error", () => {
    expect(() => createDictation({ provider: "whisper" as "groq", endpoint: ENDPOINT })).toThrow(WordInkError);
    expect(() => createDictation({ provider: "groq", endpoint: ENDPOINT, mode: "push" as "hold" })).toThrow(/mode/);
    expect(() => createDictation({ provider: "groq", endpoint: ENDPOINT, transformTimeoutMs: 0 })).toThrow(/transformTimeoutMs/);
  });
});
