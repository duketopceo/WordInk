// Shared upstream fakes for the gateway tests: a fetch mock that dispatches per provider key
// plus the small call-inspection helpers around it.
import { expect, vi } from "vitest";
import {
  constantTimeEqualHex,
  generateToken,
  hashToken,
  type TokenRecord,
  type TokenScope,
  type TokenStore,
} from "../../src/gateway/tokens.js";
import {
  MAX_TERMS,
  VocabularyStoreError,
  type VocabularyStore,
  type VocabularyTerm,
} from "../../src/gateway/vocabulary-store.js";

export type Responder = (signal: AbortSignal | undefined) => Response | Promise<Response>;

export const ok = (text: string): Responder => () => Response.json({ text });

/** Never answers; rejects with the abort reason like real fetch does. */
export const hang: Responder = (signal) =>
  new Promise((_resolve, reject) => {
    signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
  });

/** `status(code, headers)` responder whose error body carries `marker`, for leak assertions. */
export const statusWith =
  (marker: string) =>
  (code: number, headers: Record<string, string> = {}): Responder =>
  () =>
    Response.json({ error: { message: `upstream detail ${marker}` } }, { status: code, headers });

function keyOf(init: RequestInit | undefined): string {
  return (new Headers(init?.headers).get("authorization") ?? "").replace(/^(Bearer|Token) /, "");
}

export type FetchMock = ReturnType<typeof vi.fn<typeof fetch>>;

export function upstreamCall(fetchMock: FetchMock, i = 0): { url: string; init: RequestInit; headers: Headers } {
  const c = fetchMock.mock.calls[i];
  if (!c) throw new Error(`no upstream call #${i}`);
  const [input, init] = c;
  const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
  return { url, init: init ?? {}, headers: new Headers(init?.headers) };
}

export function sentForm(fetchMock: FetchMock, i = 0): FormData {
  const { init } = upstreamCall(fetchMock, i);
  expect(init.body).toBeInstanceOf(FormData);
  return init.body as FormData;
}

/** A fetch mock that dispatches per provider key; queue responses with `on`, the last repeats. */
export function makeUpstream() {
  const responders = new Map<string, Responder[]>();
  const fetchMock = vi.fn<typeof fetch>(async (_input, init) => {
    const queue = responders.get(keyOf(init));
    if (!queue || queue.length === 0) throw new Error(`no responder for ${keyOf(init)}`);
    const r = queue.length > 1 ? queue.shift()! : queue[0]!;
    return r(init?.signal ?? undefined);
  });
  return {
    fetchMock,
    on: (key: string, ...rs: Responder[]): void => {
      responders.set(key, rs);
    },
    keysCalled: (): string[] => fetchMock.mock.calls.map(([, init]) => keyOf(init)),
  };
}

interface StoredToken extends TokenRecord {
  hash: string;
}

/** In-memory TokenStore for route tests (the file backend is covered by tokens.test.ts). */
export class MemoryTokenStore implements TokenStore {
  private readonly records = new Map<string, StoredToken>();

  async create(label: string, scope: TokenScope = "device") {
    const token = generateToken();
    const rec: StoredToken = {
      id: crypto.randomUUID(),
      label,
      hash: await hashToken(token),
      createdAt: new Date().toISOString(),
      scope,
    };
    this.records.set(rec.id, rec);
    return { id: rec.id, label, scope, token };
  }

  async verify(token: string) {
    const hash = await hashToken(token);
    let found: StoredToken | undefined;
    for (const rec of this.records.values()) {
      if (constantTimeEqualHex(rec.hash, hash)) found = rec;
    }
    return !found || found.revokedAt ? null : { id: found.id, label: found.label, scope: found.scope ?? "device" };
  }

  async list(): Promise<TokenRecord[]> {
    return [...this.records.values()].map((r) => ({
      id: r.id,
      label: r.label,
      createdAt: r.createdAt,
      ...(r.revokedAt ? { revokedAt: r.revokedAt } : {}),
      scope: r.scope ?? "device",
    }));
  }

  async revoke(id: string) {
    const rec = this.records.get(id);
    if (!rec || rec.revokedAt) return false;
    rec.revokedAt = new Date().toISOString();
    return true;
  }
}

/** In-memory VocabularyStore mirroring FileVocabularyStore's folded semantics. */
export class MemoryVocabularyStore implements VocabularyStore {
  readonly records: VocabularyTerm[] = [];
  failWith: VocabularyStoreError | Error | null = null;

  async list(): Promise<VocabularyTerm[]> {
    if (this.failWith) throw this.failWith;
    return this.records.map((r) => ({ ...r }));
  }

  async add(terms: readonly string[], addedBy: { label: string; tokenId?: string | undefined }): Promise<void> {
    if (this.failWith) throw this.failWith;
    // Check before writing, like FileVocabularyStore: nothing partially persists.
    const held = new Set(this.records.map((r) => r.term.toLowerCase()));
    const fresh = new Set(terms.map((t) => t.toLowerCase()).filter((f) => !held.has(f)));
    if (this.records.length + fresh.size > MAX_TERMS) {
      throw new VocabularyStoreError("vocabulary_full", "full");
    }
    for (const term of terms) {
      const folded = term.toLowerCase();
      const i = this.records.findIndex((r) => r.term.toLowerCase() === folded);
      const rec: VocabularyTerm = {
        term,
        addedBy: addedBy.label,
        ...(addedBy.tokenId ? { addedByTokenId: addedBy.tokenId } : {}),
        addedAt: new Date().toISOString(),
      };
      if (i >= 0) this.records[i] = rec;
      else this.records.push(rec);
    }
  }

  async remove(term: string): Promise<string | null> {
    if (this.failWith) throw this.failWith;
    const i = this.records.findIndex((r) => r.term.toLowerCase() === term.toLowerCase());
    if (i < 0) return null;
    return this.records.splice(i, 1)[0]!.term;
  }
}
