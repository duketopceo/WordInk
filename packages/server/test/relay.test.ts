import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRelay, type RelayConfig } from "../src/index.js";
import { createWorker } from "../src/cloudflare.js";
import { createNodeHandler } from "../src/node.js";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";

const GROQ_KEY = "gsk_test_GROQ_SECRET_0123456789";
const OPENAI_KEY = "sk-test-OPENAI_SECRET_0123456789";
const DEEPGRAM_KEY = "dg_test_DEEPGRAM_SECRET_0123456789";
const ALLOWED = "https://app.example.com";

type FetchMock = ReturnType<typeof vi.fn<typeof fetch>>;
let fetchMock: FetchMock;
let errorSpy: ReturnType<typeof vi.spyOn>;
let logSpies: ReturnType<typeof vi.spyOn>[];

/** Upstream fake: answers like each provider would, echoing TTLs it was asked for. */
async function fakeUpstream(input: string | URL | Request, init?: RequestInit): Promise<Response> {
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  if (url === "https://api.groq.com/openai/v1/audio/transcriptions") {
    return new Response("hello world", { status: 200, headers: { "content-type": "text/plain" } });
  }
  if (url === "https://api.openai.com/v1/realtime/client_secrets") {
    const body = JSON.parse(String(init?.body)) as { expires_after: { seconds: number } };
    return Response.json({
      value: "ek_test_ephemeral",
      expires_at: 1_700_000_000 + body.expires_after.seconds,
      session: { type: "transcription", id: "sess_1", echo: OPENAI_KEY },
    });
  }
  if (url === "https://api.deepgram.com/v1/auth/grant") {
    const body = JSON.parse(String(init?.body)) as { ttl_seconds: number };
    return Response.json({ access_token: "eyJ.test.jwt", expires_in: body.ttl_seconds, scope: "x" });
  }
  return new Response("unexpected", { status: 599 });
}

function relay(overrides: Partial<RelayConfig> = {}) {
  return createRelay({
    keys: { groq: GROQ_KEY, openai: OPENAI_KEY, deepgram: DEEPGRAM_KEY },
    authorize: () => true,
    allowedOrigins: [ALLOWED],
    ...overrides,
  });
}

/** A browser-style multipart upload, pre-encoded so it carries a real Content-Length. */
class Multipart {
  constructor(
    readonly body: Uint8Array,
    readonly contentType: string,
  ) {}
}

function audioForm(bytes = 1024, extra: Record<string, string> = {}): Multipart {
  const boundary = "----wordinktest" + bytes;
  const enc = new TextEncoder();
  const fields = { model: "whisper-large-v3-turbo", response_format: "text", ...extra };
  const parts: Uint8Array[] = [
    enc.encode(
      `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="audio.wav"\r\n` +
        "Content-Type: audio/wav\r\n\r\n",
    ),
    new Uint8Array(bytes),
    enc.encode("\r\n"),
    ...Object.entries(fields).map(([k, v]) =>
      enc.encode(`--${boundary}\r\nContent-Disposition: form-data; name="${k}"\r\n\r\n${v}\r\n`),
    ),
    enc.encode(`--${boundary}--\r\n`),
  ];
  const body = new Uint8Array(parts.reduce((n, p) => n + p.byteLength, 0));
  let off = 0;
  for (const p of parts) {
    body.set(p, off);
    off += p.byteLength;
  }
  return new Multipart(body, `multipart/form-data; boundary=${boundary}`);
}

type PostInit = Omit<RequestInit, "body" | "headers"> & {
  body?: RequestInit["body"] | Multipart;
  headers?: Record<string, string>;
};

function post(path: string, init: PostInit = {}): Request {
  const { body, headers, ...rest } = init;
  const mp = body instanceof Multipart ? body : null;
  return new Request(`https://relay.example.com${path}`, {
    method: "POST",
    ...rest,
    body: mp ? mp.body : ((body as RequestInit["body"]) ?? null),
    headers: {
      origin: ALLOWED,
      "cf-connecting-ip": "203.0.113.7",
      ...(mp ? { "content-type": mp.contentType, "content-length": String(mp.body.byteLength) } : {}),
      ...headers,
    },
  });
}

async function allText(res: Response): Promise<string> {
  const headers = [...res.headers.entries()].map(([k, v]) => `${k}: ${v}`).join("\n");
  return `${headers}\n${await res.text()}`;
}

function upstreamCall(i = 0): { url: string; init: RequestInit } {
  const call = fetchMock.mock.calls[i];
  if (!call) throw new Error(`no upstream call #${i}`);
  const [input, init] = call;
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  return { url, init: init ?? {} };
}

beforeEach(() => {
  fetchMock = vi.fn<typeof fetch>(fakeUpstream);
  vi.stubGlobal("fetch", fetchMock);
  errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  logSpies = (["log", "info", "warn", "debug"] as const).map((m) =>
    vi.spyOn(console, m).mockImplementation(() => {}),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("Groq forward", () => {
  it("forwards the audio body with the env key and never echoes the key", async () => {
    const res = await relay()(post("/groq/transcriptions", { body: audioForm(2048) }));
    expect(res.status).toBe(200);
    const text = await allText(res);
    expect(text).toContain("hello world");
    expect(text).not.toContain(GROQ_KEY);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const { url, init } = upstreamCall();
    expect(url).toBe("https://api.groq.com/openai/v1/audio/transcriptions");
    expect(init.method).toBe("POST");
    const headers = new Headers(init.headers);
    expect(headers.get("authorization")).toBe(`Bearer ${GROQ_KEY}`);
    expect(headers.get("content-type")).toMatch(/^multipart\/form-data; boundary=/);
    // The forwarded body is the browser's multipart payload, intact.
    const forwarded = await new Request("https://x", {
      method: "POST",
      headers,
      body: init.body as Uint8Array,
    }).formData();
    expect(forwarded.get("model")).toBe("whisper-large-v3-turbo");
    expect(forwarded.get("response_format")).toBe("text");
    const file = forwarded.get("file") as File;
    expect(file.size).toBe(2048);
    // Browser cookies / auth headers are not forwarded upstream.
    expect(headers.get("cookie")).toBeNull();
  });

  it("does not pass upstream error bodies (which can quote the key) through", async () => {
    fetchMock.mockResolvedValueOnce(
      Response.json({ error: { message: `Invalid API Key ${GROQ_KEY}` } }, { status: 401 }),
    );
    const res = await relay()(post("/groq/transcriptions", { body: audioForm() }));
    expect(res.status).toBe(502);
    expect(await allText(res)).not.toContain(GROQ_KEY);
  });

  it("passes an upstream 429 through as 429", async () => {
    fetchMock.mockResolvedValueOnce(new Response("slow down", { status: 429 }));
    const res = await relay()(post("/groq/transcriptions", { body: audioForm() }));
    expect(res.status).toBe(429);
  });

  it("rejects a body over the cap with 413 before any upstream call (Content-Length)", async () => {
    const res = await relay()(post("/groq/transcriptions", { body: audioForm(2 * 1024 * 1024 + 1) }));
    expect(res.status).toBe(413);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects an oversized streamed body with no Content-Length with 413", async () => {
    const chunk = new Uint8Array(64 * 1024);
    let sent = 0;
    const stream = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (sent > 3 * 1024 * 1024) return controller.close();
        sent += chunk.length;
        controller.enqueue(chunk);
      },
    });
    const req = post("/groq/transcriptions", {
      body: stream,
      headers: { "content-type": "multipart/form-data; boundary=x" },
      duplex: "half",
    } as PostInit);
    expect(req.headers.get("content-length")).toBeNull();
    const res = await relay()(req);
    expect(res.status).toBe(413);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("honours a configured maxBodyBytes", async () => {
    const res = await relay({ maxBodyBytes: 1000 })(post("/groq/transcriptions", { body: audioForm(2000) }));
    expect(res.status).toBe(413);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("OpenAI mint", () => {
  it("returns only the ek_ secret and expiry, never the source key", async () => {
    const res = await relay()(post("/openai/token"));
    expect(res.status).toBe(200);
    const raw = await res.clone().text();
    expect(raw).not.toContain(OPENAI_KEY);
    expect(await res.json()).toEqual({ value: "ek_test_ephemeral", expires_at: 1_700_000_120 });

    const { url, init } = upstreamCall();
    expect(url).toBe("https://api.openai.com/v1/realtime/client_secrets");
    expect(new Headers(init.headers).get("authorization")).toBe(`Bearer ${OPENAI_KEY}`);
    expect(JSON.parse(String(init.body))).toEqual({
      expires_after: { anchor: "created_at", seconds: 120 },
      session: { type: "transcription" },
    });
  });

  it("does not leak upstream error bodies", async () => {
    fetchMock.mockResolvedValueOnce(
      Response.json({ error: { message: `Incorrect API key provided: ${OPENAI_KEY}` } }, { status: 401 }),
    );
    const res = await relay()(post("/openai/token"));
    expect(res.status).toBe(502);
    expect(await allText(res)).not.toContain(OPENAI_KEY);
  });
});

describe("Deepgram mint", () => {
  it("returns the JWT and TTL only", async () => {
    const res = await relay()(post("/deepgram/token"));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ access_token: "eyJ.test.jwt", expires_in: 120 });
    const { url, init } = upstreamCall();
    expect(url).toBe("https://api.deepgram.com/v1/auth/grant");
    expect(new Headers(init.headers).get("authorization")).toBe(`Token ${DEEPGRAM_KEY}`);
    expect(JSON.parse(String(init.body))).toEqual({ ttl_seconds: 120 });
  });
});

describe("server-fixed TTL", () => {
  const clientTtl = JSON.stringify({ ttl_seconds: 3600, ttl: 3600, expires_after: { seconds: 3600 } });

  it("ignores a client TTL of 3600 s on the Deepgram mint", async () => {
    const res = await relay()(
      post("/deepgram/token?ttl=3600&ttl_seconds=3600", {
        body: clientTtl,
        headers: { "content-type": "application/json" },
      }),
    );
    expect(res.status).toBe(200);
    expect(((await res.json()) as { expires_in: number }).expires_in).toBe(120);
    expect(JSON.parse(String(upstreamCall().init.body))).toEqual({ ttl_seconds: 120 });
  });

  it("ignores a client TTL of 3600 s on the OpenAI mint", async () => {
    const res = await relay()(
      post("/openai/token?ttl=3600", { body: clientTtl, headers: { "content-type": "application/json" } }),
    );
    expect(((await res.json()) as { expires_at: number }).expires_at).toBe(1_700_000_120);
    const sent = JSON.parse(String(upstreamCall().init.body)) as { expires_after: { seconds: number } };
    expect(sent.expires_after.seconds).toBe(120);
  });

  it("uses a configured server TTL", async () => {
    await relay({ tokenTtlSeconds: 60 })(post("/deepgram/token"));
    expect(JSON.parse(String(upstreamCall().init.body))).toEqual({ ttl_seconds: 60 });
  });
});

describe("authorization (fail closed)", () => {
  it("returns 403 with no upstream call when authorize returns false", async () => {
    const authorize = vi.fn(async () => false);
    const handler = relay({ authorize });
    for (const path of ["/groq/transcriptions", "/openai/token", "/deepgram/token"]) {
      const res = await handler(post(path, { body: path.startsWith("/groq") ? audioForm() : null }));
      expect(res.status).toBe(403);
    }
    expect(authorize).toHaveBeenCalledTimes(3);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("returns 403 when authorize throws", async () => {
    const res = await relay({
      authorize: () => {
        throw new Error("session store down");
      },
    })(post("/openai/token"));
    expect(res.status).toBe(403);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("without an authorize hook, every route is 403 even with a spoofed allowed Origin, and logs one setup error", async () => {
    const handler = createRelay({
      keys: { groq: GROQ_KEY, openai: OPENAI_KEY, deepgram: DEEPGRAM_KEY },
      allowedOrigins: [ALLOWED],
    });
    for (const path of ["/groq/transcriptions", "/openai/token", "/deepgram/token"]) {
      const res = await handler(
        post(path, {
          body: path.startsWith("/groq") ? audioForm() : null,
          headers: { origin: ALLOWED, referer: `${ALLOWED}/page` },
        }),
      );
      expect(res.status).toBe(403);
    }
    expect(fetchMock).not.toHaveBeenCalled();
    expect(errorSpy).toHaveBeenCalledTimes(1);
    expect(String(errorSpy.mock.calls[0]?.[0])).toMatch(/authorize/);
  });

  it("passes the request to authorize so it can check the app session", async () => {
    const handler = relay({
      authorize: (req) => req.headers.get("cookie") === "session=valid",
    });
    expect((await handler(post("/deepgram/token", { headers: { cookie: "session=nope" } }))).status).toBe(403);
    expect((await handler(post("/deepgram/token", { headers: { cookie: "session=valid" } }))).status).toBe(200);
  });
});

describe("rate limiting", () => {
  it("returns 429 past the default per-client limit, with no upstream call", async () => {
    const handler = relay();
    let first429 = -1;
    for (let i = 0; i < 100; i++) {
      const res = await handler(post("/deepgram/token"));
      if (res.status === 429) {
        first429 = i;
        expect(res.headers.get("retry-after")).toMatch(/^\d+$/);
        break;
      }
      expect(res.status).toBe(200);
    }
    expect(first429).toBeGreaterThan(0);
    expect(fetchMock).toHaveBeenCalledTimes(first429);
  });

  it("keys the limit per client and resets after the window", async () => {
    vi.useFakeTimers();
    try {
      const handler = relay({ rateLimit: { windowMs: 1000, max: 2, dailyMax: 1000 } });
      const a = () => handler(post("/deepgram/token", { headers: { "cf-connecting-ip": "198.51.100.1" } }));
      const b = () => handler(post("/deepgram/token", { headers: { "cf-connecting-ip": "198.51.100.2" } }));
      expect((await a()).status).toBe(200);
      expect((await a()).status).toBe(200);
      expect((await a()).status).toBe(429);
      expect((await b()).status).toBe(200);
      vi.advanceTimersByTime(1001);
      expect((await a()).status).toBe(200);
    } finally {
      vi.useRealTimers();
    }
  });

  it("enforces the daily cap across all clients", async () => {
    const handler = relay({ rateLimit: { windowMs: 60_000, max: 100, dailyMax: 3 } });
    const statuses: number[] = [];
    for (let i = 0; i < 5; i++) {
      const res = await handler(post("/deepgram/token", { headers: { "cf-connecting-ip": `198.51.100.${i}` } }));
      statuses.push(res.status);
    }
    expect(statuses).toEqual([200, 200, 200, 429, 429]);
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });

  it("uses a custom clientId hook", async () => {
    const handler = relay({
      rateLimit: { windowMs: 60_000, max: 1, dailyMax: 100 },
      clientId: (req) => req.headers.get("x-user") ?? "anon",
    });
    expect((await handler(post("/deepgram/token", { headers: { "x-user": "u1" } }))).status).toBe(200);
    expect((await handler(post("/deepgram/token", { headers: { "x-user": "u1" } }))).status).toBe(429);
    expect((await handler(post("/deepgram/token", { headers: { "x-user": "u2" } }))).status).toBe(200);
  });

  it("accepts a custom limiter function", async () => {
    const limiter = vi.fn(async (_id: string) => false);
    const res = await relay({ rateLimit: limiter })(post("/openai/token"));
    expect(res.status).toBe(429);
    expect(limiter).toHaveBeenCalledWith("203.0.113.7", expect.any(Request));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("does not spend rate-limit budget on unauthorized requests", async () => {
    let allow = false;
    const handler = relay({ authorize: () => allow, rateLimit: { windowMs: 60_000, max: 1, dailyMax: 100 } });
    for (let i = 0; i < 5; i++) expect((await handler(post("/deepgram/token"))).status).toBe(403);
    allow = true;
    expect((await handler(post("/deepgram/token"))).status).toBe(200);
  });
});

describe("CORS", () => {
  it("sets the allow header for a configured origin", async () => {
    const res = await relay()(post("/deepgram/token"));
    expect(res.headers.get("access-control-allow-origin")).toBe(ALLOWED);
    expect(res.headers.get("vary")).toMatch(/origin/i);
  });

  it("gives a non-allowed origin no CORS allow header", async () => {
    const res = await relay()(post("/deepgram/token", { headers: { origin: "https://evil.example" } }));
    expect(res.headers.get("access-control-allow-origin")).toBeNull();
  });

  it("answers preflight only for allowed origins, without touching authorize or upstream", async () => {
    const authorize = vi.fn(() => true);
    const handler = relay({ authorize });
    const ok = await handler(
      new Request("https://relay.example.com/groq/transcriptions", {
        method: "OPTIONS",
        headers: { origin: ALLOWED, "access-control-request-method": "POST" },
      }),
    );
    expect(ok.status).toBe(204);
    expect(ok.headers.get("access-control-allow-origin")).toBe(ALLOWED);
    expect(ok.headers.get("access-control-allow-methods")).toContain("POST");
    const bad = await handler(
      new Request("https://relay.example.com/groq/transcriptions", {
        method: "OPTIONS",
        headers: { origin: "https://evil.example", "access-control-request-method": "POST" },
      }),
    );
    expect(bad.headers.get("access-control-allow-origin")).toBeNull();
    expect(authorize).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("an allowed Origin is not authentication", async () => {
    const res = await relay({ authorize: () => false })(post("/openai/token"));
    expect(res.status).toBe(403);
  });
});

describe("origin check (CSRF)", () => {
  it("rejects an authorized cookie request from a disallowed Origin with 403 and no upstream call", async () => {
    const authorize = vi.fn(() => true);
    const res = await relay({ authorize })(
      post("/groq/transcriptions", {
        body: audioForm(),
        headers: { origin: "https://evil.example", cookie: "session=valid" },
      }),
    );
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: "origin_not_allowed" });
    expect(authorize).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects the opaque Origin: null", async () => {
    const res = await relay()(post("/deepgram/token", { headers: { origin: "null" } }));
    expect(res.status).toBe(403);
    expect(await res.json()).toEqual({ error: "origin_not_allowed" });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("lets a same-origin request through without listing it", async () => {
    const res = await relay({ allowedOrigins: [] })(
      post("/deepgram/token", { headers: { origin: "https://relay.example.com" } }),
    );
    expect(res.status).toBe(200);
  });

  it("lets an allowed origin through", async () => {
    const res = await relay()(post("/deepgram/token", { headers: { origin: ALLOWED } }));
    expect(res.status).toBe(200);
  });

  it("sends a request with no Origin header on to authorize", async () => {
    const authorize = vi.fn(() => true);
    const req = post("/deepgram/token");
    req.headers.delete("origin");
    const res = await relay({ authorize })(req);
    expect(authorize).toHaveBeenCalledTimes(1);
    expect(res.status).toBe(200);
  });
});

describe("upstream timeout", () => {
  const hang = (_input: string | URL | Request, init?: RequestInit) =>
    new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => reject(init.signal?.reason));
    });

  it.each(["/groq/transcriptions", "/openai/token", "/deepgram/token"])(
    "answers 502 upstream_unreachable when %s never responds",
    async (path) => {
      fetchMock.mockImplementationOnce(hang);
      const res = await relay({ upstreamTimeoutMs: 20 })(
        post(path, path === "/groq/transcriptions" ? { body: audioForm() } : {}),
      );
      expect(res.status).toBe(502);
      expect(await res.json()).toEqual({ error: "upstream_unreachable" });
    },
  );

  it("passes a default timeout signal to the upstream fetch", async () => {
    await relay()(post("/openai/token"));
    expect(upstreamCall().init.signal).toBeInstanceOf(AbortSignal);
  });
});

describe("routing and hygiene", () => {
  it("honours a base path", async () => {
    const handler = relay({ basePath: "/api/wordink/" });
    expect((await handler(post("/api/wordink/deepgram/token"))).status).toBe(200);
    expect((await handler(post("/deepgram/token"))).status).toBe(404);
  });

  it("returns 404 for unknown paths and 405 for non-POST", async () => {
    const handler = relay();
    expect((await handler(post("/nope"))).status).toBe(404);
    expect((await handler(new Request("https://relay.example.com/openai/token"))).status).toBe(405);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("returns 503 when the provider key is not configured", async () => {
    const res = await relay({ keys: { groq: GROQ_KEY } })(post("/deepgram/token"));
    expect(res.status).toBe(503);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("returns 502 when the upstream is unreachable", async () => {
    fetchMock.mockRejectedValueOnce(new TypeError("fetch failed"));
    const res = await relay()(post("/openai/token"));
    expect(res.status).toBe(502);
  });

  it("never logs request bodies or keys", async () => {
    const handler = relay();
    const marker = "SECRET_BODY_MARKER";
    fetchMock.mockRejectedValueOnce(new TypeError("fetch failed"));
    await handler(post("/openai/token", { body: JSON.stringify({ marker }) }));
    fetchMock.mockResolvedValueOnce(new Response("err", { status: 500 }));
    const form = audioForm(1024, { prompt: marker });
    await handler(post("/groq/transcriptions", { body: form }));
    const logged = [errorSpy, ...logSpies]
      .flatMap((s) => s.mock.calls)
      .map((args) => args.map((a: unknown) => (a instanceof Error ? `${a.message} ${a.stack}` : String(a))).join(" "))
      .join("\n");
    expect(logged).not.toContain(marker);
    for (const key of [GROQ_KEY, OPENAI_KEY, DEEPGRAM_KEY]) expect(logged).not.toContain(key);
  });
});

describe("Cloudflare adapter", () => {
  it("builds the relay from env keys and reuses it (rate-limit state persists)", async () => {
    const worker = createWorker<{ GROQ_API_KEY: string; OPENAI_API_KEY: string; DEEPGRAM_API_KEY: string }>(
      () => ({
        authorize: () => true,
        allowedOrigins: [ALLOWED],
        rateLimit: { windowMs: 60_000, max: 1, dailyMax: 10 },
      }),
    );
    const env = { GROQ_API_KEY: GROQ_KEY, OPENAI_API_KEY: OPENAI_KEY, DEEPGRAM_API_KEY: DEEPGRAM_KEY };
    const ctx = { waitUntil: () => {}, passThroughOnException: () => {} };
    const r1 = await worker.fetch(post("/deepgram/token"), env, ctx);
    expect(r1.status).toBe(200);
    expect(new Headers(upstreamCall().init.headers).get("authorization")).toBe(`Token ${DEEPGRAM_KEY}`);
    const r2 = await worker.fetch(post("/deepgram/token"), env, ctx);
    expect(r2.status).toBe(429);
  });

  it("fails closed when the factory gives no authorize", async () => {
    const worker = createWorker(() => ({}));
    const res = await worker.fetch(post("/deepgram/token"), { DEEPGRAM_API_KEY: DEEPGRAM_KEY }, {
      waitUntil: () => {},
      passThroughOnException: () => {},
    });
    expect(res.status).toBe(403);
  });
});

describe("Node adapter", () => {
  let server: Server | undefined;
  afterEach(async () => {
    await new Promise<void>((r) => (server ? server.close(() => r()) : r()));
    server = undefined;
  });

  async function listen(handler: Parameters<typeof createServer>[1]): Promise<string> {
    server = createServer(handler);
    await new Promise<void>((r) => server!.listen(0, "127.0.0.1", () => r()));
    return `http://127.0.0.1:${(server!.address() as AddressInfo).port}`;
  }

  it("serves the relay over node:http and keys the limit on the socket address, not X-Forwarded-For", async () => {
    // Real loopback HTTP (node:http client) for the adapter; upstream fetch stays mocked.
    const seen: string[] = [];
    const base = await listen(
      createNodeHandler({
        keys: { deepgram: DEEPGRAM_KEY },
        authorize: () => true,
        clientId: (req) => {
          const id = req.headers.get("x-forwarded-for") ?? "anon";
          seen.push(id);
          return id;
        },
      }),
    );
    const { request } = await import("node:http");
    const status = await new Promise<number>((resolve, reject) => {
      const req = request(`${base}/deepgram/token`, {
        method: "POST",
        headers: { "x-forwarded-for": "6.6.6.6", "content-type": "application/json" },
      });
      req.on("response", (res) => {
        res.resume();
        resolve(res.statusCode ?? 0);
      });
      req.on("error", reject);
      req.end("{}");
    });
    expect(status).toBe(200);
    expect(seen).toEqual(["127.0.0.1"]);
  });

  it("forwards a multipart body through node:http to Groq", async () => {
    const base = await listen(createNodeHandler({ keys: { groq: GROQ_KEY }, authorize: () => true }));
    const mp = audioForm(4096);
    const { request } = await import("node:http");
    const body = Buffer.from(mp.body);
    const result = await new Promise<{ status: number; text: string }>((resolve, reject) => {
      const r = request(`${base}/groq/transcriptions`, {
        method: "POST",
        headers: { "content-type": mp.contentType, "content-length": String(body.length) },
      });
      r.on("response", (res) => {
        let text = "";
        res.setEncoding("utf8");
        res.on("data", (d: string) => (text += d));
        res.on("end", () => resolve({ status: res.statusCode ?? 0, text }));
      });
      r.on("error", reject);
      r.end(body);
    });
    expect(result).toEqual({ status: 200, text: "hello world" });
    const forwarded = upstreamCall().init.body as ArrayBuffer | Uint8Array;
    expect(forwarded.byteLength).toBe(body.length);
  });
});
