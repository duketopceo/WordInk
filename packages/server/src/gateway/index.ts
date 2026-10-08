// The desktop-facing OpenAI-compatible transcription route (KTD1). Everything here is
// Workers-safe: the Node-only token backend lives in file-tokens.ts and is never imported.
import { createMemoryLimiter, errorName, GATEWAY_LOG_PREFIX, limiterRetrySeconds, readCapped } from "../internal.js";
import type { RateLimiter, RateLimitOptions } from "../internal.js";
import { parseTranscriptionForm, type ParseError } from "./multipart.js";
import { DEFAULT_MODELS, type ResolvedEntry } from "./providers.js";
import { createRouter } from "./router.js";
import type { TokenInfo, TokenStore } from "./tokens.js";

/** The route path (under `basePath`) the gateway serves when configured. */
export const GATEWAY_TRANSCRIPTIONS_PATH = "/v1/audio/transcriptions";

/**
 * `GET {basePath}/v1/models` — the OpenAI models-list shape desktop clients query when they
 * offer model discovery (TypeWhisper's OpenAI Compatible engine does). It reports each
 * configured provider's effective model id, never provider names, URLs, or keys (R7 posture).
 */
export const GATEWAY_MODELS_PATH = "/v1/models";

/** `gateway.maxBodyBytes` default: Groq's upload limit (KTD7). */
export const DEFAULT_GATEWAY_MAX_BODY_BYTES = 25 * 1024 * 1024;

/**
 * The gateway route's own limiter default (KTD9): 60 requests a minute per device token and no
 * daily cap. It never shares state with the browser relay's limiter.
 */
export const DEFAULT_GATEWAY_RATE_LIMIT: Required<RateLimitOptions> = {
  windowMs: 60_000,
  max: 60,
  dailyMax: Number.POSITIVE_INFINITY,
};

export interface GatewayConfig {
  /**
   * Device-token store the route verifies `Authorization: Bearer wdk_…` against (R8, KTD3).
   * Node callers typically pass a `FileTokenStore`.
   */
  tokenStore: TokenStore;
  /**
   * Ordered provider entries with their keys already resolved (KTD2). Each request is tried
   * against them in order, falling through on 429, 5xx, timeout, network error or a rejected
   * operator key (KTD5). The client's `model` field is accepted and ignored: every entry calls
   * its own provider model (R3).
   */
  providers: readonly ResolvedEntry[];
  /** Gateway-wide operator vocabulary merged into every attempt's prompt or keyterms (KTD6, R12). */
  vocabulary?: readonly string[] | undefined;
  /** Per-token limiter settings, or a limiter function. Default {@link DEFAULT_GATEWAY_RATE_LIMIT}. */
  rateLimit?: RateLimitOptions | RateLimiter | undefined;
  /** Upload cap; larger bodies get 413 in the OpenAI shape before any provider call. Default 25 MB. */
  maxBodyBytes?: number | undefined;
  /** Abort one provider attempt after this many ms and fall through. Default 15 000 (KTD5). */
  upstreamTimeoutMs?: number | undefined;
  /** Bound on the whole fallback chain; reaching it answers 504. Default 30 000 (KTD5). */
  deadlineMs?: number | undefined;
}

const JSON_HEADERS = { "content-type": "application/json", "cache-control": "no-store" };
const TEXT_HEADERS = { "content-type": "text/plain; charset=utf-8", "cache-control": "no-store" };

/**
 * Handler for `POST {basePath}/v1/audio/transcriptions`: the OpenAI-audio shape desktop clients
 * speak (R1, R2). Bearer device tokens replace the browser relay's `authorize` + Origin checks
 * (KTD9). Every failure is answered in the OpenAI error shape, and provider detail — keys,
 * upstream bodies, which entry served — never reaches the client (R7).
 */
export function createGatewayHandler(config: GatewayConfig): (request: Request) => Promise<Response> {
  const router = createRouter(config.providers, {
    vocabulary: config.vocabulary,
    upstreamTimeoutMs: config.upstreamTimeoutMs,
    deadlineMs: config.deadlineMs,
  });
  const limiter =
    typeof config.rateLimit === "function"
      ? config.rateLimit
      : createMemoryLimiter({ ...DEFAULT_GATEWAY_RATE_LIMIT, ...config.rateLimit });
  const maxBodyBytes = config.maxBodyBytes ?? DEFAULT_GATEWAY_MAX_BODY_BYTES;

  // Shared device gate: Bearer token check, then the per-token budget (KTD9). A spent
  // budget is a 429 in the OpenAI shape with Retry-After — on transcriptions this runs
  // before any body buffering so an over-quota request never holds maxBodyBytes of memory.
  const authorizeDevice = async (request: Request): Promise<TokenInfo | Response> => {
    const token = bearerToken(request);
    const device = token === null ? null : await verify(config.tokenStore, token);
    if (device === null) {
      return openaiError(401, "A valid WordInk device token is required.", "invalid_request_error", {
        code: "invalid_api_key",
      });
    }
    const verdict = await limiter(device.id, request);
    if (verdict !== true) {
      return openaiError(429, "Rate limit reached for this device token.", "rate_limit_exceeded", {
        code: "rate_limit_exceeded",
        headers: { "retry-after": String(limiterRetrySeconds(verdict)) },
      });
    }
    return device;
  };

  return async function gateway(request: Request): Promise<Response> {
    const path = new URL(request.url).pathname;
    if (path.endsWith(GATEWAY_MODELS_PATH)) {
      if (request.method !== "GET") {
        return openaiError(405, "Only GET requests are supported.", "invalid_request_error", {
          headers: { allow: "GET" },
        });
      }
      try {
        const device = await authorizeDevice(request);
        if (device instanceof Response) return device;
        return new Response(modelsList(config.providers), { headers: JSON_HEADERS });
      } catch (err) {
        console.error(`${GATEWAY_LOG_PREFIX} models request failed unexpectedly.`, errorName(err));
        return openaiError(500, "The models request failed internally.", "server_error");
      }
    }
    if (request.method !== "POST") {
      return openaiError(405, "Only POST requests are supported.", "invalid_request_error", {
        headers: { allow: "POST" },
      });
    }

    try {
      const device = await authorizeDevice(request);
      if (device instanceof Response) return device;

      const body = await readCapped(request, maxBodyBytes);
      if (body === "too_large") {
        return openaiError(413, "The request body is too large.", "invalid_request_error");
      }

      // The request stream is consumed by the cap, so the bytes are re-wrapped for formData (KTD7).
      const parsed = await parseTranscriptionForm(body, request.headers.get("content-type"));
      if (!parsed.ok) return formError(parsed.error);

      const result = await router.transcribe(parsed.form.file, {
        prompt: parsed.form.prompt,
        language: parsed.form.language,
        responseFormat: parsed.form.responseFormat,
        temperature: parsed.form.temperature,
      });
      if (!result.ok) {
        return new Response(JSON.stringify(result.body), { status: result.status, headers: JSON_HEADERS });
      }

      if (parsed.form.responseFormat === "text") {
        return new Response(result.text, { status: 200, headers: TEXT_HEADERS });
      }
      const json =
        parsed.form.responseFormat === "verbose_json" ? (result.verbose ?? { text: result.text }) : { text: result.text };
      return new Response(JSON.stringify(json), { status: 200, headers: JSON_HEADERS });
    } catch (err) {
      // An unexpected throw (e.g. a reset body stream mid-upload) still answers in
      // the OpenAI error shape, not the adapter's bare 500.
      console.error(`${GATEWAY_LOG_PREFIX} request failed unexpectedly.`, errorName(err));
      return openaiError(500, "The transcription request failed internally.", "server_error");
    }
  };
}

/** `Authorization: Bearer <token>` — desktop clients send no cookies and no Origin (KTD9). */
function bearerToken(request: Request): string | null {
  const header = request.headers.get("authorization");
  const match = /^Bearer[ \t]+(\S+)[ \t]*$/i.exec(header ?? "");
  return match?.[1] ?? null;
}

/** A store that throws is treated as "no such token": auth fails closed, like `authorize`. */
async function verify(store: TokenStore, token: string): Promise<TokenInfo | null> {
  try {
    return await store.verify(token);
  } catch (err) {
    console.error(`${GATEWAY_LOG_PREFIX} token store threw during verify; refusing request.`, errorName(err));
    return null;
  }
}

function formError(error: ParseError): Response {
  switch (error) {
    case "missing_file":
      return openaiError(400, "The multipart field `file` is required.", "invalid_request_error", {
        code: "missing_required_parameter",
      });
    case "unsupported_response_format":
      return openaiError(400, "Unsupported response_format; use json, text or verbose_json.", "invalid_request_error");
    default:
      return openaiError(400, "The request body is not valid multipart form data.", "invalid_request_error");
  }
}

/** The OpenAI `GET /v1/models` body: each configured provider's effective model, de-duplicated. */
function modelsList(providers: readonly ResolvedEntry[]): string {
  const seen = new Set<string>();
  const data: { id: string; object: string; created: number; owned_by: string }[] = [];
  for (const entry of providers) {
    const id = entry.model ?? DEFAULT_MODELS[entry.provider];
    if (seen.has(id)) continue;
    seen.add(id);
    data.push({ id, object: "model", created: 0, owned_by: "wordink" });
  }
  return JSON.stringify({ object: "list", data });
}

function openaiError(
  status: number,
  message: string,
  type: string,
  extra: { code?: string; headers?: Record<string, string> } = {},
): Response {
  const error: { message: string; type: string; code?: string } = { message, type };
  if (extra.code) error.code = extra.code;
  return new Response(JSON.stringify({ error }), {
    status,
    headers: { ...JSON_HEADERS, ...extra.headers },
  });
}
