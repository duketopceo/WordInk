// Shared (Workers-safe) runtime vocabulary primitives. No node:* imports here: the
// Node-only file backend lives in file-vocabulary.ts.

/** One runtime vocabulary record, as exposed to readers (tombstones never leave the store). */
export interface VocabularyTerm {
  term: string;
  /** Free-text attribution of the writer (an agent name, an operator label). */
  addedBy: string;
  /** The writing token's unique id — unambiguous provenance across agents reusing labels. */
  addedByTokenId?: string;
  addedAt: string;
}

export type VocabularyErrorCode = "unreadable" | "vocabulary_full" | "invalid_term" | "changed";

/** Store failures carry a machine-readable code the route layer maps to an HTTP status. */
export class VocabularyStoreError extends Error {
  constructor(
    readonly code: VocabularyErrorCode,
    message: string,
  ) {
    super(message);
    this.name = "VocabularyStoreError";
  }
}

export interface VocabularyStore {
  /**
   * Active runtime terms, in insertion order. Throws {@link VocabularyStoreError} `unreadable`
   * when the backing store exists but cannot be parsed — distinct from an empty store.
   */
  list(): Promise<VocabularyTerm[]>;
  /**
   * Store terms, atomically as one batch. A case-folded match on a stored term respells it to
   * the new spelling (attribution moves to this writer); a folded match on a removed record
   * revives it. Implementations reject a batch that would exceed {@link MAX_TERMS} active
   * records with `vocabulary_full` — nothing partially persists.
   */
  add(terms: readonly string[], addedBy: { label: string; tokenId?: string | undefined }): Promise<void>;
  /**
   * Remove a term by case-folded match; echoes the stored spelling removed, or null when
   * absent. `removedBy` carries delete provenance onto the tombstone where the backend
   * keeps one.
   */
  remove(term: string, removedBy?: { label: string; tokenId?: string | undefined }): Promise<string | null>;
}

/** Per-term and per-file caps (R5, R7): kept small because terms ride every transcription. */
export const MAX_TERM_CHARS = 200;
export const MAX_TERMS = 200;
/** Vocabulary request bodies are JSON — 64 KB is far beyond a 200-term cap's needs (R8). */
export const VOCAB_MAX_BODY_BYTES = 64 * 1024;

const CONTROL_CHARS = /[\x00-\x1f\x7f]/;

/**
 * The canonical stored form of a term — trimmed, non-empty, control-char-free, at most
 * {@link MAX_TERM_CHARS} — or null when the input cannot be one (hostile-input rules;
 * the file is hand-editable too, so readers normalize rather than trusting).
 */
export function cleanTerm(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const term = value.trim();
  return term !== "" && !CONTROL_CHARS.test(term) && term.length <= MAX_TERM_CHARS ? term : null;
}
