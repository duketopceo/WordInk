import { transcribe, type AudioFile, type ResolvedEntry, type TranscribeOptions, type VerboseTranscript } from "./providers.js";

export const DEFAULT_GATEWAY_UPSTREAM_TIMEOUT_MS = 15_000;
export const DEFAULT_GATEWAY_DEADLINE_MS = 30_000;
export const DEFAULT_COOLDOWN_MS = 30_000;
export const MAX_COOLDOWN_MS = 300_000;

const LOG_PREFIX = "[@wordink/server] gateway:";

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
  const vocabulary = options.vocabulary ?? [];
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
      const started = now();
      const deadline = started + deadlineMs;
      for (const i of candidates(started)) {
        const entry = entries[i]!;
        const label = `${entry.provider}#${i}`;
        const remaining = deadline - now();
        if (remaining <= 0) return timedOut();

        const result = await attempt(entry, audio, { ...request, vocabulary }, Math.min(attemptMs, remaining));
        if (result.ok) {
          coolUntil.delete(i);
          return result.verbose
            ? { ok: true, text: result.text, verbose: result.verbose }
            : { ok: true, text: result.text };
        }
        if (result.kind === "client") {
          console.error(`${LOG_PREFIX} ${label} rejected the request (HTTP ${result.status ?? "?"}).`);
          return clientError(result.status ?? 400);
        }

        const cooldown =
          result.retryAfter !== undefined ? Math.min(result.retryAfter * 1000, MAX_COOLDOWN_MS) : DEFAULT_COOLDOWN_MS;
        coolUntil.set(i, now() + cooldown);
        console.error(
          `${LOG_PREFIX} ${label} failed (${result.status === undefined ? "no response" : `HTTP ${result.status}`}); ` +
            `cooling down ${Math.round(cooldown / 1000)} s.`,
        );
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
