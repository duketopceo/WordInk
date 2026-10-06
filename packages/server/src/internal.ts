// Shared internals for index.ts and gateway/index.ts. Not a public surface on its own:
// index.ts decides which of these are re-exported.

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

export const DEFAULT_RATE_LIMIT: Required<RateLimitOptions> = { windowMs: 60_000, max: 20, dailyMax: 1000 };

/** `err.name` for an Error, otherwise a short description: never the message (it can quote secrets). */
export function errorName(err: unknown): string {
  return err instanceof Error ? err.name : typeof err;
}

/** Read the body up to `max` bytes; anything larger is rejected without buffering it all. */
export async function readCapped(request: Request, max: number): Promise<Uint8Array | "too_large"> {
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
export function createMemoryLimiter(options: RateLimitOptions | undefined): (clientId: string) => true | number {
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
