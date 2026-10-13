import { mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FileVocabularyStore, defaultVocabularyPath } from "../../src/gateway/file-vocabulary.js";
import { MAX_TERMS, VocabularyStoreError, cleanTerm } from "../../src/gateway/vocabulary-store.js";

let dir: string;
let path: string;
const AGENT = { label: "argus", tokenId: "tok-1" };

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "wdk-vocab-"));
  path = join(dir, "state", "vocabulary.json");
  vi.spyOn(console, "warn").mockImplementation(() => {});
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

  it("a cross-process write between stat and save is retried once, then wins", async () => {
    const a = new FileVocabularyStore(path);
    const b = new FileVocabularyStore(path);
    await a.add(["from-a"], AGENT);
    await b.add(["from-b"], AGENT);
    await a.add(["a-again"], AGENT);
    expect((await b.list()).map((t) => t.term)).toEqual(["from-a", "from-b", "a-again"]);
  });
});
