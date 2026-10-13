// The desktop-facing OpenAI-compatible transcription route (KTD1). Everything here is
// Workers-safe: the Node-only token backend lives in file-tokens.ts and is never imported.
import { createMemoryLimiter, errorName, GATEWAY_LOG_PREFIX, limiterRetrySeconds, readCapped } from "../internal.js";
import type { RateLimiter, RateLimitOptions } from "../internal.js";
import { parseTranscriptionForm, type ParseError } from "./multipart.js";
import { DEFAULT_MODELS, type ResolvedEntry } from "./providers.js";
import { createRouter } from "./router.js";
import type { TokenInfo, TokenStore } from "./tokens.js";
import { vocabularyPacking } from "./vocabulary.js";
import {
  MAX_TERMS,
  VOCAB_MAX_BODY_BYTES,
  VocabularyStoreError,
  cleanTerm,
  type VocabularyStore,
  type VocabularyTerm,
} from "./vocabulary-store.js";

/** The route path (under `basePath`) the gateway serves when configured. */
export const GATEWAY_TRANSCRIPTIONS_PATH = "/v1/audio/transcriptions";

/**
 * `GET|POST|DELETE {basePath}/v1/vocabulary` — runtime vocabulary for the agent correction
 * loop. Exact collection-path dispatch (KTD13): no parameterized route, so `%2F` and
 * prefix-collision bugs (`/v1/vocabularyevil`) cannot exist. Admin tokens only.
 */
export const GATEWAY_VOCABULARY_PATH = "/v1/vocabulary";

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
  /**
   * Runtime vocabulary store (R9). Present → `/v1/vocabulary` answers and every transcription
   * resolves the live config∪runtime union per request; absent → the vocab routes 404 (embedders
   * opt in) and transcription uses the static `vocabulary` alone.
   */
  vocabularyStore?: VocabularyStore | undefined;
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

  // The live union resolved once per transcribe() call (KTD10): runtime terms join the
  // config set without rebuilding the router, so cooldown state survives every mutation.
  // The one place store failure degrades instead of answering 500 — a corrupt
  // vocabulary.json must never take dictation down (R10).
  let vocabDegradedWarned = false;
  const liveVocabulary = async (): Promise<readonly string[] | undefined> => {
    const store = config.vocabularyStore;
    if (!store) return undefined; // the construction-time set applies
    try {
      return effectiveUnion(config.vocabulary ?? [], await store.list()).map((entry) => entry.term);
    } catch (err) {
      if (!vocabDegradedWarned) {
        vocabDegradedWarned = true;
        console.warn(`${GATEWAY_LOG_PREFIX} vocabulary store unreadable; transcribing with config terms only.`, errorName(err));
      }
      return undefined;
    }
  };

  /**
   * The vocabulary routes' gate: same Bearer check as devices, then scope — a device token is
   * a clean 403, not a mystery 401 — and only then the per-token budget, so rejected calls
   * never burn transcription budget (R3).
   */
  const authorizeAdmin = async (request: Request): Promise<TokenInfo | Response> => {
    const token = bearerToken(request);
    const device = token === null ? null : await verify(config.tokenStore, token);
    if (device === null) {
      return openaiError(401, "A valid WordInk device token is required.", "invalid_request_error", {
        code: "invalid_api_key",
      });
    }
    if (device.scope !== "admin") {
      return openaiError(403, "This operation requires an admin token (`tokens create --admin`).", "invalid_request_error", {
        code: "insufficient_permissions",
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

  const store = config.vocabularyStore;
  const configTerms = config.vocabulary ?? [];

  /** Store failures map onto the API's failure modes (R10); unexpected ones rethrow. */
  const storeError = (err: unknown): Response | null => {
    if (!(err instanceof VocabularyStoreError)) return null;
    if (err.code === "vocabulary_full") {
      return openaiError(409, `The runtime vocabulary holds at most ${MAX_TERMS} terms; remove some first.`, "invalid_request_error", {
        code: "vocabulary_full",
      });
    }
    if (err.code === "invalid_term") {
      return openaiError(400, "Vocabulary terms must be non-empty, control-char-free and <= 200 characters.", "invalid_request_error");
    }
    return openaiError(500, "The vocabulary store could not be read.", "server_error");
  };

  const capacityOf = (union: UnionEntry[]) => {
    const packed = vocabularyPacking(union.map((e) => e.term));
    return { packed, prompt_chars: packed.promptChars, keyterm_count: packed.keyterm.filter(Boolean).length };
  };

  const vocabGet = async (): Promise<Response> => {
    const union = effectiveUnion(configTerms, await store!.list());
    const { packed, prompt_chars, keyterm_count } = capacityOf(union);
    const data = union.map((entry, i) => ({
      term: entry.term,
      source: entry.source,
      pinned: entry.pinned,
      ...(entry.runtimeOverride ? { runtimeOverride: true } : {}),
      effective: { prompt: packed.prompt[i] === true, keyterm: packed.keyterm[i] === true },
      ...(entry.provenance
        ? {
            addedBy: entry.provenance.addedBy,
            ...(entry.provenance.addedByTokenId ? { addedByTokenId: entry.provenance.addedByTokenId } : {}),
            addedAt: entry.provenance.addedAt,
          }
        : {}),
    }));
    return new Response(JSON.stringify({ object: "list", data, prompt_chars, keyterm_count }), { headers: JSON_HEADERS });
  };

  /** Resolve one deletion against a snapshot: the outcome + the config spelling it reverts to. */
  const resolveRemove = (
    folded: string,
    byFold: ReadonlyMap<string, VocabularyTerm>,
    configByFold: ReadonlyMap<string, string>,
  ): { outcome: "removed" | "not_found" | "term_pinned"; revertedTo?: string } => {
    const rec = byFold.get(folded);
    const cfg = configByFold.get(folded);
    if (!rec && !cfg) return { outcome: "not_found" };
    if (!rec || (cfg !== undefined && rec.term === cfg)) return { outcome: "term_pinned" };
    return cfg === undefined ? { outcome: "removed" } : { outcome: "removed", revertedTo: cfg };
  };

  const vocabPost = async (request: Request, device: TokenInfo): Promise<Response> => {
    const body = await readCapped(request, VOCAB_MAX_BODY_BYTES);
    if (body === "too_large") return openaiError(413, "The request body is too large.", "invalid_request_error");
    let parsed: unknown;
    try {
      parsed = JSON.parse(new TextDecoder().decode(body));
    } catch {
      return openaiError(400, "The request body is not valid JSON.", "invalid_request_error");
    }
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      return openaiError(400, "The request body must be a JSON object.", "invalid_request_error");
    }
    const { terms, remove, dryRun } = parsed as { terms?: unknown; remove?: unknown; dryRun?: unknown };
    if ((terms !== undefined && !Array.isArray(terms)) || (remove !== undefined && !Array.isArray(remove))) {
      return openaiError(400, '`terms` and `remove` must be arrays of terms.', "invalid_request_error");
    }
    if (dryRun !== undefined && typeof dryRun !== "boolean") {
      return openaiError(400, "`dryRun` must be a boolean.", "invalid_request_error");
    }
    const adds = (terms ?? []).map(cleanTerm);
    const removes = (remove ?? []).map(cleanTerm);
    if (adds.some((t) => t === null) || removes.some((t) => t === null)) {
      return openaiError(400, "Vocabulary terms must be non-empty, control-char-free and <= 200 characters.", "invalid_request_error");
    }
    if (adds.length > MAX_TERMS || removes.length > MAX_TERMS) {
      return openaiError(400, `At most ${MAX_TERMS} terms per request.`, "invalid_request_error", { code: "too_many_terms" });
    }
    if (adds.length === 0 && removes.length === 0) {
      return openaiError(400, "`terms` must contain at least one term (or provide `remove`).", "invalid_request_error");
    }

    const dry = dryRun === true;
    const runtime = await store!.list();
    const byFold = foldMap(runtime);
    const configByFold = foldConfig(configTerms);

    // Resolve adds against the runtime snapshot, then config, then terms pending in this batch.
    const toStore = new Map<string, string>();
    let netNew = 0;
    const addRows = (adds as string[]).map((term) => {
      const folded = term.toLowerCase();
      const rec = byFold.get(folded);
      if (rec) {
        const outcome = rec.term === term ? "already_present" : "updated";
        if (outcome === "updated") toStore.set(folded, term);
        return { term, outcome };
      }
      const cfg = configByFold.get(folded);
      if (cfg !== undefined) {
        const outcome = cfg === term ? "pinned" : "config_shadowed";
        if (outcome === "config_shadowed") {
          toStore.set(folded, term);
          netNew++;
        }
        return { term, outcome };
      }
      const pending = toStore.get(folded);
      const outcome = pending === undefined ? "added" : pending === term ? "already_present" : "updated";
      if (pending === undefined) netNew++;
      if (outcome !== "already_present") toStore.set(folded, term);
      return { term, outcome };
    });

    const postCount = runtime.length + netNew;
    if (postCount > MAX_TERMS) {
      if (!dry) {
        return openaiError(409, `The runtime vocabulary holds at most ${MAX_TERMS} terms; remove some first.`, "invalid_request_error", {
          code: "vocabulary_full",
        });
      }
    }
    if (!dry && toStore.size > 0) {
      try {
        await store!.add([...toStore.values()], { label: device.label, tokenId: device.id });
      } catch (err) {
        const mapped = storeError(err);
        if (mapped) return mapped;
        throw err;
      }
    }

    // Removes resolve against post-add state: fresh list on a real write, overlay on a dry run.
    const postByFold = dry ? new Map(byFold) : foldMap(await store!.list());
    if (dry) for (const [folded, term] of toStore) postByFold.set(folded, { term, addedBy: device.label, addedAt: "" });
    const removeRows: { term: string; outcome: string; reverted_to?: string }[] = [];
    for (const term of removes as string[]) {
      const folded = term.toLowerCase();
      const r = resolveRemove(folded, postByFold, configByFold);
      if (r.outcome === "removed" && !dry) {
        try {
          const echo = await store!.remove(term, { label: device.label, tokenId: device.id });
          if (echo === null) {
            removeRows.push({ term, outcome: "not_found" });
            continue;
          }
          removeRows.push({ term: echo, outcome: "removed", ...(r.revertedTo ? { reverted_to: r.revertedTo } : {}) });
          continue;
        } catch (err) {
          const mapped = storeError(err);
          if (mapped) return mapped;
          throw err;
        }
      }
      removeRows.push({ term, outcome: r.outcome, ...(r.revertedTo ? { reverted_to: r.revertedTo } : {}) });
    }

    // Capacity reports the post-change union (or the dry-run hypothetical one): a term that
    // would land past a merge budget must never report itself effective (KTD14).
    const postUnion = effectiveUnion(configTerms, [...postByFold.values()]);
    const { packed, prompt_chars, keyterm_count } = capacityOf(postUnion);
    const effectiveOf = (term: string) => {
      const i = postUnion.findIndex((e) => e.term.toLowerCase() === term.toLowerCase());
      return i >= 0
        ? { prompt: packed.prompt[i] === true, keyterm: packed.keyterm[i] === true }
        : { prompt: false, keyterm: false };
    };
    const data = [
      ...addRows.map((r) => ({ ...r, effective: effectiveOf(r.term) })),
      ...removeRows,
    ];
    return new Response(
      JSON.stringify({
        object: "list",
        data,
        prompt_chars,
        keyterm_count,
        post_change_terms: postCount,
        ...(dry ? { dry_run: true, ...(postCount > MAX_TERMS ? { capacity_exceeded: true } : {}) } : {}),
      }),
      { headers: JSON_HEADERS },
    );
  };

  const vocabDelete = async (request: Request, device: TokenInfo): Promise<Response> => {
    const body = await readCapped(request, VOCAB_MAX_BODY_BYTES);
    if (body === "too_large") return openaiError(413, "The request body is too large.", "invalid_request_error");
    let parsed: unknown;
    try {
      parsed = JSON.parse(new TextDecoder().decode(body));
    } catch {
      return openaiError(400, "The request body is not valid JSON.", "invalid_request_error");
    }
    const term =
      typeof parsed === "object" && parsed !== null && !Array.isArray(parsed)
        ? cleanTerm((parsed as { term?: unknown }).term)
        : null;
    if (term === null) {
      return openaiError(400, "`term` must be a non-empty, control-char-free string of at most 200 characters.", "invalid_request_error");
    }
    const runtime = await store!.list();
    const r = resolveRemove(term.toLowerCase(), foldMap(runtime), foldConfig(configTerms));
    if (r.outcome === "not_found") {
      return openaiError(404, `No vocabulary term matches ${JSON.stringify(term)}.`, "invalid_request_error", { code: "term_not_found" });
    }
    if (r.outcome === "term_pinned") {
      return openaiError(409, `${JSON.stringify(term)} is config-pinned; edit gateway.json to change it.`, "invalid_request_error", { code: "term_pinned" });
    }
    const echo = await store!.remove(term, { label: device.label, tokenId: device.id });
    if (echo === null) {
      return openaiError(404, `No vocabulary term matches ${JSON.stringify(term)}.`, "invalid_request_error", { code: "term_not_found" });
    }
    const union = effectiveUnion(configTerms, await store!.list());
    const { prompt_chars, keyterm_count } = capacityOf(union);
    return new Response(
      JSON.stringify({
        object: "list",
        data: [{ term: echo, outcome: "removed", ...(r.revertedTo ? { reverted_to: r.revertedTo } : {}) }],
        prompt_chars,
        keyterm_count,
      }),
      { headers: JSON_HEADERS },
    );
  };

  const vocabHandle = async (request: Request): Promise<Response> => {
    if (!store) return openaiError(404, "The vocabulary route is not enabled.", "invalid_request_error");
    if (request.method !== "GET" && request.method !== "POST" && request.method !== "DELETE") {
      return openaiError(405, "Only GET, POST and DELETE requests are supported.", "invalid_request_error", {
        headers: { allow: "GET, POST, DELETE" },
      });
    }
    const device = await authorizeAdmin(request);
    if (device instanceof Response) return device;
    try {
      if (request.method === "GET") return await vocabGet();
      if (request.method === "POST") return await vocabPost(request, device);
      return await vocabDelete(request, device);
    } catch (err) {
      const mapped = storeError(err);
      if (mapped) return mapped;
      throw err;
    }
  };

  return async function gateway(request: Request): Promise<Response> {
    const path = new URL(request.url).pathname;
    if (path.endsWith(GATEWAY_VOCABULARY_PATH)) {
      try {
        return await vocabHandle(request);
      } catch (err) {
        console.error(`${GATEWAY_LOG_PREFIX} vocabulary request failed unexpectedly.`, errorName(err));
        return openaiError(500, "The vocabulary request failed internally.", "server_error");
      }
    }
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

      const vocabulary = await liveVocabulary();
      const result = await router.transcribe(parsed.form.file, {
        prompt: parsed.form.prompt,
        language: parsed.form.language,
        responseFormat: parsed.form.responseFormat,
        temperature: parsed.form.temperature,
        ...(vocabulary !== undefined ? { vocabulary } : {}),
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

/** One row of the config∪runtime union (R9 union semantics; the shape GET reports). */
interface UnionEntry {
  /** The spelling providers actually receive — the runtime override's when one exists. */
  term: string;
  source: "config" | "runtime";
  /** Config terms own their presence: nothing under `source:"config"` is deletable. */
  pinned: boolean;
  /** A runtime record respelled this config term (KTD15): the runtime casing wins the union. */
  runtimeOverride?: boolean;
  /** The runtime record behind this row, for provenance fields. */
  provenance?: VocabularyTerm;
}

/** Active runtime records keyed by case-folded spelling (first record wins on file dupes). */
function foldMap(runtime: readonly VocabularyTerm[]): Map<string, VocabularyTerm> {
  const map = new Map<string, VocabularyTerm>();
  for (const rec of runtime) {
    const term = cleanTerm(rec.term);
    if (term && !map.has(term.toLowerCase())) map.set(term.toLowerCase(), rec);
  }
  return map;
}

/** Config terms keyed by case-folded spelling, config order, first spelling kept. */
function foldConfig(configTerms: readonly string[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const raw of configTerms) {
    const term = cleanTerm(raw);
    if (term && !map.has(term.toLowerCase())) map.set(term.toLowerCase(), term);
  }
  return map;
}

/**
 * config ∪ runtime, folded-unique: config terms first in config order, then runtime terms in
 * insertion order — the `mergePrompt` "operator terms first" precedent. A runtime case-variant
 * of a config term is a spelling override: the runtime record wins the union's casing while
 * presence stays config-pinned (KTD15).
 */
function effectiveUnion(configTerms: readonly string[], runtime: readonly VocabularyTerm[]): UnionEntry[] {
  const byFold = foldMap(runtime);
  const union: UnionEntry[] = [];
  const claimed = new Set<string>();
  for (const term of foldConfig(configTerms).values()) {
    claimed.add(term.toLowerCase());
    const rec = byFold.get(term.toLowerCase());
    union.push(
      rec && rec.term !== term
        ? { term: rec.term, source: "config", pinned: true, runtimeOverride: true, provenance: rec }
        : { term, source: "config", pinned: true },
    );
  }
  for (const rec of runtime) {
    const term = cleanTerm(rec.term);
    if (!term) continue;
    const folded = term.toLowerCase();
    if (claimed.has(folded)) continue;
    claimed.add(folded);
    union.push({ term: rec.term, source: "runtime", pinned: false, provenance: rec });
  }
  return union;
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
    if (!id || seen.has(id)) continue;
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
