// Shared (Workers-safe) device token primitives. No node:* imports here: Node-only
// backends live in file-tokens.ts.

export interface TokenInfo {
  id: string;
  label: string;
}

export interface TokenRecord {
  id: string;
  label: string;
  createdAt: string;
  revokedAt?: string;
}

export interface TokenStore {
  /** The device behind a live token, or null for an unknown or revoked one. */
  verify(token: string): Promise<TokenInfo | null>;
  /** Mint a token. The full token is returned here and never again. */
  create(label: string): Promise<TokenInfo & { token: string }>;
  list(): Promise<TokenRecord[]>;
  /** True if a live token with this id was revoked. */
  revoke(id: string): Promise<boolean>;
}

const TOKEN_PREFIX = "wdk_";

function base64url(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** `wdk_` + 32 random bytes, base64url. */
export function generateToken(): string {
  return TOKEN_PREFIX + base64url(crypto.getRandomValues(new Uint8Array(32)));
}

/** SHA-256 hex of a token: the only form that is ever stored. */
export async function hashToken(token: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Constant-time equality for equal-purpose hex strings (length mismatch returns false). */
export function constantTimeEqualHex(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}
