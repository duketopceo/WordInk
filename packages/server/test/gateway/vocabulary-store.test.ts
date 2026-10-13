import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FileVocabularyStore, defaultVocabularyPath } from "../../src/gateway/file-vocabulary.js";
import { MAX_TERMS, VocabularyStoreError, cleanTerm } from "../../src/gateway/vocabulary-store.js";

// stat/rename/rm stay injectable so cross-process writes and mid-save failures are
// deterministic: vi.mock replaces the module file-vocabulary.ts binds against.
vi.mock("node:fs/promises", async () => {
  const real = await vi.importActual<typeof import("node:fs/promises")>("node:fs/promises");
  return { ...real, stat: vi.fn(real.stat), rename: vi.fn(real.rename) };
});

const fsp = vi.mocked(await import("node:fs/promises"));
const realFsp = await vi.importActual<typeof import("node:fs/promises")>("node:fs/promises");

/**
 * Counts stat calls inside one mutation so a file write can be injected at the exact
 * statKey re-check — the only point where a cross-process write produces a conflict.
 * Call 1 is load()'s stat, call 2 the post-update statKey, calls 3/4 the retry's pair.
 */
let statCalls = 0;
let injectAt: ReadonlyMap<number, string> = new Map();

const injectableStats = (): void => {
  statCalls = 0;
  injectAt = new Map();
  vi.mocked(fsp.stat).mockImplementation(async (...args) => {
    statCalls++;
    const body = injectAt.get(statCalls);
    if (body !== undefined) {
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, body);
    }
    return realFsp.stat(...args);
  });
};

let dir: string;
let path: string;
const AGENT = { label: "argus", tokenId: "tok-1" };

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "wdk-vocab-"));
  path = join(dir, "state", "vocabulary.json");
  vi.spyOn(console, "warn").mockImplementation(() => {});
  // Re-pin delegation every test: per-test mockImplementations and restoreAllMocks must
  // never leave stat/rename returning undefined into the next file op.
  vi.mocked(fsp.stat).mockImplementation(realFsp.stat);
  vi.mocked(fsp.rename).mockImplementation(realFsp.rename);
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
  vi.restoreAllMocks();
});

describe("cleanTerm", () => {
  it.each([
    ["Kurultai", "Kurultai"],
    ["  Omarchy  ", "Omarchy"],
  ])("normalizes %j to %j", (raw, want) => {
    expect(cleanTerm(raw)).toBe(want);
  });

  it.each([
    ["whitespace-only", "   "],
    ["empty", ""],
    ["a control char", "bad\tterm"],
    ["a C1 control", "badterm"],
    ["a zero-width space", "bad​term"],
    ["a bidi mark", "bad‮term"],
    ["a mid-string BOM", "ba﻿d"],
    ["201 chars", "x".repeat(201)],
    ["a non-string", 42],
    ["a null", null],
  ])("rejects %s", (_name, raw) => {
    expect(cleanTerm(raw)).toBeNull();
  });
});

describe("defaultVocabularyPath", () => {
  it("honours XDG_STATE_HOME and falls back to ~/.local/state", () => {
    expect(defaultVocabularyPath({ XDG_STATE_HOME: "/xdg" }, "/home/u")).toBe("/xdg/wordink/vocabulary.json");
    expect(defaultVocabularyPath({}, "/home/u")).toBe("/home/u/.local/state/wordink/vocabulary.json");
  });
});

describe("FileVocabularyStore", () => {
  it("add persists terms with provenance; a second instance sees them", async () => {
    await new FileVocabularyStore(path).add(["Kurultai", "Omarchy"], AGENT);
    const again = new FileVocabularyStore(path);
    expect(await again.list()).toEqual([
      { term: "Kurultai", addedBy: "argus", addedByTokenId: "tok-1", addedAt: expect.any(String) },
      { term: "Omarchy", addedBy: "argus", addedByTokenId: "tok-1", addedAt: expect.any(String) },
    ]);
    expect(statSync(path).mode & 0o777).toBe(0o600);
    expect(statSync(join(dir, "state")).mode & 0o777).toBe(0o700);
  });

  it("a missing file lists empty (not unreadable); an empty store removes to null", async () => {
    const store = new FileVocabularyStore(path);
    expect(await store.list()).toEqual([]);
    expect(await store.remove("anything")).toBeNull();
  });

  it("a corrupt file fails closed: list throws unreadable, add refuses to overwrite", async () => {
    const raw = '{"version":1,"terms":[{"term":"{truncated';
    mkdirSync(join(dir, "state"), { recursive: true });
    writeFileSync(path, raw, { mode: 0o600 });
    const store = new FileVocabularyStore(path);
    await expect(store.list()).rejects.toMatchObject({ code: "unreadable" });
    await expect(store.add(["x"], AGENT)).rejects.toMatchObject({ code: "unreadable" });
    await expect(store.remove("x")).rejects.toMatchObject({ code: "unreadable" });
    expect(readFileSync(path, "utf8")).toBe(raw); // the corrupt bytes were never overwritten
  });

  it("case-folds on add: a variant respells the stored term, a dupe keeps insertion order", async () => {
    const store = new FileVocabularyStore(path);
    await store.add(["kurultai", "Omarchy"], AGENT);
    await store.add(["Kurultai"], { label: "wisp" });
    const terms = await store.list();
    expect(terms.map((t) => t.term)).toEqual(["Kurultai", "Omarchy"]);
    expect(terms[0]!.addedBy).toBe("wisp"); // the respeller owns the spelling now
  });

  it("remove folds to the stored spelling and echoes it; the tombstone keeps delete attribution", async () => {
    const store = new FileVocabularyStore(path);
    await store.add(["Kurultai", "Omarchy"], AGENT);
    expect(await store.remove("kurultai", { label: "wisp", tokenId: "tok-2" })).toBe("Kurultai");
    expect(await store.remove("kurultai")).toBeNull();
    expect((await store.list()).map((t) => t.term)).toEqual(["Omarchy"]);
    const raw = JSON.parse(readFileSync(path, "utf8")) as { terms: Record<string, unknown>[] };
    const tombstone = raw.terms.find((t) => t.term === "Kurultai")!;
    expect(tombstone).toMatchObject({ removedBy: "wisp", removedByTokenId: "tok-2" });
    expect(tombstone.removedAt).toEqual(expect.any(String));
  });

  it("a removed term can be re-added (the tombstone is spent)", async () => {
    const store = new FileVocabularyStore(path);
    await store.add(["Kurultai"], AGENT);
    await store.remove("kurultai");
    await store.add(["Kurultai"], { label: "wisp" });
    const terms = await store.list();
    expect(terms).toEqual([
      { term: "Kurultai", addedBy: "wisp", addedAt: expect.any(String) },
    ]);
  });

  it("rejects invalid terms before touching the file", async () => {
    const store = new FileVocabularyStore(path);
    await expect(store.add(["ok", "bad\nterm"], AGENT)).rejects.toMatchObject({ code: "invalid_term" });
    expect(await store.list()).toEqual([]);
  });

  it("caps at MAX_TERMS active records; a batch that would exceed it fails wholly", async () => {
    const store = new FileVocabularyStore(path);
    await store.add(Array.from({ length: MAX_TERMS - 1 }, (_, i) => `t${i}`), AGENT);
    await expect(store.add(["one", "two"], AGENT)).rejects.toMatchObject({ code: "vocabulary_full" });
    expect(await store.list()).toHaveLength(MAX_TERMS - 1); // nothing partially persisted
    // A tombstone does not count toward the cap — re-filling after a remove works.
    await store.remove("t0");
    await store.add(["new"], AGENT);
    expect(await store.list()).toHaveLength(MAX_TERMS - 1);
  });

  it("parallel mutations serialize: N concurrent adds all land, none lost", async () => {
    const store = new FileVocabularyStore(path);
    await Promise.all(
      Array.from({ length: 10 }, (_, i) => store.add([`term-${i}`], AGENT)),
    );
    expect((await store.list()).map((t) => t.term).sort()).toEqual(
      Array.from({ length: 10 }, (_, i) => `term-${i}`).sort(),
    );
  });

  it("a cross-process write at the stat re-check retries once and merges, not clobbers", async () => {
    injectableStats();
    const store = new FileVocabularyStore(path);
    // The write lands between load() and the statKey re-check.
    injectAt = new Map([[2, JSON.stringify({ version: 1, terms: [{ term: "extern", addedBy: "b", addedAt: "t" }] })]]);
    await store.add(["from-a"], AGENT);
    // stat calls: 1 load, 2 statKey (conflict -> retry), 3 retry load, 4 retry statKey.
    expect(statCalls).toBe(4);
    expect((await store.list()).map((t) => t.term)).toEqual(["extern", "from-a"]);
  });

  it("a second cross-process write on the retry throws 'changed' rather than retry forever", async () => {
    injectableStats();
    const store = new FileVocabularyStore(path);
    // The write wins both the first check and the retry's (distinct bodies, so the key
    // provably differs — same-content writes can share mtimeMs granularity).
    injectAt = new Map([
      [2, JSON.stringify({ version: 1, terms: [{ term: "extern", addedBy: "b", addedAt: "t" }] })],
      [4, JSON.stringify({ version: 1, terms: [{ term: "extern", addedBy: "b", addedAt: "t" }, { term: "again", addedBy: "b", addedAt: "t" }] })],
    ]);
    await expect(store.add(["from-a"], AGENT)).rejects.toMatchObject({ code: "changed" });
    expect(statCalls).toBe(4); // one retry, then it gives up — no spin loop
  });

  it("a remove retried into an already-removed term echoes null, not the stale spelling", async () => {
    const store = new FileVocabularyStore(path);
    await store.add(["Victim"], AGENT);
    injectableStats();
    injectAt = new Map([[2, JSON.stringify({ version: 1, terms: [] })]]); // another process deleted it
    expect(await store.remove("victim")).toBeNull();
    expect(await store.list()).toEqual([]); // the external version won; nothing resurrected
  });

  it("a failed save leaves no tmp file behind and the old file intact", async () => {
    const store = new FileVocabularyStore(path);
    await store.add(["old"], AGENT);
    const before = readFileSync(path, "utf8");
    vi.mocked(fsp.rename).mockImplementationOnce(() => Promise.reject(new Error("EACCES simulated")));
    await expect(store.add(["new"], AGENT)).rejects.toThrow("EACCES simulated");
    expect(readdirSync(dirname(path)).filter((f) => f.endsWith(".tmp"))).toEqual([]);
    expect(readFileSync(path, "utf8")).toBe(before);
  });

  it("tombstones are bounded at MAX_TERMS: churn cannot grow the file without limit", async () => {
    const tombstones = Array.from({ length: MAX_TERMS + 5 }, (_, i) => ({
      term: `old${i}`, addedBy: "a", addedAt: "t", removedBy: "b", removedAt: "t",
    }));
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, JSON.stringify({ version: 1, terms: tombstones }));
    const store = new FileVocabularyStore(path);
    await store.add(["new"], AGENT);
    const raw = JSON.parse(readFileSync(path, "utf8")) as { terms: { term: string; removedAt?: string }[] };
    expect(raw.terms.filter((t) => t.removedAt)).toHaveLength(MAX_TERMS); // oldest 5 pruned
    expect(raw.terms.filter((t) => !t.removedAt).map((t) => t.term)).toEqual(["new"]);
    expect(raw.terms[0]!.term).toBe("old5"); // pruning drops the oldest first
  });

  it.each<[string, string]>([
    ["a null element", '{"version":1,"terms":[null]}'],
    ["a non-record element", '{"version":1,"terms":[42]}'],
    ["a record without a term", '{"version":1,"terms":[{"addedBy":"a"}]}'],
    ["a non-string term", '{"version":1,"terms":[{"term":5}]}'],
    ["a control-char term", '{"version":1,"terms":[{"term":"bad\\tterm"}]}'],
    ["a raw string element", '{"version":1,"terms":["plain"]}'],
    ["the wrong version", '{"version":2,"terms":[]}'],
  ])("a parseable file with %s is unreadable, not a TypeError", async (_name, raw) => {
    mkdirSync(join(dir, "state"), { recursive: true });
    writeFileSync(path, raw, { mode: 0o600 });
    const store = new FileVocabularyStore(path);
    await expect(store.list()).rejects.toMatchObject({ code: "unreadable" });
    expect(readFileSync(path, "utf8")).toBe(raw);
  });
});
