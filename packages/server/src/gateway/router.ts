import { GATEWAY_LOG_PREFIX } from "../internal.js";
import { transcribe, type AudioFile, type ResolvedEntry, type TranscribeOptions, type VerboseTranscript } from "./providers.js";
import { uniqueTerms } from "./vocabulary.js";

const DEFAULT_GATEWAY_UPSTREAM_TIMEOUT_MS = 15_000;
const DEFAULT_GATEWAY_DEADLINE_MS = 30_000;
const DEFAULT_COOLDOWN_MS = 30_000;
const MAX_COOLDOWN_MS = 300_000;

/** The OpenAI error shape every gateway failure uses (R7). */
export interface OpenAIErrorBody {
  error: { message: string; type: string; code?: string };
}

export type RouterResult =
  | { ok: true; text: string; verbose?: VerboseTranscript }
  | { ok: false; status: number; body: OpenAIErrorBody };

export interface RouterOptions {
  /** Gateway-wide operator vocabulary merged into every attempt (KTD6). */
  vocabulary?: readonly string[] | undefined;
  /** Abort one provider attempt after this many ms and fall through. Default 15 000. */
  upstreamTimeoutMs?: number | undefined;
  /** Bound on the whole fallback chain; reaching it answers 504. Default 30 000. */
  deadlineMs?: number | undefined;
  /** Clock for cooldown bookkeeping. Default `Date.now`. */
  now?: (() => number) | undefined;
}

export interface GatewayRouter {
  transcribe(audio: AudioFile, options: Omit<TranscribeOptions, "vocabulary">): Promise<RouterResult>;
}

/**
 * Fallback router (KTD5). Entries are tried in order; 429, 5xx, 401/403, timeouts and network
 * errors fall through and cool the entry down for 30 s (or `Retry-After`, capped at 300 s).
 * 400/413/415 are returned as-is. Cooling entries are skipped unless all are cooling, in which
 * case the one whose cooldown ends first is tried. State is per router instance.
 */
export function createRouter(entries: readonly ResolvedEntry[], options: RouterOptions = {}): GatewayRouter {
  if (entries.length === 0) throw new Error("createRouter needs at least one provider entry");
  // A blank key sends `Bearer undefined` upstream, where it reads as a rejected key (401)
  // rather than the config error it is — catch it here, naming the entry, never the value.
  entries.forEach((entry, i) => {
    if (typeof entry.key !== "string" || entry.key.trim() === "") {
      throw new Error(`createRouter: providers[${i}] (${entry.provider}) has no resolved key`);
    }
  });
  // Deduped once: the operator vocabulary is static for the router's lifetime.
  const vocabulary = uniqueTerms(options.vocabulary ?? []);
  const attemptMs = options.upstreamTimeoutMs ?? DEFAULT_GATEWAY_UPSTREAM_TIMEOUT_MS;
  const deadlineMs = options.deadlineMs ?? DEFAULT_GATEWAY_DEADLINE_MS;
  const now = options.now ?? Date.now;
  /** Entry index -> time its cooldown ends. */
  const coolUntil = new Map<number, number>();

  function candidates(at: number): number[] {
    const ready = entries.map((_, i) => i).filter((i) => (coolUntil.get(i) ?? 0) <= at);
    if (ready.length > 0) return ready;
    let earliest = 0;
    for (let i = 1; i < entries.length; i++) {
      if ((coolUntil.get(i) ?? 0) < (coolUntil.get(earliest) ?? 0)) earliest = i;
    }
    return [earliest];
  }

  return {
    async transcribe(audio, request) {
      const deadline = now() + deadlineMs;
      const tried = new Set<number>();
      for (;;) {
        // Readiness is re-evaluated between attempts: a cooldown that expires while a
        // slower attempt is in flight makes its entry a candidate again this request.
        const i = candidates(now()).find((j) => !tried.has(j));
        if (i === undefined) break;
        tried.add(i);
        const entry = entries[i]!;
        const label = `${entry.provider}#${i}`;
        const remaining = deadline - now();
        if (remaining <= 0) return timedOut();

        const budget = Math.min(attemptMs, remaining);
        const attemptStart = now();
        const result = await attempt(entry, audio, { ...request, vocabulary }, budget);
        if (result.ok) {
          coolUntil.delete(i);
          return result.verbose
            ? { ok: true, text: result.text, verbose: result.verbose }
            : { ok: true, text: result.text };
        }
        if (result.kind === "client") {
          console.error(`${GATEWAY_LOG_PREFIX} ${label} rejected the request (HTTP ${result.status ?? "?"}).`);
          return clientError(result.status ?? 400);
        }

        // An attempt cut short by the shared deadline — not the entry's own budget —
        // proves nothing about the provider, so it does not earn a cooldown.
        const clipped = budget < attemptMs && result.status === undefined && now() - attemptStart >= budget;
        if (clipped) {
          console.error(`${GATEWAY_LOG_PREFIX} ${label} attempt was cut short by the deadline; not cooling it.`);
        } else {
          const cooldown =
            result.retryAfter !== undefined ? Math.min(result.retryAfter * 1000, MAX_COOLDOWN_MS) : DEFAULT_COOLDOWN_MS;
          coolUntil.set(i, now() + cooldown);
          console.error(
            `${GATEWAY_LOG_PREFIX} ${label} failed (${result.status === undefined ? "no response" : `HTTP ${result.status}`}); ` +
              `cooling down ${Math.round(cooldown / 1000)} s.`,
          );
        }
        if (now() >= deadline) return timedOut();
      }
      return {
        ok: false,
        status: 502,
        body: { error: { message: "No transcription provider is available right now.", type: "upstream_unavailable" } },
      };
    },
  };
}

/** One attempt with its own timer (fake-timer friendly, unlike `AbortSignal.timeout`). */
async function attempt(entry: ResolvedEntry, audio: AudioFile, options: TranscribeOptions, timeoutMs: number) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(new DOMException("attempt timed out", "TimeoutError")), timeoutMs);
  try {
    return await transcribe(entry, audio, options, controller.signal);
  } finally {
    clearTimeout(timer);
  }
}

function timedOut(): RouterResult {
  return {
    ok: false,
    status: 504,
    body: { error: { message: "Transcription timed out.", type: "upstream_timeout" } },
  };
}

function clientError(status: number): RouterResult {
  const message =
    status === 413
      ? "The audio file is too large."
      : status === 415
        ? "The audio format is not supported."
        : "The transcription request was rejected as invalid.";
  return { ok: false, status, body: { error: { message, type: "invalid_request_error" } } };
}
