/** Deepgram: mint a short-lived JWT via the auth grant endpoint. */
export const DEEPGRAM_GRANT_URL = "https://api.deepgram.com/v1/auth/grant";

export interface DeepgramGrant {
  access_token: string;
  expires_in: number;
}

export function requestDeepgramGrant(apiKey: string, ttlSeconds: number, timeoutMs: number): Promise<Response> {
  return fetch(DEEPGRAM_GRANT_URL, {
    method: "POST",
    headers: { authorization: `Token ${apiKey}`, "content-type": "application/json" },
    body: JSON.stringify({ ttl_seconds: ttlSeconds }),
    signal: AbortSignal.timeout(timeoutMs),
  });
}

/** Keep only the JWT and its TTL. */
export function pickDeepgramGrant(json: unknown): DeepgramGrant | null {
  if (typeof json !== "object" || json === null) return null;
  const { access_token, expires_in } = json as Record<string, unknown>;
  if (typeof access_token !== "string" || typeof expires_in !== "number") return null;
  return { access_token, expires_in };
}
