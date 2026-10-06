import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEEPGRAM_LISTEN_URL,
  GROQ_AUDIO_URL,
  OPENAI_AUDIO_URL,
  transcribe,
  type AudioFile,
  type ResolvedEntry,
} from "../../src/gateway/providers.js";
import { cleanLanguage, parseTranscriptionForm, type ParseResult } from "../../src/gateway/multipart.js";
import { sentForm, upstreamCall, type FetchMock } from "./fakes.js";

const GROQ_KEY = "gsk_test_GROQ_SECRET_0123456789";
const OPENAI_KEY = "sk-test-OPENAI_SECRET_0123456789";
const DEEPGRAM_KEY = "dg_test_DEEPGRAM_SECRET_0123456789";

const groq: ResolvedEntry = { provider: "groq", key: GROQ_KEY };
const openai: ResolvedEntry = { provider: "openai", key: OPENAI_KEY };
const deepgram: ResolvedEntry = { provider: "deepgram", key: DEEPGRAM_KEY };

const audio: AudioFile = { bytes: new Uint8Array([1, 2, 3, 4, 5]), type: "audio/wav", name: "audio.wav" };

let fetchMock: FetchMock;

function call(i = 0): { url: string; init: RequestInit; headers: Headers } {
  return upstreamCall(fetchMock, i);
}

beforeEach(() => {
  fetchMock = vi.fn<typeof fetch>(async () => Response.json({ text: "hello world" }));
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("Groq", () => {
  it("sends the bearer key, whisper-large-v3-turbo, the merged prompt and the file, and returns the text", async () => {
    const res = await transcribe(groq, audio, {
      prompt: "Talking about Linux.",
      vocabulary: ["Omarchy"],
      language: "en",
      temperature: 0.2,
    });
    expect(res).toEqual({ ok: true, text: "hello world" });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const { url, init, headers } = call();
    expect(url).toBe(GROQ_AUDIO_URL);
    expect(url).toBe("https://api.groq.com/openai/v1/audio/transcriptions");
    expect(init.method).toBe("POST");
    expect(headers.get("authorization")).toBe(`Bearer ${GROQ_KEY}`);

    const form = sentForm(fetchMock);
    expect(form.get("model")).toBe("whisper-large-v3-turbo");
    expect(form.get("prompt")).toBe("Omarchy. Talking about Linux.");
    expect(form.get("language")).toBe("en");
    expect(form.get("temperature")).toBe("0.2");
    expect(form.get("response_format")).toBe("json");
    const file = form.get("file") as File;
    expect(file.name).toBe("audio.wav");
    expect(file.type).toBe("audio/wav");
    expect(new Uint8Array(await file.arrayBuffer())).toEqual(audio.bytes);
  });

  it("uses the entry's own model when configured, whatever the client asked for (R3)", async () => {
    await transcribe({ ...groq, model: "whisper-large-v3" }, audio, {});
    expect(sentForm(fetchMock).get("model")).toBe("whisper-large-v3");
  });

  it("omits prompt and language when there is nothing to send, and drops an invalid language", async () => {
    await transcribe(groq, audio, { language: "en&model=x" });
    const form = sentForm(fetchMock);
    expect(form.has("prompt")).toBe(false);
    expect(form.has("language")).toBe(false);
    expect(form.has("temperature")).toBe(false);
  });

  it("builds a fresh form for every call", async () => {
    await transcribe(groq, audio, {});
    await transcribe(groq, audio, {});
    expect(sentForm(fetchMock, 0)).not.toBe(sentForm(fetchMock, 1));
  });

  it("normalizes verbose_json to text, language, duration and segments only (R7)", async () => {
    fetchMock.mockResolvedValueOnce(
      Response.json({
        task: "transcribe",
        text: "hello world",
        language: "English",
        duration: 1.5,
        segments: [
          { id: 0, seek: 0, start: 0, end: 1.5, text: " hello world", tokens: [1, 2], avg_logprob: -0.1 },
        ],
        x_groq: { id: "req_123" },
      }),
    );
    const res = await transcribe(groq, audio, { responseFormat: "verbose_json" });
    expect(sentForm(fetchMock).get("response_format")).toBe("verbose_json");
    expect(res).toEqual({
      ok: true,
      text: "hello world",
      verbose: {
        text: "hello world",
        language: "English",
        duration: 1.5,
        segments: [{ id: 0, start: 0, end: 1.5, text: " hello world" }],
      },
    });
  });
});

describe("OpenAI", () => {
  it("json uses gpt-4o-transcribe at the OpenAI endpoint with the bearer key", async () => {
    await transcribe(openai, audio, { responseFormat: "json" });
    const { url, headers } = call();
    expect(url).toBe(OPENAI_AUDIO_URL);
    expect(url).toBe("https://api.openai.com/v1/audio/transcriptions");
    expect(headers.get("authorization")).toBe(`Bearer ${OPENAI_KEY}`);
    expect(sentForm(fetchMock).get("model")).toBe("gpt-4o-transcribe");
    expect(sentForm(fetchMock).get("response_format")).toBe("json");
  });

  it("text is requested as json upstream (the gateway formats it)", async () => {
    const res = await transcribe(openai, audio, { responseFormat: "text" });
    expect(sentForm(fetchMock).get("response_format")).toBe("json");
    expect(res).toEqual({ ok: true, text: "hello world" });
  });

  it("verbose_json switches to whisper-1", async () => {
    fetchMock.mockResolvedValueOnce(Response.json({ text: "hi", language: "english", duration: 2, words: [] }));
    const res = await transcribe(openai, audio, { responseFormat: "verbose_json" });
    expect(sentForm(fetchMock).get("model")).toBe("whisper-1");
    expect(sentForm(fetchMock).get("response_format")).toBe("verbose_json");
    expect(res).toEqual({ ok: true, text: "hi", verbose: { text: "hi", language: "english", duration: 2 } });
  });
});

describe("Deepgram", () => {
  const dgReply = {
    metadata: { request_id: "dg-req-1", duration: 3.25, models: ["nova-3"] },
    results: { channels: [{ alternatives: [{ transcript: "Hello, Omarchy.", confidence: 0.99, words: [] }] }] },
  };

  it("posts raw bytes with the file's content type, Token auth, and keyterm params from the vocabulary", async () => {
    fetchMock.mockResolvedValueOnce(Response.json(dgReply));
    const res = await transcribe(deepgram, audio, {
      vocabulary: ["Omarchy", "Hyprland & Co"],
      prompt: "client prompt is not a Deepgram field",
      language: "en-US",
    });
    expect(res).toEqual({ ok: true, text: "Hello, Omarchy." });

    const { url, init, headers } = call();
    const u = new URL(url);
    expect(`${u.origin}${u.pathname}`).toBe(DEEPGRAM_LISTEN_URL);
    expect(DEEPGRAM_LISTEN_URL).toBe("https://api.deepgram.com/v1/listen");
    expect(u.searchParams.get("model")).toBe("nova-3");
    expect(u.searchParams.get("language")).toBe("en-US");
    expect(u.searchParams.get("smart_format")).toBe("true");
    expect(u.searchParams.getAll("keyterm")).toEqual(["Omarchy", "Hyprland & Co"]);
    expect([...u.searchParams.keys()].sort()).toEqual(["keyterm", "keyterm", "language", "model", "smart_format"]);

    expect(headers.get("authorization")).toBe(`Token ${DEEPGRAM_KEY}`);
    expect(headers.get("content-type")).toBe("audio/wav");
    expect(init.body).toBeInstanceOf(Uint8Array);
    expect(init.body).toEqual(audio.bytes);
  });

  it("sends at most 50 keyterms", async () => {
    fetchMock.mockResolvedValueOnce(Response.json(dgReply));
    await transcribe(deepgram, audio, { vocabulary: Array.from({ length: 70 }, (_, i) => `term${i}`) });
    expect(new URL(call().url).searchParams.getAll("keyterm")).toHaveLength(50);
  });

  it("does not let an injected language add query parameters", async () => {
    fetchMock.mockResolvedValueOnce(Response.json(dgReply));
    await transcribe(deepgram, audio, { language: "en&model=x" });
    const u = new URL(call().url);
    expect(u.searchParams.has("language")).toBe(false);
    expect(u.searchParams.getAll("model")).toEqual(["nova-3"]);
  });

  it("falls back to application/octet-stream when the file has no type", async () => {
    fetchMock.mockResolvedValueOnce(Response.json(dgReply));
    await transcribe(deepgram, { ...audio, type: "" }, {});
    expect(call().headers.get("content-type")).toBe("application/octet-stream");
  });

  it("verbose_json carries duration from metadata and the detected language, not the requested one", async () => {
    const reply = {
      ...dgReply,
      results: { channels: [{ detected_language: "nl", alternatives: [{ transcript: "Hello, Omarchy.", confidence: 0.99, words: [] }] }] },
    };
    fetchMock.mockResolvedValueOnce(Response.json(reply));
    const res = await transcribe(deepgram, audio, { responseFormat: "verbose_json", language: "en" });
    expect(res).toEqual({
      ok: true,
      text: "Hello, Omarchy.",
      verbose: { text: "Hello, Omarchy.", language: "nl", duration: 3.25 },
    });
  });

  it("an unexpected reply shape is a retryable failure", async () => {
    fetchMock.mockResolvedValueOnce(Response.json({ results: {} }));
    const res = await transcribe(deepgram, audio, {});
    expect(res).toMatchObject({ ok: false, kind: "retryable" });
  });
});

describe("upstream failures", () => {
  it("429 with Retry-After: 12 yields retryAfter = 12", async () => {
    fetchMock.mockResolvedValueOnce(new Response("slow down", { status: 429, headers: { "retry-after": "12" } }));
    const res = await transcribe(groq, audio, {});
    expect(res).toEqual({ ok: false, status: 429, kind: "retryable", retryAfter: 12 });
  });

  it("ignores a non-numeric Retry-After", async () => {
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 429, headers: { "retry-after": "soon" } }));
    expect(await transcribe(groq, audio, {})).toEqual({ ok: false, status: 429, kind: "retryable" });
  });

  it.each([500, 503, 401, 403])("HTTP %i is a retryable non-ok result with no body leaked", async (status) => {
    fetchMock.mockResolvedValueOnce(
      Response.json({ error: { message: `Invalid API Key ${GROQ_KEY}` } }, { status }),
    );
    const res = await transcribe(groq, audio, {});
    expect(res).toEqual({ ok: false, status, kind: "retryable" });
    expect(JSON.stringify(res)).not.toContain(GROQ_KEY);
    expect(JSON.stringify(res)).not.toContain("Invalid API Key");
  });

  it.each([400, 413, 415])("HTTP %i is a client failure", async (status) => {
    fetchMock.mockResolvedValueOnce(Response.json({ error: { message: `bad audio ${DEEPGRAM_KEY}` } }, { status }));
    const res = await transcribe(deepgram, audio, {});
    expect(res).toEqual({ ok: false, status, kind: "client" });
  });

  it("a network error is a retryable failure with no status", async () => {
    fetchMock.mockRejectedValueOnce(new TypeError("fetch failed"));
    expect(await transcribe(openai, audio, {})).toEqual({ ok: false, kind: "retryable" });
  });

  it("an aborted signal is a retryable failure", async () => {
    fetchMock.mockImplementationOnce(async (_input, init) => {
      init?.signal?.throwIfAborted();
      return Response.json({ text: "late" });
    });
    const ctrl = new AbortController();
    ctrl.abort(new DOMException("timed out", "TimeoutError"));
    expect(await transcribe(groq, audio, {}, ctrl.signal)).toEqual({ ok: false, kind: "retryable" });
  });

  it("a 200 without a text field is a retryable failure", async () => {
    fetchMock.mockResolvedValueOnce(Response.json({ nope: true }));
    expect(await transcribe(groq, audio, {})).toEqual({ ok: false, status: 200, kind: "retryable" });
  });
});

describe("request parsing (multipart)", () => {
  function form(fields: Record<string, string>, withFile = true): Promise<ParseResult> {
    const fd = new FormData();
    if (withFile) fd.append("file", new Blob([new Uint8Array([9, 8, 7])], { type: "audio/wav" }), "audio.wav");
    for (const [k, v] of Object.entries(fields)) fd.append(k, v);
    const req = new Request("https://x", { method: "POST", body: fd });
    return req.arrayBuffer().then((buf) =>
      parseTranscriptionForm(new Uint8Array(buf), req.headers.get("content-type")),
    );
  }

  it("parses a Voxtype-shaped request", async () => {
    const parsed = await form({ model: "whisper-1", language: "en", prompt: "Omarchy", response_format: "json" });
    expect(parsed).toEqual({
      ok: true,
      form: {
        file: { bytes: new Uint8Array([9, 8, 7]), type: "audio/wav", name: "audio.wav" },
        model: "whisper-1",
        language: "en",
        prompt: "Omarchy",
        responseFormat: "json",
      },
    });
  });

  it("defaults response_format to json and keeps a valid temperature", async () => {
    const parsed = await form({ temperature: "0.3" });
    expect(parsed).toMatchObject({ ok: true, form: { responseFormat: "json", temperature: 0.3 } });
  });

  it("drops an invalid language and an out-of-range temperature", async () => {
    const parsed = await form({ language: "en&model=x", temperature: "7" });
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.form.language).toBeUndefined();
      expect(parsed.form.temperature).toBeUndefined();
    }
  });

  it("reports a missing file, an unsupported response_format and a non-multipart body", async () => {
    expect(await form({}, false)).toEqual({ ok: false, error: "missing_file" });
    expect(await form({ response_format: "srt" })).toEqual({ ok: false, error: "unsupported_response_format" });
    expect(await parseTranscriptionForm(new Uint8Array([1, 2]), "application/json")).toEqual({
      ok: false,
      error: "invalid_form",
    });
  });

  it("validates language tags (KTD9)", () => {
    expect(cleanLanguage("en")).toBe("en");
    expect(cleanLanguage("yue")).toBe("yue");
    expect(cleanLanguage("en-US")).toBe("en-US");
    expect(cleanLanguage("zh-Hant-TW")).toBe("zh-Hant-TW");
    expect(cleanLanguage("e")).toBeUndefined();
    expect(cleanLanguage("english")).toBeUndefined();
    expect(cleanLanguage("en&model=x")).toBeUndefined();
    expect(cleanLanguage("en US")).toBeUndefined();
    expect(cleanLanguage(undefined)).toBeUndefined();
  });
});
