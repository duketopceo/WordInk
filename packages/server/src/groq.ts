/** Groq has no temporary tokens, so the relay forwards the browser's audio itself. */
export const GROQ_TRANSCRIPTIONS_URL = "https://api.groq.com/openai/v1/audio/transcriptions";

/**
 * Forward an already size-checked multipart body to Groq with the server key.
 * Only the content type travels with it: browser cookies and auth headers stay here.
 */
export function forwardGroq(apiKey: string, body: Uint8Array, contentType: string | null): Promise<Response> {
  const headers: Record<string, string> = { authorization: `Bearer ${apiKey}` };
  if (contentType) headers["content-type"] = contentType;
  return fetch(GROQ_TRANSCRIPTIONS_URL, { method: "POST", headers, body });
}
