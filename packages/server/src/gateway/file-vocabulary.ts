// Node-only: imported by node.ts and the gateway bin, never by the Workers entry.
import { randomUUID } from "node:crypto";
import { mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import {
  MAX_TERMS,
  VocabularyStoreError,
  cleanTerm,
  type VocabularyStore,
  type VocabularyTerm,
} from "./vocabulary-store.js";

/** The on-disk record: active terms plus delete tombstones (they keep delete provenance). */
interface StoredTerm extends VocabularyTerm {
  removedBy?: string;
  removedByTokenId?: string;
  removedAt?: string;
}

interface VocabularyFile {
  version: 1;
  terms: StoredTerm[];
}

/** `$XDG_STATE_HOME/wordink/vocabulary.json`, else `~/.local/state/wordink/vocabulary.json`. */
export function defaultVocabularyPath(
  env: Record<string, string | undefined> = process.env,
  home: string = homedir(),
): string {
  const base = env.XDG_STATE_HOME || join(home, ".local", "state");
  return join(base, "wordink", "vocabulary.json");
}

/**
 * Runtime vocabulary in a JSON file, written atomically with mode 0600 — the same file
 * contract {@link FileTokenStore} keeps, plus an in-process mutation queue: HTTP writers
 * are agents in loops, not the rare CLI invocation the token store assumes, so mutations
 * serialize in-process and only the stat re-check guards cross-process races.
 */
export class FileVocabularyStore implements VocabularyStore {
  private cache: { key: string; terms: StoredTerm[] } | undefined;
  /** The file key the last read was taken from, for the write race re-check. */
  private loadedKey: string | null = null;
  /** Serializes in-process mutations — the queue is this store's only lock. */
  private queue: Promise<unknown> = Promise.resolve();
  /** Warn-once latches: a persistent failure must not log once per request. */
  private statWarned = false;
  private unreadableWarnedFor: string | null = null;

  constructor(private readonly path: string = defaultVocabularyPath()) {}

  private async read(): Promise<{ terms: StoredTerm[]; unreadable: boolean }> {
    let key: string;
    try {
      const s = await stat(this.path);
      this.statWarned = false;
      key = `${s.ino}:${s.mtimeMs}:${s.size}`;
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        this.loadedKey = null;
        return { terms: [], unreadable: false };
      }
      if (!this.statWarned) {
        this.statWarned = true;
        console.warn(`wordink gateway: cannot stat vocabulary file ${this.path}`);
      }
      return { terms: [], unreadable: true };
    }
    this.loadedKey = key;
    if (this.cache?.key === key) return { terms: this.cache.terms, unreadable: false };
    try {
      const parsed = JSON.parse(await readFile(this.path, "utf8")) as Partial<VocabularyFile>;
      if (!Array.isArray(parsed.terms)) throw new Error("no terms array");
      const terms = parsed.terms;
      this.cache = { key, terms };
      return { terms, unreadable: false };
    } catch {
      // A failed read is not cached: the next call retries, so a transient error recovers
      // instead of latching dead.
      if (this.unreadableWarnedFor !== key) {
        this.unreadableWarnedFor = key;
        console.warn(`wordink gateway: vocabulary file ${this.path} is unreadable`);
      }
      return { terms: [], unreadable: true };
    }
  }

  private async load(): Promise<StoredTerm[]> {
    const { terms, unreadable } = await this.read();
    if (unreadable) throw new VocabularyStoreError("unreadable", `vocabulary file ${this.path} is unreadable`);
    return terms;
  }

  /** The file's current (ino, mtime, size) key; null when absent, a never-matching marker on error. */
  private async statKey(): Promise<string | null> {
    try {
      const s = await stat(this.path);
      return `${s.ino}:${s.mtimeMs}:${s.size}`;
    } catch (err) {
      return (err as NodeJS.ErrnoException).code === "ENOENT" ? null : "unstatable";
    }
  }

  private async save(terms: StoredTerm[]): Promise<void> {
    await mkdir(dirname(this.path), { recursive: true, mode: 0o700 });
    const tmp = `${this.path}.${process.pid}.${randomUUID()}.tmp`;
    const body: VocabularyFile = { version: 1, terms };
    await writeFile(tmp, JSON.stringify(body, null, 2) + "\n", { mode: 0o600 });
    await rename(tmp, this.path);
    this.cache = undefined;
  }

  /**
   * Read-modify-write inside the in-process queue, retried once when the file changed under
   * us (a cross-process writer). The update fn returns the next record list, or null for
   * "no such term" — and may throw (invalid terms, the cap) which is never swallowed.
   */
  private async mutate(update: (terms: StoredTerm[]) => StoredTerm[] | null): Promise<boolean> {
    const run = this.queue.then(async () => {
      for (let tries = 0; ; tries++) {
        const loaded = await this.load();
        const keyAtLoad = this.loadedKey;
        const next = update(loaded);
        if (next === null) return false;
        if ((await this.statKey()) === keyAtLoad) {
          await this.save(next);
          return true;
        }
        if (tries === 1) {
          throw new VocabularyStoreError("changed", `vocabulary file ${this.path} changed while writing; try again`);
        }
      }
    });
    // The queue must not latch dead on a rejected run — subsequent mutations still queue.
    this.queue = run.catch(() => {});
    return run;
  }

  async list(): Promise<VocabularyTerm[]> {
    const terms = await this.load();
    return terms
      .filter((t) => !t.removedAt)
      .map(({ term, addedBy, addedByTokenId, addedAt }) => ({
        term,
        addedBy,
        ...(addedByTokenId ? { addedByTokenId } : {}),
        addedAt,
      }));
  }

  async add(terms: readonly string[], addedBy: { label: string; tokenId?: string | undefined }): Promise<void> {
    const cleaned = terms.map(cleanTerm);
    if (cleaned.some((t) => t === null)) {
      throw new VocabularyStoreError("invalid_term", "vocabulary terms must be non-empty, control-char-free and <= 200 chars");
    }
    const now = new Date().toISOString();
    await this.mutate((stored) => {
      // Elements are replaced wholesale, never mutated — the cached list must stay
      // identical to disk until the write lands.
      const next = [...stored];
      for (const term of cleaned as string[]) {
        const folded = term.toLowerCase();
        const i = next.findIndex((t) => t.term.toLowerCase() === folded);
        // A folded match — active respell or tombstone revival alike — becomes one fresh
        // active record carrying this writer's provenance. No folded duplicates exist.
        const fresh: StoredTerm = {
          term,
          addedBy: addedBy.label,
          ...(addedBy.tokenId ? { addedByTokenId: addedBy.tokenId } : {}),
          addedAt: now,
        };
        if (i >= 0) next[i] = fresh;
        else next.push(fresh);
      }
      if (next.filter((t) => !t.removedAt).length > MAX_TERMS) {
        throw new VocabularyStoreError("vocabulary_full", `runtime vocabulary holds at most ${MAX_TERMS} terms`);
      }
      return next;
    });
  }

  async remove(term: string, removedBy?: { label: string; tokenId?: string | undefined }): Promise<string | null> {
    const cleaned = cleanTerm(term);
    if (cleaned === null) return null;
    const folded = cleaned.toLowerCase();
    let removed: string | null = null;
    await this.mutate((stored) => {
      const i = stored.findIndex((t) => !t.removedAt && t.term.toLowerCase() === folded);
      if (i < 0) return null;
      const next = [...stored];
      removed = next[i]!.term;
      next[i] = {
        ...next[i]!,
        removedBy: removedBy?.label ?? "unknown",
        ...(removedBy?.tokenId ? { removedByTokenId: removedBy.tokenId } : {}),
        removedAt: new Date().toISOString(),
      };
      return next;
    });
    return removed;
  }
}
