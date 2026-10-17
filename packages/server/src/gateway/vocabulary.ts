import { cleanTerm, type VocabularyTerm } from "./vocabulary-store.js";

/** Maximum length of the prompt sent to Whisper-style providers (KTD6). */
export const PROMPT_MAX_CHARS = 800;
/** Maximum number of `keyterm` query parameters sent to Deepgram (KTD6). */
export const DEEPGRAM_MAX_KEYTERMS = 50;

/** Trimmed, non-empty, case-insensitively de-duplicated terms, first spelling kept. */
export function uniqueTerms(vocabulary: readonly string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of vocabulary) {
    const term = raw.trim();
    const folded = term.toLowerCase();
    if (!term || seen.has(folded)) continue;
    seen.add(folded);
    out.push(term);
  }
  return out;
}

function containsWord(haystack: string, term: string): boolean {
  const escaped = term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(?:^|[^\\p{L}\\p{N}])${escaped}(?:[^\\p{L}\\p{N}]|$)`, "iu").test(haystack);
}

/** Cut `text` to at most `max` characters, on a word boundary when there is one. */
function truncateWords(text: string, max: number): string {
  if (text.length <= max) return text;
  const cut = text.slice(0, max + 1);
  const space = cut.lastIndexOf(" ");
  return (space > 0 ? cut.slice(0, space) : text.slice(0, max)).trimEnd();
}

/**
 * The first-fit-break head-packing rule, in one copy both consumers share: terms pack into the
 * prompt head in order until the first one that does not fit — nothing after it does either
 * (KTD14's truthful `effective` flags depend on this matching mergePrompt exactly).
 */
function packPromptHead(terms: readonly string[]): { head: string; fits: boolean[] } {
  const fits: boolean[] = [];
  let head = "";
  let packing = true;
  for (const term of terms) {
    const next = head ? `${head}, ${term}` : term;
    if (packing && next.length <= PROMPT_MAX_CHARS) {
      head = next;
      fits.push(true);
    } else {
      packing = false;
      fits.push(false);
    }
  }
  return { head, fits };
}

/**
 * Merge the operator vocabulary into a Whisper-style prompt: operator terms first, minus any the
 * client prompt already contains (case-insensitive, whole words), then the client prompt. The
 * result is capped at {@link PROMPT_MAX_CHARS}: terms are kept in order up to the first one that
 * doesn't fit, and the client prompt is trimmed (on a word boundary) to whatever room is left. `undefined` when there is nothing to send.
 */
export function mergePrompt(vocabulary: readonly string[], clientPrompt: string | undefined): string | undefined {
  const client = clientPrompt?.trim() ?? "";
  const terms = uniqueTerms(vocabulary).filter((t) => !containsWord(client, t));
  const { head } = packPromptHead(terms);

  if (!client) return head || undefined;
  if (!head) return truncateWords(client, PROMPT_MAX_CHARS);
  const room = PROMPT_MAX_CHARS - head.length - 2; // ". "
  const tail = room > 0 ? truncateWords(client, room) : "";
  return tail ? `${head}. ${tail}` : head;
}

/** Operator vocabulary as Deepgram `keyterm`s, at most {@link DEEPGRAM_MAX_KEYTERMS}. */
export function keyterms(vocabulary: readonly string[]): string[] {
  return uniqueTerms(vocabulary).slice(0, DEEPGRAM_MAX_KEYTERMS);
}

/**
 * The capacity picture the vocabulary API reports (KTD14): which union-order terms actually
 * pack into the prompt head (`mergePrompt`'s first-fit-break against PROMPT_MAX_CHARS, no
 * client prompt) and which land in the keyterm slice. `promptChars` is the emitted head's
 * length, so a term that is stored but inert reports `{prompt:false, keyterm:false}`.
 */
export function vocabularyPacking(terms: readonly string[]): {
  promptChars: number;
  prompt: boolean[];
  keyterm: boolean[];
} {
  const { head, fits } = packPromptHead(terms);
  return { promptChars: head.length, prompt: fits, keyterm: terms.map((_, i) => i < DEEPGRAM_MAX_KEYTERMS) };
}

/**
 * A runtime record as the union needs it: the spelling, plus whatever provenance the caller
 * has. The GET path passes full {@link VocabularyTerm} records; hypothetical post-change
 * snapshots pass `{ term }` alone — provenance fields are emitted only when present.
 */
export type RuntimeSpelling = Pick<VocabularyTerm, "term"> & Partial<Omit<VocabularyTerm, "term">>;

/** One row of the config∪runtime union (R9 union semantics; the shape GET reports). */
export interface UnionEntry {
  /** The spelling providers actually receive — the runtime override's when one exists. */
  term: string;
  source: "config" | "runtime";
  /** Config terms own their presence: nothing under `source:"config"` is deletable. */
  pinned: boolean;
  /** A runtime record respelled this config term (KTD15): the runtime casing wins the union. */
  runtimeOverride?: boolean;
  /** The runtime record behind this row, for provenance fields. */
  provenance?: RuntimeSpelling;
}

/**
 * Active runtime records keyed by case-folded spelling (first record wins on file dupes). The
 * stored value's `term` is the cleaned spelling so folded comparisons never see file padding.
 */
export function foldMap(runtime: readonly RuntimeSpelling[]): Map<string, RuntimeSpelling> {
  const map = new Map<string, RuntimeSpelling>();
  for (const rec of runtime) {
    const term = cleanTerm(rec.term);
    if (term && !map.has(term.toLowerCase())) map.set(term.toLowerCase(), { ...rec, term });
  }
  return map;
}

/**
 * Config terms keyed by case-folded spelling, config order, first spelling kept. Mirrors
 * `uniqueTerms` (trim + fold + first-wins) with no length/control filter: enabling the runtime
 * store must not silently narrow which operator terms reach providers.
 */
export function foldConfig(configTerms: readonly string[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const raw of configTerms) {
    const term = raw.trim();
    if (term && !map.has(term.toLowerCase())) map.set(term.toLowerCase(), term);
  }
  return map;
}

/**
 * config ∪ runtime, folded-unique: config terms first in config order, then runtime terms in
 * insertion order — the `mergePrompt` "operator terms first" precedent. A runtime case-variant
 * of a config term is a spelling override: the runtime record wins the union's casing while
 * presence stays config-pinned (KTD15).
 */
export function effectiveUnion(configTerms: readonly string[], runtime: readonly RuntimeSpelling[]): UnionEntry[] {
  const byFold = foldMap(runtime);
  const union: UnionEntry[] = [];
  const claimed = new Set<string>();
  for (const term of foldConfig(configTerms).values()) {
    claimed.add(term.toLowerCase());
    const rec = byFold.get(term.toLowerCase());
    union.push(
      rec && rec.term !== term
        ? { term: rec.term, source: "config", pinned: true, runtimeOverride: true, provenance: rec }
        : { term, source: "config", pinned: true },
    );
  }
  for (const rec of runtime) {
    const term = cleanTerm(rec.term);
    if (!term) continue;
    const folded = term.toLowerCase();
    if (claimed.has(folded)) continue;
    claimed.add(folded);
    union.push({ term, source: "runtime", pinned: false, provenance: rec });
  }
  return union;
}

/** The capacity picture for a union: the packed head's length and the two provider budgets. */
export function capacityOf(union: UnionEntry[]): {
  packed: ReturnType<typeof vocabularyPacking>;
  prompt_chars: number;
  keyterm_count: number;
} {
  const packed = vocabularyPacking(union.map((e) => e.term));
  return { packed, prompt_chars: packed.promptChars, keyterm_count: packed.keyterm.filter(Boolean).length };
}
