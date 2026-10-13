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
 * Merge the operator vocabulary into a Whisper-style prompt: operator terms first, minus any the
 * client prompt already contains (case-insensitive, whole words), then the client prompt. The
 * result is capped at {@link PROMPT_MAX_CHARS}: terms are kept in order up to the first one that
 * doesn't fit, and the client prompt is trimmed (on a word boundary) to whatever room is left. `undefined` when there is nothing to send.
 */
export function mergePrompt(vocabulary: readonly string[], clientPrompt: string | undefined): string | undefined {
  const client = clientPrompt?.trim() ?? "";
  const terms = uniqueTerms(vocabulary).filter((t) => !containsWord(client, t));

  let head = "";
  for (const term of terms) {
    const next = head ? `${head}, ${term}` : term;
    if (next.length > PROMPT_MAX_CHARS) break;
    head = next;
  }

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
  const prompt: boolean[] = [];
  let head = "";
  let packing = true; // first-fit-break: once a term does not fit, none after it do
  for (const term of terms) {
    const next = head ? `${head}, ${term}` : term;
    if (packing && next.length <= PROMPT_MAX_CHARS) {
      head = next;
      prompt.push(true);
    } else {
      packing = false;
      prompt.push(false);
    }
  }
  return { promptChars: head.length, prompt, keyterm: terms.map((_, i) => i < DEEPGRAM_MAX_KEYTERMS) };
}
