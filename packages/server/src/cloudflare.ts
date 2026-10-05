import { createRelay, type ProviderKeys, type RelayConfig, type RelayHandler } from "./index.js";

/** The subset of a Workers `ExecutionContext` the relay touches (none of it, today). */
export interface WorkerContext {
  waitUntil(promise: Promise<unknown>): void;
  passThroughOnException(): void;
}

/** Relay config for a Worker: `keys` default to the GROQ_API_KEY / OPENAI_API_KEY / DEEPGRAM_API_KEY secrets. */
export type WorkerRelayConfig = Omit<RelayConfig, "keys"> & { keys?: ProviderKeys };

/**
 * Build a Worker module (`export default createWorker(...)`). The factory runs once per env
 * binding object (normally once per isolate), so the built-in rate limiter keeps its state across
 * requests. That state is per isolate: for a hard global limit, pass a `rateLimit` function backed
 * by KV or a Durable Object.
 */
export function createWorker<Env extends object = Record<string, unknown>>(
  factory: (env: Env) => WorkerRelayConfig,
): { fetch(request: Request, env: Env, ctx: WorkerContext): Promise<Response> } {
  const relays = new WeakMap<object, RelayHandler>();
  return {
    fetch(request, env) {
      let relay = relays.get(env);
      if (!relay) {
        const config = factory(env);
        const vars = env as Record<string, unknown>;
        relay = createRelay({
          ...config,
          keys: config.keys ?? {
            groq: str(vars.GROQ_API_KEY),
            openai: str(vars.OPENAI_API_KEY),
            deepgram: str(vars.DEEPGRAM_API_KEY),
          },
        });
        relays.set(env, relay);
      }
      return relay(request);
    },
  };
}

function str(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}
