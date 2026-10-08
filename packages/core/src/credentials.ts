import { configError, sessionError, type WordInkError } from "./errors.js";
import type { CloudProvider } from "./providers.js";

/** How a cloud provider is reached (KTD3). */
export type Credentials =
  /** The developer's `@wordink/server` relay holds the key. */
  | { kind: "relay"; endpoint: string }
  /** A provider key used directly from the page: localhost only. */
  | { kind: "devKey"; key: string };

const LOCAL_HOSTNAMES = new Set(["localhost", "127.0.0.1", "[::1]", "::1"]);

/** Dev-key mode is allowed only on these hostnames (KTD3). */
export function isLocalHostname(hostname: string | undefined): boolean {
  return hostname !== undefined && LOCAL_HOSTNAMES.has(hostname);
}

/**
 * Validates `endpoint` / `devKey` for a cloud provider. Throws a `Config` error, before any network
 * call, when neither or both are given, or when a dev key is used off localhost. `hostname` is the
 * page's `location.hostname`.
 */
export function resolveCredentials(
  provider: CloudProvider,
  options: { endpoint?: string | undefined; devKey?: string | undefined },
  hostname: string | undefined,
): Credentials {
  const { endpoint, devKey } = options;
  if (endpoint && devKey) {
    throw configError(
      "Pass either `endpoint` or `devKey`, not both.",
      "Use `endpoint` (your @wordink/server relay) in production; `devKey` is for localhost experiments only.",
    );
  }
  if (devKey) {
    if (!isLocalHostname(hostname)) {
      throw configError(
        `\`devKey\` is only allowed on localhost, not on "${hostname ?? "(no location)"}".`,
        "A key in the page is visible to anyone who can open it. Deploy @wordink/server and pass its URL as `endpoint`.",
      );
    }
    console.warn(
      `[@wordink/core] Dev-key mode: your ${provider} key is visible to anyone with access to this page. ` +
        "Use `endpoint` with @wordink/server outside local development.",
    );
    return { kind: "devKey", key: devKey };
  }
  if (endpoint) {
    try {
      new URL(endpoint, globalThis.location?.href);
    } catch {
      throw configError(`\`endpoint\` is not a valid URL: "${endpoint}".`, "Pass your relay's base URL, e.g. \"/api/wordink\".");
    }
    return { kind: "relay", endpoint: endpoint.replace(/\/+$/, "") };
  }
  throw configError(
    `The ${provider} provider needs an \`endpoint\` (or a \`devKey\` on localhost).`,
    "Deploy @wordink/server and pass its URL as `endpoint`.",
  );
}

/** Absolute URL for a relay route; relative endpoints resolve against the page. */
export function relayUrl(endpoint: string, route: string): string {
  return new URL(`${endpoint}${route}`, globalThis.location?.href).href;
}

/**
 * Mints a short-lived credential through the relay (`POST {endpoint}/openai/token` returning
 * `{ value, expires_at }`, or `POST {endpoint}/deepgram/token` returning `{ access_token, expires_in }`).
 * Rejects with a `WordInkError` (AuthFailed, RateLimited or ProviderDown).
 */
export async function mintToken(
  provider: Exclude<CloudProvider, "groq">,
  endpoint: string,
  fetchImpl: typeof fetch = fetch,
  signal?: AbortSignal,
): Promise<string> {
  let response: Response;
  try {
    response = await fetchImpl(relayUrl(endpoint, `/${provider}/token`), {
      method: "POST",
      credentials: "include",
      ...(signal ? { signal } : {}),
    });
  } catch (cause) {
    throw withCause(sessionError("ProviderDown"), cause);
  }
  if (!response.ok) {
    const code = response.status === 401 || response.status === 403 ? "AuthFailed" : response.status === 429 ? "RateLimited" : "ProviderDown";
    throw sessionError(code);
  }
  const body: unknown = await response.json().catch(() => null);
  const token =
    typeof body === "object" && body !== null
      ? (body as Record<string, unknown>)[provider === "openai" ? "value" : "access_token"]
      : undefined;
  if (typeof token !== "string" || token === "") throw sessionError("ProviderDown");
  return token;
}

function withCause(error: WordInkError, cause: unknown): WordInkError {
  Object.defineProperty(error, "cause", { value: cause });
  return error;
}
