import { forwardGroq } from "./groq.js";
import { pickOpenAIClientSecret, requestOpenAIClientSecret } from "./openai.js";
import { pickDeepgramGrant, requestDeepgramGrant } from "./deepgram.js";
import {
  createMemoryLimiter,
  errorName,
  limiterRetrySeconds,
  readCapped,
  retryAfterSeconds,
  type RateLimiter,
  type RateLimitOptions,
} from "./internal.js";
import { createGatewayHandler, GATEWAY_TRANSCRIPTIONS_PATH } from "./gateway/index.js";
import type { GatewayConfig } from "./gateway/index.js";

export { GROQ_TRANSCRIPTIONS_URL } from "./groq.js";
export { OPENAI_CLIENT_SECRETS_URL, type OpenAIClientSecret } from "./openai.js";
export { DEEPGRAM_GRANT_URL, type DeepgramGrant } from "./deepgram.js";
export { DEFAULT_RATE_LIMIT, type RateLimiter, type RateLimitOptions } from "./internal.js";
export {
  DEFAULT_GATEWAY_MAX_BODY_BYTES,
  DEFAULT_GATEWAY_RATE_LIMIT,
  GATEWAY_TRANSCRIPTIONS_PATH,
  type GatewayConfig,
} from "./gateway/index.js";
export type { ProviderName, ResolvedEntry, VerboseTranscript } from "./gateway/providers.js";
export {
  constantTimeEqualHex,
  generateToken,
  hashToken,
  type TokenInfo,
  type TokenRecord,
  type TokenStore,
} from "./gateway/tokens.js";

/** Long-lived provider keys. They stay on the server; a route whose key is missing answers 503. */
export interface ProviderKeys {
  groq?: string | undefined;
  openai?: string | undefined;
  deepgram?: string | undefined;
}

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
  /**
   * Optional desktop gateway: serve `POST {basePath}/v1/audio/transcriptions`, the OpenAI
   * audio-transcription shape that apps like Voxtype and TypeWhisper already speak (KTD1). The
   * route authenticates `Bearer` device tokens through `gateway.tokenStore`; the browser `authorize`
   * and Origin checks do not apply to it. When absent, the path 404s like any other (R11).
   */
  gateway?: GatewayConfig | undefined;
}

export type RelayHandler = (request: Request) => Promise<Response>;

export const DEFAULT_MAX_BODY_BYTES = 2 * 1024 * 1024;
export const DEFAULT_TOKEN_TTL_SECONDS = 120;
export const DEFAULT_UPSTREAM_TIMEOUT_MS = 15_000;

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
    if (config.gateway) {
      // A gateway-only deployment is intentional: the gateway route authenticates device tokens
      // instead, so this is not a misconfiguration (KTD9).
      console.warn(
        `${LOG_PREFIX} No \`authorize\` hook configured: the browser relay routes are disabled ` +
          "(every browser request gets 403). The gateway route is unaffected.",
      );
    } else {
      console.error(
        `${LOG_PREFIX} No \`authorize\` hook configured: every request will be refused with 403. ` +
          "Pass authorize(request) that checks your app's own session (cookie or JWT). " +
          "CORS/Origin checks are not authentication.",
      );
    }
  }

  const basePath = normalizeBasePath(config.basePath);
  const allowedOrigins = new Set(config.allowedOrigins ?? []);
  const maxBodyBytes = config.maxBodyBytes ?? DEFAULT_MAX_BODY_BYTES;
  const ttl = config.tokenTtlSeconds ?? DEFAULT_TOKEN_TTL_SECONDS;
  const timeoutMs = config.upstreamTimeoutMs ?? DEFAULT_UPSTREAM_TIMEOUT_MS;
  const clientIdOf = config.clientId ?? defaultClientId;
  const limiter =
    typeof config.rateLimit === "function" ? config.rateLimit : createMemoryLimiter(config.rateLimit);
  const gateway = config.gateway ? createGatewayHandler(config.gateway) : null;

  return async function relay(request: Request): Promise<Response> {
    const origin = request.headers.get("origin");
    const cors = origin !== null && allowedOrigins.has(origin) ? corsHeaders(origin) : null;
    const reply = (status: number, body: unknown, extra?: Record<string, string>) =>
      json(status, body, { ...cors, ...extra });

    const path = new URL(request.url).pathname;
    // The gateway route matches before the browser-relay logic: no Origin or `authorize` checks
    // apply to it, and when no gateway is configured the path falls through to 404 (KTD1, KTD9).
    if (gateway !== null && path === `${basePath}${GATEWAY_TRANSCRIPTIONS_PATH}`) {
      return gateway(request);
    }
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
      return reply(429, { error: "rate_limited" }, { "retry-after": String(limiterRetrySeconds(verdict)) });
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
      let text: string;
      try {
        text = await upstream.text();
      } catch (err) {
        // The body can stall past the upstream timeout (or the connection drop) after the headers.
        console.error(`${LOG_PREFIX} ${route} upstream body unreadable.`, errorName(err));
        return reply(502, { error: "upstream_unreachable" });
      }
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
  const value = retryAfterSeconds(upstream.headers);
  return value !== undefined ? { "retry-after": String(value) } : {};
}
