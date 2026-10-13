// The desktop-facing OpenAI-compatible transcription route (KTD1). Everything here is
// Workers-safe: the Node-only token backend lives in file-tokens.ts and is never imported.
import { createMemoryLimiter, errorName, GATEWAY_LOG_PREFIX, isRecord, limiterRetrySeconds, readCapped } from "../internal.js";
import type { RateLimiter, RateLimitOptions } from "../internal.js";
import { parseTranscriptionForm, type ParseError } from "./multipart.js";
import { DEFAULT_MODELS, type ResolvedEntry } from "./providers.js";
import { createRouter } from "./router.js";
import type { TokenInfo, TokenStore } from "./tokens.js";
import { capacityOf, effectiveUnion, foldConfig, foldMap } from "./vocabulary.js";
import {
  MAX_TERMS,
  VOCAB_MAX_BODY_BYTES,
  VocabularyStoreError,
  cleanTerm,
  type VocabularyStore,
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

  // Shared gates: Bearer token check, then the per-token budget (KTD9). A spent
  // budget is a 429 in the OpenAI shape with Retry-After — on transcriptions this runs
  // before any body buffering so an over-quota request never holds maxBodyBytes of memory.
  const verifyBearer = async (request: Request): Promise<TokenInfo | Response> => {
    const token = bearerToken(request);
    const device = token === null ? null : await verify(config.tokenStore, token);
    if (device === null) {
      return openaiError(401, "A valid WordInk device token is required.", "invalid_request_error", {
        code: "invalid_api_key",
      });
    }
    return device;
  };

  const chargeBudget = async (device: TokenInfo, request: Request): Promise<Response | null> => {
    const verdict = await limiter(device.id, request);
    if (verdict !== true) {
      return openaiError(429, "Rate limit reached for this device token.", "rate_limit_exceeded", {
        code: "rate_limit_exceeded",
        headers: { "retry-after": String(limiterRetrySeconds(verdict)) },
      });
    }
    return null;
  };

  const authorizeDevice = async (request: Request): Promise<TokenInfo | Response> => {
    const device = await verifyBearer(request);
    if (device instanceof Response) return device;
    return (await chargeBudget(device, request)) ?? device;
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
      const terms = effectiveUnion(config.vocabulary ?? [], await store.list()).map((entry) => entry.term);
      vocabDegradedWarned = false; // a recovery re-arms the latch so a later failure warns again
      return terms;
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
    const device = await verifyBearer(request);
    if (device instanceof Response) return device;
    if (device.scope !== "admin") {
      return openaiError(403, "This operation requires an admin token (`tokens create --admin`).", "invalid_request_error", {
        code: "insufficient_permissions",
      });
    }
    return (await chargeBudget(device, request)) ?? device;
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
    if (err.code === "changed") {
      return openaiError(500, "The vocabulary store changed mid-write; retry the request.", "server_error");
    }
    return openaiError(500, "The vocabulary store could not be read.", "server_error");
  };

  /** A capped JSON object body, or the OpenAI-shaped error to answer with (R8). */
  const jsonBody = async (request: Request): Promise<Record<string, unknown> | Response> => {
    const body = await readCapped(request, VOCAB_MAX_BODY_BYTES);
    if (body === "too_large") return openaiError(413, "The request body is too large.", "invalid_request_error");
    let parsed: unknown;
    try {
      parsed = JSON.parse(new TextDecoder().decode(body));
    } catch {
      return openaiError(400, "The request body is not valid JSON.", "invalid_request_error");
    }
    if (!isRecord(parsed)) {
      return openaiError(400, "The request body must be a JSON object.", "invalid_request_error");
    }
    return parsed;
  };

  const vocabGet = async (store: VocabularyStore): Promise<Response> => {
    const runtime = await store.list();
    const union = effectiveUnion(configTerms, runtime);
    const { packed, prompt_chars, keyterm_count } = capacityOf(union);
    const data = union.map((entry, i) => ({
      term: entry.term,
      source: entry.source,
      pinned: entry.pinned,
      ...(entry.runtimeOverride ? { runtimeOverride: true } : {}),
      effective: { prompt: packed.prompt[i] === true, keyterm: packed.keyterm[i] === true },
      ...(entry.provenance
        ? {
            ...(entry.provenance.addedBy !== undefined ? { addedBy: entry.provenance.addedBy } : {}),
            ...(entry.provenance.addedByTokenId ? { addedByTokenId: entry.provenance.addedByTokenId } : {}),
            ...(entry.provenance.addedAt !== undefined ? { addedAt: entry.provenance.addedAt } : {}),
          }
        : {}),
    }));
    return new Response(
      JSON.stringify({ object: "list", data, prompt_chars, keyterm_count, runtime_terms: runtime.length, max_terms: MAX_TERMS }),
      { headers: JSON_HEADERS },
    );
  };

  /** Resolve one deletion against a snapshot: the outcome + the config spelling it reverts to. */
  const resolveRemove = (
    folded: string,
    byFold: ReadonlyMap<string, { term: string }>,
    configByFold: ReadonlyMap<string, string>,
  ): { outcome: "removed" | "not_found" | "term_pinned"; revertedTo?: string } => {
    const rec = byFold.get(folded);
    const cfg = configByFold.get(folded);
    if (!rec) return cfg === undefined ? { outcome: "not_found" } : { outcome: "term_pinned" };
    // Any live runtime record is removable — even a same-spelling zombie hiding under a
    // config term (its removal frees the cap slot and the config spelling reverts).
    return cfg === undefined ? { outcome: "removed" } : { outcome: "removed", revertedTo: cfg };
  };

  const vocabPost = async (request: Request, device: TokenInfo, store: VocabularyStore): Promise<Response> => {
    const parsed = await jsonBody(request);
    if (parsed instanceof Response) return parsed;
    const { terms, remove, dryRun } = parsed;
    if ((terms !== undefined && !Array.isArray(terms)) || (remove !== undefined && !Array.isArray(remove))) {
      return openaiError(400, '`terms` and `remove` must be arrays of terms.', "invalid_request_error");
    }
    if (dryRun !== undefined && typeof dryRun !== "boolean") {
      return openaiError(400, "`dryRun` must be a boolean.", "invalid_request_error");
    }
    const adds = (terms ?? []).map(cleanTerm);
    const removes = (remove ?? []).map(cleanTerm);
    if (!adds.every((t): t is string => t !== null) || !removes.every((t): t is string => t !== null)) {
      return openaiError(400, "Vocabulary terms must be non-empty, control-char-free and <= 200 characters.", "invalid_request_error");
    }
    if (adds.length > MAX_TERMS || removes.length > MAX_TERMS) {
      return openaiError(400, `At most ${MAX_TERMS} terms per request.`, "invalid_request_error", { code: "too_many_terms" });
    }
    if (adds.length === 0 && removes.length === 0) {
      return openaiError(400, "`terms` must contain at least one term (or provide `remove`).", "invalid_request_error");
    }

    const dry = dryRun === true;
    const runtime = await store.list();
    const byFold = foldMap(runtime);
    const configByFold = foldConfig(configTerms);

    // Resolve adds against the runtime snapshot, then config, then terms pending in this
    // batch. `netNew` counts distinct folded records, never rows — a folded duplicate
    // respells its pending record instead of counting twice.
    const toStore = new Map<string, string>();
    const reverts = new Map<string, string>();
    let netNew = 0;
    const addRows = adds.map((term) => {
      const folded = term.toLowerCase();
      const cfg = configByFold.get(folded);
      const rec = byFold.get(folded);
      if (rec) {
        // Respelling an override back to the exact config spelling reverts it: the
        // runtime record is deleted and config re-owns the row. Persisting it would
        // create a zombie — union-invisible, undeletable, cap-consuming.
        if (cfg === term) {
          reverts.set(folded, term);
          return { term, outcome: "reverted", reverted_to: cfg };
        }
        const outcome = rec.term === term ? "already_present" : "updated";
        if (outcome === "updated") toStore.set(folded, term);
        return { term, outcome };
      }
      if (cfg !== undefined) {
        if (cfg === term) {
          // An earlier row staged a shadow for this key; the config spelling winning
          // last means nothing should persist — unstage it so "pinned" stays honest.
          if (toStore.delete(folded)) netNew--;
          return { term, outcome: "pinned" };
        }
        if (!toStore.has(folded)) netNew++;
        toStore.set(folded, term);
        return { term, outcome: "config_shadowed" };
      }
      const pending = toStore.get(folded);
      const outcome = pending === undefined ? "added" : pending === term ? "already_present" : "updated";
      if (outcome !== "already_present") toStore.set(folded, term);
      if (pending === undefined) netNew++;
      return { term, outcome };
    });

    // The cap sees the request's net effect: removes (and override reverts) free slots, so a
    // rotation batch that fits after its deletions is not a false vocabulary_full.
    const landing = new Set<string>();
    for (const term of removes) {
      const folded = term.toLowerCase();
      if (byFold.has(folded) || toStore.has(folded)) landing.add(folded);
    }
    for (const folded of reverts.keys()) landing.add(folded);
    const postCount = runtime.length + netNew - landing.size;
    if (postCount > MAX_TERMS && !dry) {
      // Atomic: the cap check precedes every write, so an over-cap batch persists nothing.
      return openaiError(409, `The runtime vocabulary holds at most ${MAX_TERMS} terms; remove some first.`, "invalid_request_error", {
        code: "vocabulary_full",
      });
    }

    // The batch applies in dependency order: removes of pre-existing records and override
    // reverts run FIRST so they free the cap slots the adds then need (a rotation batch at
    // the cap must not trip the store's own ceiling mid-flight). Removes that target a
    // term staged in this same request resolve last. `postByFold` mirrors every applied
    // op so the reported union, effective flags and post-change count are post-change —
    // and folded-duplicate removes resolve sequentially (the second is an honest
    // not_found). A dry run plays the same sequence against the snapshot with no writes.
    const attr = { label: device.label, tokenId: device.id };
    const postByFold: Map<string, { term: string }> = new Map(byFold);
    type RemoveRow = { term: string; outcome: string; reverted_to?: string };
    const removeRows: (RemoveRow | undefined)[] = removes.map(() => undefined);
    const deferred: number[] = [];

    const applyRemove = async (i: number): Promise<void> => {
      const term = removes[i]!;
      const folded = term.toLowerCase();
      const r = resolveRemove(folded, postByFold, configByFold);
      if (r.outcome !== "removed") {
        removeRows[i] = { term, outcome: r.outcome, ...(r.revertedTo ? { reverted_to: r.revertedTo } : {}) };
        return;
      }
      const stored = postByFold.get(folded)!;
      postByFold.delete(folded);
      if (dry) {
        // Dry-run removes echo the stored spelling they would take.
        removeRows[i] = { term: stored.term, outcome: "removed", ...(r.revertedTo ? { reverted_to: r.revertedTo } : {}) };
        return;
      }
      const echo = await store.remove(term, attr);
      removeRows[i] =
        echo === null
          ? { term, outcome: "not_found" }
          : { term: echo, outcome: "removed", ...(r.revertedTo ? { reverted_to: r.revertedTo } : {}) };
    };

    for (let i = 0; i < removes.length; i++) {
      const folded = removes[i]!.toLowerCase();
      if (toStore.has(folded)) deferred.push(i); // targets a staged add — resolve after it lands
      else await applyRemove(i);
    }
    for (const [folded, reverted] of reverts) {
      postByFold.delete(folded);
      if (!dry) await store.remove(reverted, attr);
    }
    if (!dry && toStore.size > 0) await store.add([...toStore.values()], attr);
    for (const [folded, term] of toStore) postByFold.set(folded, { term });
    for (const i of deferred) await applyRemove(i);

    // Report the persisted truth on a real write — a fresh list also picks up any
    // cross-process mutation; on a dry run the op-mirror snapshot is the hypothetical.
    const finalByFold: Map<string, { term: string }> = dry ? postByFold : foldMap(await store.list());

    // Capacity reports the post-change union (or the dry-run hypothetical one): a term that
    // would land past a merge budget must never report itself effective (KTD14).
    const postUnion = effectiveUnion(configTerms, [...finalByFold.values()]);
    const { packed, prompt_chars, keyterm_count } = capacityOf(postUnion);
    const effectiveOf = (term: string) => {
      const i = postUnion.findIndex((e) => e.term.toLowerCase() === term.toLowerCase());
      return i >= 0
        ? { prompt: packed.prompt[i] === true, keyterm: packed.keyterm[i] === true }
        : { prompt: false, keyterm: false };
    };
    const data = [
      ...addRows.map((r) => ({ ...r, effective: effectiveOf(r.term) })),
      ...removeRows.flatMap((r) => (r ? [r] : [])),
    ];
    return new Response(
      JSON.stringify({
        object: "list",
        data,
        prompt_chars,
        keyterm_count,
        post_change_terms: finalByFold.size,
        ...(dry ? { dry_run: true, ...(finalByFold.size > MAX_TERMS ? { capacity_exceeded: true } : {}) } : {}),
      }),
      { headers: JSON_HEADERS },
    );
  };

  const vocabDelete = async (request: Request, device: TokenInfo, store: VocabularyStore): Promise<Response> => {
    const parsed = await jsonBody(request);
    if (parsed instanceof Response) return parsed;
    const term = cleanTerm(parsed.term);
    if (term === null) {
      return openaiError(400, "`term` must be a non-empty, control-char-free string of at most 200 characters.", "invalid_request_error");
    }
    const runtime = await store.list();
    const r = resolveRemove(term.toLowerCase(), foldMap(runtime), foldConfig(configTerms));
    if (r.outcome === "not_found") {
      return openaiError(404, `No vocabulary term matches ${JSON.stringify(term)}.`, "invalid_request_error", { code: "term_not_found" });
    }
    if (r.outcome === "term_pinned") {
      return openaiError(409, `${JSON.stringify(term)} is config-pinned; edit gateway.json to change it.`, "invalid_request_error", { code: "term_pinned" });
    }
    const echo = await store.remove(term, { label: device.label, tokenId: device.id });
    if (echo === null) {
      return openaiError(404, `No vocabulary term matches ${JSON.stringify(term)}.`, "invalid_request_error", { code: "term_not_found" });
    }
    const union = effectiveUnion(configTerms, await store.list());
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
      if (request.method === "GET") return await vocabGet(store);
      if (request.method === "POST") return await vocabPost(request, device, store);
      return await vocabDelete(request, device, store);
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
