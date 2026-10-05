/** OpenAI realtime: mint an `ek_` client secret scoped to a transcription session. */
export const OPENAI_CLIENT_SECRETS_URL = "https://api.openai.com/v1/realtime/client_secrets";

export interface OpenAIClientSecret {
  value: string;
  expires_at: number;
}

export function requestOpenAIClientSecret(apiKey: string, ttlSeconds: number): Promise<Response> {
  return fetch(OPENAI_CLIENT_SECRETS_URL, {
    method: "POST",
    headers: { authorization: `Bearer ${apiKey}`, "content-type": "application/json" },
    body: JSON.stringify({
      expires_after: { anchor: "created_at", seconds: ttlSeconds },
      session: { type: "transcription" },
    }),
  });
}

/** Keep only the secret and its expiry; everything else in the upstream reply is dropped. */
export function pickOpenAIClientSecret(json: unknown): OpenAIClientSecret | null {
  if (typeof json !== "object" || json === null) return null;
  const { value, expires_at } = json as Record<string, unknown>;
  if (typeof value !== "string" || typeof expires_at !== "number") return null;
  return { value, expires_at };
}
