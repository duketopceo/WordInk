import { forwardGroq } from "./groq.js";
import { pickOpenAIClientSecret, requestOpenAIClientSecret } from "./openai.js";
import { pickDeepgramGrant, requestDeepgramGrant } from "./deepgram.js";

export { GROQ_TRANSCRIPTIONS_URL } from "./groq.js";
export { OPENAI_CLIENT_SECRETS_URL, type OpenAIClientSecret } from "./openai.js";
export { DEEPGRAM_GRANT_URL, type DeepgramGrant } from "./deepgram.js";

/** Long-lived provider keys. They stay on the server; a route whose key is missing answers 503. */
export interface ProviderKeys {
  groq?: string | undefined;
  openai?: string | undefined;
  deepgram?: string | undefined;
}

/** Built-in in-memory limiter settings. State is per process / per Worker isolate. */
export interface RateLimitOptions {
  /** Fixed window length for the per-client limit. Default 60 000 ms. */
  windowMs?: number;
  /** Upstream calls allowed per client per window. Default 20. */
  max?: number;
  /** Upstream calls allowed per UTC day across all clients (this instance). Default 1000. */
  dailyMax?: number;
}

/**
 * A custom limiter (e.g. backed by KV, Redis or a Durable Object). Return `true` to allow the
 * upstream call, `false` to answer 429. Replaces the built-in limiter entirely.
 */
export type RateLimiter = (clientId: string, request: Request) => boolean | Promise<boolean>;

export interface RelayConfig {
  /**
   * Required. Decide whether this request may spend your provider quota, typically by checking
   * your app's session cookie or JWT. Without it the relay refuses every request (fails closed).
   * Throwing is treated as `false`.
   */
  authorize?: ((request: Request) => boolean | Promise<boolean>) | undefined;
  keys: ProviderKeys;
  /** Path prefix the routes live under, e.g. `/api/wordink`. Default: none. */
  basePath?: string;
  /**
   * Cross-origin browser apps allowed to call the relay; they also get CORS allow headers. A request
   * whose `Origin` (including `null`) is neither the relay's own origin nor listed here gets 403
   * before `authorize` runs, so a cross-site page can't spend quota with the user's cookie.
   * Requests with no Origin header (server-to-server) still go to `authorize`. Origin is trivially
   * forged outside a browser, so this is never a substitute for `authorize`.
   */
  allowedOrigins?: readonly string[];
  /** Override the built-in limiter's settings, or supply your own limiter. */
  rateLimit?: RateLimitOptions | RateLimiter;
  /**
   * Key for per-client rate limiting. Default: CF-Connecting-IP, then the first X-Forwarded-For
   * entry, then "anon". Prefer your authenticated user id when you have one.
   */
  clientId?: (request: Request) => string;
  /** Maximum Groq upload size in bytes; larger bodies get 413 before any upstream call. Default 2 MB. */
  maxBodyBytes?: number;
  /** Lifetime of minted OpenAI/Deepgram credentials. Fixed server-side; client values are ignored. Default 120. */
  tokenTtlSeconds?: number;
  /** Abort an upstream provider call after this many ms; the relay then answers 502. Default 15 000. */
  upstreamTimeoutMs?: number;
}

export type RelayHandler = (request: Request) => Promise<Response>;

export const DEFAULT_MAX_BODY_BYTES = 2 * 1024 * 1024;
export const DEFAULT_TOKEN_TTL_SECONDS = 120;
export const DEFAULT_UPSTREAM_TIMEOUT_MS = 15_000;
export const DEFAULT_RATE_LIMIT: Required<RateLimitOptions> = { windowMs: 60_000, max: 20, dailyMax: 1000 };

const LOG_PREFIX = "[@wordink/server]";
const ROUTES = ["/groq/transcriptions", "/openai/token", "/deepgram/token"] as const;
type Route = (typeof ROUTES)[number];

export function defaultClientId(request: Request): string {
  const cf = request.headers.get("cf-connecting-ip");
  if (cf) return cf.trim();
  const xff = request.headers.get("x-forwarded-for");
  const first = xff?.split(",")[0]?.trim();
  return first || "anon";
}

/** Create the relay: a Web-standard `(Request) => Promise<Response>` handler. */
export function createRelay(config: RelayConfig): RelayHandler {
  const authorize = config.authorize;
  if (typeof authorize !== "function") {
    console.error(
      `${LOG_PREFIX} No \`authorize\` hook configured: every request will be refused with 403. ` +
        "Pass authorize(request) that checks your app's own session (cookie or JWT). " +
        "CORS/Origin checks are not authentication.",
    );
  }

  const basePath = normalizeBasePath(config.basePath);
  const allowedOrigins = new Set(config.allowedOrigins ?? []);
  const maxBodyBytes = config.maxBodyBytes ?? DEFAULT_MAX_BODY_BYTES;
  const ttl = config.tokenTtlSeconds ?? DEFAULT_TOKEN_TTL_SECONDS;
  const timeoutMs = config.upstreamTimeoutMs ?? DEFAULT_UPSTREAM_TIMEOUT_MS;
  const clientIdOf = config.clientId ?? defaultClientId;
  const limiter =
    typeof config.rateLimit === "function" ? config.rateLimit : createMemoryLimiter(config.rateLimit);

  return async function relay(request: Request): Promise<Response> {
    const origin = request.headers.get("origin");
    const cors = origin !== null && allowedOrigins.has(origin) ? corsHeaders(origin) : null;
    const reply = (status: number, body: unknown, extra?: Record<string, string>) =>
      json(status, body, { ...cors, ...extra });

    const path = new URL(request.url).pathname;
    const route = matchRoute(path, basePath);
    if (!route) return reply(404, { error: "not_found" });

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: { ...cors, vary: "Origin" } });
    }
    if (request.method !== "POST") return reply(405, { error: "method_not_allowed" }, { allow: "POST, OPTIONS" });

    // Browsers set Origin on cross-site POSTs; an untrusted one never reaches authorize (CSRF).
    if (origin !== null && origin !== new URL(request.url).origin && !allowedOrigins.has(origin)) {
      return reply(403, { error: "origin_not_allowed" });
    }

    if (typeof authorize !== "function") return reply(403, { error: "relay_not_configured" });
    let allowed = false;
    try {
      allowed = (await authorize(request)) === true;
    } catch (err) {
      console.error(`${LOG_PREFIX} authorize hook threw; denying request.`, errorName(err));
    }
    if (!allowed) return reply(403, { error: "forbidden" });

    const key = keyFor(route, config.keys);
    if (!key) return reply(503, { error: "provider_not_configured" });

    // Read (and cap) the Groq body before spending rate-limit budget on it.
    let audio: Uint8Array | undefined;
    if (route === "/groq/transcriptions") {
      const read = await readCapped(request, maxBodyBytes);
      if (read === "too_large") return reply(413, { error: "payload_too_large", max_bytes: maxBodyBytes });
      audio = read;
    }

    const verdict = await limiter(clientIdOf(request), request);
    if (verdict !== true) {
      const retryAfter = typeof verdict === "number" ? verdict : 60;
      return reply(429, { error: "rate_limited" }, { "retry-after": String(retryAfter) });
    }

    let upstream: Response;
    try {
      if (route === "/groq/transcriptions") {
        upstream = await forwardGroq(key, audio ?? new Uint8Array(), request.headers.get("content-type"), timeoutMs);
      } else if (route === "/openai/token") {
        upstream = await requestOpenAIClientSecret(key, ttl, timeoutMs);
      } else {
        upstream = await requestDeepgramGrant(key, ttl, timeoutMs);
      }
    } catch (err) {
      console.error(`${LOG_PREFIX} ${route} upstream unreachable.`, errorName(err));
      return reply(502, { error: "upstream_unreachable" });
    }

    if (!upstream.ok) {
      // Upstream error bodies can quote (part of) the key, so they are never passed through.
      await upstream.body?.cancel();
      console.error(`${LOG_PREFIX} ${route} upstream returned HTTP ${upstream.status}.`);
      return upstream.status === 429
        ? reply(429, { error: "upstream_rate_limited" }, retryAfterOf(upstream))
        : reply(502, { error: "upstream_error", upstream_status: upstream.status });
    }

    if (route === "/groq/transcriptions") {
      const text = await upstream.text();
      return new Response(text, {
        status: 200,
        headers: {
          ...cors,
          vary: "Origin",
          "content-type": upstream.headers.get("content-type") ?? "text/plain; charset=utf-8",
          "cache-control": "no-store",
        },
      });
    }

    const parsed: unknown = await upstream.json().catch(() => null);
    const token = route === "/openai/token" ? pickOpenAIClientSecret(parsed) : pickDeepgramGrant(parsed);
    if (!token) {
      console.error(`${LOG_PREFIX} ${route} upstream reply had an unexpected shape.`);
      return reply(502, { error: "upstream_bad_response" });
    }
    return reply(200, token);
  };
}

function normalizeBasePath(basePath: string | undefined): string {
  if (!basePath) return "";
  const trimmed = basePath.replace(/\/+$/, "");
  return trimmed.startsWith("/") || trimmed === "" ? trimmed : `/${trimmed}`;
}

function matchRoute(path: string, basePath: string): Route | null {
  if (!path.startsWith(basePath)) return null;
  const rest = path.slice(basePath.length);
  return (ROUTES as readonly string[]).includes(rest) ? (rest as Route) : null;
}

function keyFor(route: Route, keys: ProviderKeys): string | undefined {
  if (route === "/groq/transcriptions") return keys.groq || undefined;
  if (route === "/openai/token") return keys.openai || undefined;
  return keys.deepgram || undefined;
}

function corsHeaders(origin: string): Record<string, string> {
  return {
    "access-control-allow-origin": origin,
    "access-control-allow-methods": "POST, OPTIONS",
    "access-control-allow-headers": "content-type, authorization",
    "access-control-allow-credentials": "true",
    "access-control-max-age": "600",
  };
}

function json(status: number, body: unknown, headers: Record<string, string>): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...headers, vary: "Origin", "content-type": "application/json", "cache-control": "no-store" },
  });
}

function retryAfterOf(upstream: Response): Record<string, string> {
  const value = upstream.headers.get("retry-after");
  return value && /^\d+$/.test(value) ? { "retry-after": value } : {};
}

function errorName(err: unknown): string {
  return err instanceof Error ? err.name : typeof err;
}

/** Read the body up to `max` bytes; anything larger is rejected without buffering it all. */
async function readCapped(request: Request, max: number): Promise<Uint8Array | "too_large"> {
  const declared = Number(request.headers.get("content-length"));
  // Leave the unread body alone: the runtime discards it with the request.
  if (Number.isFinite(declared) && declared > max) return "too_large";
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > max) {
      await reader.cancel();
      return "too_large";
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return out;
}

/**
 * Fixed-window per-client limit plus a daily cap across all clients. Returns `true` to allow or
 * the number of seconds to wait.
 */
function createMemoryLimiter(options: RateLimitOptions | undefined): (clientId: string) => true | number {
  const { windowMs, max, dailyMax } = { ...DEFAULT_RATE_LIMIT, ...options };
  const windows = new Map<string, { start: number; count: number }>();
  let day = "";
  let dayCount = 0;

  return (clientId) => {
    const now = Date.now();
    if (windows.size > 10_000) {
      for (const [id, w] of windows) if (now - w.start >= windowMs) windows.delete(id);
    }
    let w = windows.get(clientId);
    if (!w || now - w.start >= windowMs) {
      w = { start: now, count: 0 };
      windows.set(clientId, w);
    }
    if (w.count >= max) return Math.max(1, Math.ceil((w.start + windowMs - now) / 1000));

    const today = new Date(now).toISOString().slice(0, 10);
    if (today !== day) {
      day = today;
      dayCount = 0;
    }
    if (dayCount >= dailyMax) {
      const midnight = Date.parse(`${today}T00:00:00.000Z`) + 86_400_000;
      return Math.max(1, Math.ceil((midnight - now) / 1000));
    }
    w.count += 1;
    dayCount += 1;
    return true;
  };
}
