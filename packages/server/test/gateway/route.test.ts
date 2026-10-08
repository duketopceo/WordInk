import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRelay, type GatewayConfig, type RelayConfig } from "../../src/index.js";
import {
  constantTimeEqualHex,
  generateToken,
  hashToken,
  type TokenRecord,
  type TokenStore,
} from "../../src/gateway/tokens.js";
import {
  DEEPGRAM_LISTEN_URL,
  GROQ_AUDIO_URL,
  OPENAI_AUDIO_URL,
  type ResolvedEntry,
} from "../../src/gateway/providers.js";
import {
  hang,
  makeUpstream,
  ok,
  sentForm,
  statusWith,
  upstreamCall,
  type FetchMock,
  type Responder,
} from "./fakes.js";

const GROQ_KEY_A = "gsk_test_KEY_A_0123456789";
const GROQ_KEY_B = "gsk_test_KEY_B_0123456789";
const OPENAI_KEY = "sk-test_OPENAI_SECRET_0123456789";
const DEEPGRAM_KEY = "dg_test_DEEPGRAM_SECRET_0123456789";

const groqA: ResolvedEntry = { provider: "groq", key: GROQ_KEY_A };
const groqB: ResolvedEntry = { provider: "groq", key: GROQ_KEY_B };
const openai: ResolvedEntry = { provider: "openai", key: OPENAI_KEY };
const deepgram: ResolvedEntry = { provider: "deepgram", key: DEEPGRAM_KEY };

const BASE = "http://127.0.0.1:8941";
const AUDIO = new Uint8Array([7, 7, 7, 7]);

interface Stored extends TokenRecord {
  hash: string;
}

/** In-memory TokenStore for route tests (the file backend is covered by tokens.test.ts). */
class MemoryTokenStore implements TokenStore {
  private readonly records = new Map<string, Stored>();

  async create(label: string) {
    const token = generateToken();
    const rec: Stored = {
      id: crypto.randomUUID(),
      label,
      hash: await hashToken(token),
      createdAt: new Date().toISOString(),
    };
    this.records.set(rec.id, rec);
    return { id: rec.id, label, token };
  }

  async verify(token: string) {
    const hash = await hashToken(token);
    let found: Stored | undefined;
    for (const rec of this.records.values()) {
      if (constantTimeEqualHex(rec.hash, hash)) found = rec;
    }
    return !found || found.revokedAt ? null : { id: found.id, label: found.label };
  }

  async list(): Promise<TokenRecord[]> {
    return [...this.records.values()].map((r) => ({
      id: r.id,
      label: r.label,
      createdAt: r.createdAt,
      ...(r.revokedAt ? { revokedAt: r.revokedAt } : {}),
    }));
  }

  async revoke(id: string) {
    const rec = this.records.get(id);
    if (!rec || rec.revokedAt) return false;
    rec.revokedAt = new Date().toISOString();
    return true;
  }
}

const status = statusWith(GROQ_KEY_A);

let fetchMock: FetchMock;
let on: (key: string, ...rs: Responder[]) => void;
let keysCalled: () => string[];
let errorSpy: ReturnType<typeof vi.spyOn>;
let warnSpy: ReturnType<typeof vi.spyOn>;
let tokenStore: MemoryTokenStore;
let deviceToken: string;
let deviceId: string;

interface PostOptions {
  /** Bearer token; `false` sends no Authorization header at all. Defaults to the valid token. */
  token?: string | false;
  withFile?: boolean;
  fields?: Record<string, string>;
  headers?: Record<string, string>;
  path?: string;
  method?: string;
  /** Replace the multipart body entirely (e.g. a non-multipart body). */
  rawBody?: NonNullable<RequestInit["body"]>;
  rawContentType?: string;
}

/** A Voxtype-shaped request: multipart fields file/model/language/prompt/response_format, Bearer token, no Origin. */
function post(opts: PostOptions = {}): Request {
  let body: NonNullable<RequestInit["body"]>;
  if (opts.rawBody !== undefined) {
    body = opts.rawBody;
  } else {
    const fd = new FormData();
    if (opts.withFile !== false) fd.append("file", new Blob([AUDIO], { type: "audio/wav" }), "audio.wav");
    for (const [k, v] of Object.entries({ model: "whisper-1", response_format: "json", ...opts.fields })) {
      fd.append(k, v);
    }
    body = fd;
  }
  const headers: Record<string, string> = { ...opts.headers };
  if (opts.rawContentType) headers["content-type"] = opts.rawContentType;
  if (opts.token !== false && !("authorization" in headers)) {
    headers.authorization = `Bearer ${opts.token ?? deviceToken}`;
  }
  return new Request(`${BASE}${opts.path ?? "/v1/audio/transcriptions"}`, {
    method: opts.method ?? "POST",
    body,
    headers,
  });
}

/** A device-client GET (model discovery): Bearer token, no Origin, no body. */
function get(path: string, token: string | false = deviceToken): Request {
  const headers: Record<string, string> = {};
  if (token !== false) headers.authorization = `Bearer ${token}`;
  return new Request(`${BASE}${path}`, { headers });
}

function relay(gateway: Partial<GatewayConfig> = {}, config: Partial<RelayConfig> = {}) {
  return createRelay({
    keys: {},
    gateway: { tokenStore, providers: [groqA], ...gateway },
    ...config,
  });
}

async function expectOpenAIError(res: Response, status: number, type?: string, code?: string): Promise<void> {
  expect(res.status).toBe(status);
  const body: unknown = await res.json();
  const error = (body as { error: Record<string, unknown> }).error;
  expect(error.message).toEqual(expect.any(String));
  expect(error.type).toBe(type ?? expect.any(String));
  if (code) expect(error.code).toBe(code);
  expect(Object.keys(body as Record<string, unknown>)).toEqual(["error"]);
}

beforeEach(async () => {
  ({ fetchMock, on, keysCalled } = makeUpstream());
  vi.stubGlobal("fetch", fetchMock);
  errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
  tokenStore = new MemoryTokenStore();
  const made = await tokenStore.create("voxtype-laptop");
  deviceToken = made.token;
  deviceId = made.id;
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("routing", () => {
  it("serves a Voxtype-shaped request: 200 {\"text\"} from mocked Groq, provider model used (R2, AE3)", async () => {
    on(GROQ_KEY_A, ok("hello world"));
    const res = await relay()(
      post({ fields: { model: "whisper-1", language: "en", prompt: "Omarchy", response_format: "json" } }),
    );
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ text: "hello world" });

    const { url, headers } = upstreamCall(fetchMock);
    expect(url).toBe(GROQ_AUDIO_URL);
    expect(headers.get("authorization")).toBe(`Bearer ${GROQ_KEY_A}`);
    const form = sentForm(fetchMock);
    // The client's `model` is accepted but the entry's own model is called (R3).
    expect(form.get("model")).toBe("whisper-large-v3-turbo");
    expect(form.get("language")).toBe("en");
    expect(form.get("prompt")).toBe("Omarchy");
    const file = form.get("file") as File;
    expect(file.name).toBe("audio.wav");
    expect(new Uint8Array(await file.arrayBuffer())).toEqual(AUDIO);
  });

  it("honours basePath", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay({}, { basePath: "/wordink" });
    const res = await handler(post({ path: "/wordink/v1/audio/transcriptions" }));
    expect(res.status).toBe(200);
    expect((await handler(post())).status).toBe(404);
  });

  it("404s the route when no gateway is configured, in the relay's own shape (R11)", async () => {
    const handler = createRelay({ keys: { groq: GROQ_KEY_A }, authorize: () => true });
    const res = await handler(post({ token: false }));
    expect(res.status).toBe(404);
    expect(await res.json()).toEqual({ error: "not_found" });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("405s non-POST methods on the route", async () => {
    const res = await relay()(new Request(`${BASE}/v1/audio/transcriptions`, { method: "OPTIONS" }));
    expect(res.status).toBe(405);
    expect(res.headers.get("allow")).toBe("POST");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("does not run the browser Origin check on the gateway route (KTD9)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay();
    const res = await handler(post({ headers: { origin: "https://evil.example" } }));
    expect(res.status).toBe(200);
  });
});

describe("device-token auth", () => {
  it("missing Bearer gets 401 in the OpenAI shape, no provider call (AE2)", async () => {
    const res = await relay()(post({ token: false }));
    await expectOpenAIError(res, 401, "invalid_request_error", "invalid_api_key");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("malformed Authorization headers get 401", async () => {
    const handler = relay();
    for (const authorization of ["Basic d2RrX3g=", "Bearer", "BEARER", "Token wdk_x", "Bearer  "]) {
      const res = await handler(post({ token: false, headers: { authorization } }));
      expect(res.status).toBe(401);
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("unknown Bearer gets 401, no provider call", async () => {
    const res = await relay()(post({ token: generateToken() }));
    await expectOpenAIError(res, 401, "invalid_request_error", "invalid_api_key");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("revoked token gets 401 on its next request, other tokens unaffected (F3)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay();
    expect((await handler(post())).status).toBe(200);
    await tokenStore.revoke(deviceId);
    await expectOpenAIError(await handler(post()), 401, "invalid_request_error", "invalid_api_key");
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const other = await tokenStore.create("typewhisper-desktop");
    expect((await handler(post({ token: other.token }))).status).toBe(200);
  });

  it("a throwing token store refuses the request without leaking the token to the log", async () => {
    const broken: TokenStore = {
      verify: () => Promise.reject(new Error("store exploded")),
      create: (label) => tokenStore.create(label),
      list: () => tokenStore.list(),
      revoke: (id) => tokenStore.revoke(id),
    };
    const res = await relay({ tokenStore: broken })(post());
    await expectOpenAIError(res, 401, "invalid_request_error", "invalid_api_key");
    expect(fetchMock).not.toHaveBeenCalled();
    const logged = errorSpy.mock.calls.flat().join(" ");
    expect(logged).not.toContain(deviceToken);
  });
});

describe("rate limiting (own limiter, per token, KTD9)", () => {
  it("429 in the OpenAI shape with Retry-After once the per-token budget is spent", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay({ rateLimit: { max: 1 } });
    expect((await handler(post())).status).toBe(200);
    const res = await handler(post());
    await expectOpenAIError(res, 429, "rate_limit_exceeded", "rate_limit_exceeded");
    expect(res.headers.get("retry-after")).toMatch(/^\d+$/);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("is keyed by the token id: a second token keeps its own budget", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay({ rateLimit: { max: 1 } });
    expect((await handler(post())).status).toBe(200);
    const other = await tokenStore.create("second-device");
    expect((await handler(post({ token: other.token }))).status).toBe(200);
    expect((await handler(post())).status).toBe(429);
  });

  it("passes the verified token id to a custom limiter", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const seen: string[] = [];
    const handler = relay({
      rateLimit: (clientId) => {
        seen.push(clientId);
        return true;
      },
    });
    await handler(post());
    expect(seen).toEqual([deviceId]);
  });

  it("browser-route traffic does not count against the gateway limiter (and vice versa)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    on(DEEPGRAM_KEY, () => Response.json({ access_token: "eyJ.test.jwt", expires_in: 120 }));
    const handler = relay(
      { rateLimit: { max: 1 } },
      { keys: { deepgram: DEEPGRAM_KEY }, authorize: () => true },
    );
    const browserPost = () => new Request(`${BASE}/deepgram/token`, { method: "POST" });

    expect((await handler(post())).status).toBe(200); // spends the gateway budget
    expect((await handler(browserPost())).status).toBe(200); // browser limiter untouched
    expect((await handler(post())).status).toBe(429); // gateway budget spent
    expect((await handler(browserPost())).status).toBe(200); // browser route still fine
  });
});

describe("request body", () => {
  it("413 in the OpenAI shape before any provider call when the body exceeds gateway.maxBodyBytes", async () => {
    const handler = relay({ maxBodyBytes: 64 });
    const res = await handler(post());
    await expectOpenAIError(res, 413, "invalid_request_error");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("a body over the cap still spends the token's budget: the limiter runs before buffering", async () => {
    const handler = relay({ maxBodyBytes: 64, rateLimit: { max: 1 } });
    await expectOpenAIError(await handler(post()), 413, "invalid_request_error");
    await expectOpenAIError(await handler(post()), 429, "rate_limit_exceeded", "rate_limit_exceeded");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("an unexpected throw answers 500 in the OpenAI error shape", async () => {
    const handler = relay({
      rateLimit: () => {
        throw new Error("limiter exploded");
      },
    });
    const res = await handler(post());
    await expectOpenAIError(res, 500, "server_error");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("400 when `file` is missing, no provider call", async () => {
    const res = await relay()(post({ withFile: false }));
    await expectOpenAIError(res, 400, "invalid_request_error", "missing_required_parameter");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("400 on an unsupported response_format", async () => {
    const res = await relay()(post({ fields: { response_format: "srt" } }));
    await expectOpenAIError(res, 400, "invalid_request_error");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("400 on a non-multipart body", async () => {
    const res = await relay()(post({ rawBody: JSON.stringify({ hi: 1 }), rawContentType: "application/json" }));
    await expectOpenAIError(res, 400, "invalid_request_error");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("response formats", () => {
  it("response_format=text returns text/plain", async () => {
    on(GROQ_KEY_A, ok("hello world"));
    const res = await relay()(post({ fields: { response_format: "text" } }));
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/plain");
    expect(await res.text()).toBe("hello world");
    // `text` is fetched as `json` upstream; the gateway formats it.
    expect(sentForm(fetchMock).get("response_format")).toBe("json");
  });

  it.each([
    {
      name: "groq",
      entry: () => groqA,
      key: GROQ_KEY_A,
      reply: {
        task: "transcribe",
        text: "hello world",
        language: "english",
        duration: 1.5,
        segments: [{ id: 0, seek: 0, start: 0, end: 1.5, text: " hello world", tokens: [1], avg_logprob: -0.1 }],
        x_groq: { id: "req_123" },
      },
      want: {
        text: "hello world",
        language: "english",
        duration: 1.5,
        segments: [{ id: 0, start: 0, end: 1.5, text: " hello world" }],
      },
      forbidden: ["x_groq", "avg_logprob", "seek", "req_123"],
    },
    {
      name: "openai",
      entry: () => openai,
      key: OPENAI_KEY,
      reply: {
        text: "hi",
        language: "english",
        duration: 2,
        words: [{ word: "hi", start: 0, end: 2 }],
        usage: { seconds: 2, type: "duration" },
      },
      want: { text: "hi", language: "english", duration: 2 },
      forbidden: ["words", "usage"],
    },
    {
      name: "deepgram",
      entry: () => deepgram,
      key: DEEPGRAM_KEY,
      reply: {
        metadata: { request_id: "dg-req-1", duration: 3.25, models: ["nova-3"] },
        results: { channels: [{ alternatives: [{ transcript: "Hello.", confidence: 0.99, words: [] }] }] },
      },
      want: { text: "Hello.", duration: 3.25 },
      forbidden: ["request_id", "confidence", "results", "metadata"],
    },
  ])("verbose_json from $name is normalized to OpenAI fields only (R7)", async ({ entry, key, reply, want, forbidden }) => {
    on(key, () => Response.json(reply));
    const res = await relay({ providers: [entry()] })(post({ fields: { response_format: "verbose_json" } }));
    expect(res.status).toBe(200);
    const body: unknown = await res.json();
    expect(body).toEqual(want);
    const serialized = JSON.stringify(body);
    for (const bad of forbidden) expect(serialized).not.toContain(bad);
  });
});

describe("input hygiene (KTD9)", () => {
  it("drops `language=en&model=x`; the provider gets no language and no injected model", async () => {
    on(GROQ_KEY_A, ok("hi"));
    await relay()(post({ fields: { language: "en&model=x" } }));
    const form = sentForm(fetchMock);
    expect(form.get("language")).toBeNull();
    expect(form.get("model")).toBe("whisper-large-v3-turbo");
  });

  it("drops an invalid language on the Deepgram query too", async () => {
    on(DEEPGRAM_KEY, () =>
      Response.json({ results: { channels: [{ alternatives: [{ transcript: "hi" }] }] } }),
    );
    await relay({ providers: [deepgram] })(post({ fields: { language: "en&model=x" } }));
    const u = new URL(upstreamCall(fetchMock).url);
    expect(u.searchParams.has("language")).toBe(false);
    expect(u.searchParams.getAll("model")).toEqual(["nova-3"]);
  });
});

describe("vocabulary (R12)", () => {
  it("the operator vocabulary reaches the provider with no client prompt (AE4)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    await relay({ vocabulary: ["Omarchy", "Hyprland"] })(post());
    expect(sentForm(fetchMock).get("prompt")).toBe("Omarchy, Hyprland");
  });

  it("operator terms come first when a client prompt is present", async () => {
    on(GROQ_KEY_A, ok("hi"));
    await relay({ vocabulary: ["Omarchy"] })(post({ fields: { prompt: "Dictating notes." } }));
    expect(sentForm(fetchMock).get("prompt")).toBe("Omarchy. Dictating notes.");
  });
});

describe("provider fallback through the route", () => {
  it("Groq-A answers 401 (bad operator key): Groq-B serves (AE1, mocked)", async () => {
    on(GROQ_KEY_A, status(401));
    on(GROQ_KEY_B, ok("from B"));
    const res = await relay({ providers: [groqA, groqB] })(post());
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ text: "from B" });
    expect(keysCalled()).toEqual([GROQ_KEY_A, GROQ_KEY_B]);
  });

  it("all entries failing answers 502 upstream_unavailable, naming no provider or key (R7)", async () => {
    on(GROQ_KEY_A, status(429));
    on(OPENAI_KEY, status(500));
    const res = await relay({ providers: [groqA, openai] })(post());
    expect(res.status).toBe(502);
    const body = await res.text();
    expect(JSON.parse(body)).toMatchObject({
      error: { message: expect.any(String), type: "upstream_unavailable" },
    });
    expect(body.toLowerCase()).not.toMatch(/groq|openai|deepgram/);
    for (const key of [GROQ_KEY_A, OPENAI_KEY]) expect(body).not.toContain(key);
  });

  it("a provider 400 does not fall through; the client error is returned in the OpenAI shape", async () => {
    on(GROQ_KEY_A, status(400));
    on(GROQ_KEY_B, ok("from B"));
    const res = await relay({ providers: [groqA, groqB] })(post());
    await expectOpenAIError(res, 400, "invalid_request_error");
    expect(keysCalled()).toEqual([GROQ_KEY_A]);
  });

  it("504 in the OpenAI shape within deadlineMs when every provider hangs", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    on(GROQ_KEY_A, hang);
    on(GROQ_KEY_B, hang);
    const pending = relay({ providers: [groqA, groqB], deadlineMs: 5_000 })(post());
    // Real event-loop turns let token verify + body parsing reach the first provider call.
    while (fetchMock.mock.calls.length === 0) await new Promise((r) => setImmediate(r));

    let settled = false;
    void pending.then(() => {
      settled = true;
    });
    await vi.advanceTimersByTimeAsync(4_999);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    const res = await pending;
    await expectOpenAIError(res, 504, "upstream_timeout");
    expect(keysCalled()).toEqual([GROQ_KEY_A]); // B never started: the deadline cut the chain
  });
});

describe("GET /v1/models", () => {
  it("lists each configured provider's effective model in the OpenAI list shape (R2)", async () => {
    const res = await relay({
      providers: [groqA, { provider: "openai", key: OPENAI_KEY, model: "custom-deploy" }, deepgram],
    })(get("/v1/models"));
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("application/json");
    const body = await res.json();
    expect(body).toEqual({
      object: "list",
      data: [
        { id: "whisper-large-v3-turbo", object: "model", created: 0, owned_by: "wordink" },
        { id: "custom-deploy", object: "model", created: 0, owned_by: "wordink" },
        { id: "nova-3", object: "model", created: 0, owned_by: "wordink" },
      ],
    });
    // No provider names, URLs or keys escape in the response (R7 posture carries over).
    const raw = JSON.stringify(body);
    for (const leak of ["groq", "openai", "deepgram", GROQ_KEY_A, OPENAI_KEY, DEEPGRAM_KEY, GROQ_AUDIO_URL]) {
      expect(raw).not.toContain(leak);
    }
  });

  it("de-duplicates entries that resolve to the same model", async () => {
    const res = await relay({ providers: [groqA, groqB, openai] })(get("/v1/models"));
    const body = (await res.json()) as { data: { id: string }[] };
    expect(body.data.map((m) => m.id)).toEqual(["whisper-large-v3-turbo", "gpt-4o-transcribe"]);
  });

  it("is behind the same device-token gate: missing, garbage and revoked tokens all 401", async () => {
    await expectOpenAIError(await relay()(get("/v1/models", false)), 401, "invalid_request_error", "invalid_api_key");
    await expectOpenAIError(await relay()(get("/v1/models", "wdk_garbage")), 401, "invalid_request_error", "invalid_api_key");
    await tokenStore.revoke(deviceId);
    await expectOpenAIError(await relay()(get("/v1/models")), 401, "invalid_request_error", "invalid_api_key");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects non-GET methods in the OpenAI shape with allow: GET", async () => {
    for (const method of ["POST", "PUT", "DELETE"]) {
      const res = await relay()(
        new Request(`${BASE}/v1/models`, { method, headers: { authorization: `Bearer ${deviceToken}` } }),
      );
      expect(res.status).toBe(405);
      expect(res.headers.get("allow")).toBe("GET");
      await expectOpenAIError(res, 405, "invalid_request_error");
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("shares the per-token rate limiter with transcriptions", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay({ rateLimit: { windowMs: 60_000, max: 1, dailyMax: Number.POSITIVE_INFINITY } });
    // A transcription spends the token's whole budget; the next models call 429s — the
    // same limiter instance, not a per-route one.
    expect((await handler(post())).status).toBe(200);
    const res = await handler(get("/v1/models"));
    await expectOpenAIError(res, 429, "rate_limit_exceeded", "rate_limit_exceeded");
    expect(res.headers.get("retry-after")).toEqual(expect.any(String));
  });

  it("honours basePath and 404s when no gateway is configured", async () => {
    const mounted = relay({}, { basePath: "/wordink" });
    expect((await mounted(get("/wordink/v1/models"))).status).toBe(200);
    expect((await mounted(get("/v1/models"))).status).toBe(404);
    const plain = createRelay({ keys: {} });
    expect((await plain(get("/v1/models"))).status).toBe(404);
  });
});

describe("coexistence with the browser relay (R11)", () => {
  it("with no authorize hook the gateway still serves, browser routes 403, and the setup log says they are disabled", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay(); // no authorize
    expect((await handler(post())).status).toBe(200);
    const res = await handler(new Request(`${BASE}/deepgram/token`, { method: "POST" }));
    expect(res.status).toBe(403);
    const warned = warnSpy.mock.calls.flat().map(String).join(" ");
    expect(warned).toMatch(/browser relay routes/i);
    // The missing authorize is not reported as a misconfiguration when a gateway is configured.
    expect(errorSpy).not.toHaveBeenCalled();
  });
});
