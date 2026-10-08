import { describe, expect, it } from "vitest";
import { DEEPGRAM_MAX_KEYTERMS, PROMPT_MAX_CHARS, keyterms, mergePrompt } from "../../src/gateway/vocabulary.js";

describe("mergePrompt (Whisper-style providers)", () => {
  it("puts operator terms first, ahead of the client prompt", () => {
    expect(mergePrompt(["Omarchy", "Hyprland"], "Talking about Linux.")).toBe(
      "Omarchy, Hyprland. Talking about Linux.",
    );
  });

  it("uses the operator vocabulary alone when the client sends no prompt (AE4)", () => {
    expect(mergePrompt(["Omarchy"], undefined)).toBe("Omarchy");
    expect(mergePrompt(["Omarchy"], "   ")).toBe("Omarchy");
  });

  it("passes the client prompt through when there is no vocabulary, and is undefined when both are empty", () => {
    expect(mergePrompt([], "Just the client.")).toBe("Just the client.");
    expect(mergePrompt([], undefined)).toBeUndefined();
    expect(mergePrompt(["  ", ""], "")).toBeUndefined();
  });

  it("drops operator terms the client prompt already contains, case-insensitively", () => {
    expect(mergePrompt(["omarchy", "Voxtype", "Hyprland"], "I use OMARCHY with hyprland daily.")).toBe(
      "Voxtype. I use OMARCHY with hyprland daily.",
    );
  });

  it("matches whole words only when de-duplicating", () => {
    // "Ink" is not present as a word in "WordInk", so it is kept.
    expect(mergePrompt(["Ink"], "WordInk")).toBe("Ink. WordInk");
    // Regex metacharacters in a term are matched literally.
    expect(mergePrompt(["C++", "Node.js"], "I write C++ and Nodexjs")).toBe("Node.js. I write C++ and Nodexjs");
  });

  it("de-duplicates the operator list itself, case-insensitively, keeping the first spelling", () => {
    expect(mergePrompt(["Omarchy", "omarchy", " Omarchy ", "Asahi"], undefined)).toBe("Omarchy, Asahi");
  });

  it("caps the merged prompt at 800 characters, keeping operator terms and trimming the client prompt", () => {
    const vocab = Array.from({ length: 10 }, (_, i) => `Term${i}`);
    const client = "word ".repeat(400).trim();
    const merged = mergePrompt(vocab, client);
    expect(merged).toBeDefined();
    expect(merged!.length).toBeLessThanOrEqual(PROMPT_MAX_CHARS);
    expect(merged!.length).toBeGreaterThan(PROMPT_MAX_CHARS - 10);
    expect(merged!.startsWith(`${vocab.join(", ")}. word`)).toBe(true);
    // Trimmed on a word boundary, never mid-word.
    expect(merged!.endsWith(" word")).toBe(true);
  });

  it("stops adding operator terms that would exceed the cap, and drops the client prompt then", () => {
    const vocab = Array.from({ length: 200 }, (_, i) => `Vocabulary${String(i).padStart(3, "0")}`);
    const merged = mergePrompt(vocab, "client words");
    expect(merged!.length).toBeLessThanOrEqual(PROMPT_MAX_CHARS);
    expect(merged!.startsWith("Vocabulary000, Vocabulary001")).toBe(true);
    expect(merged).not.toContain("client words");
  });

  it("caps a client-only prompt too", () => {
    const merged = mergePrompt([], "word ".repeat(400));
    expect(merged!.length).toBeLessThanOrEqual(PROMPT_MAX_CHARS);
  });

  it("keeps the cap at 800", () => {
    expect(PROMPT_MAX_CHARS).toBe(800);
  });
});

describe("keyterms (Deepgram)", () => {
  it("returns trimmed, de-duplicated terms in operator order", () => {
    expect(keyterms([" Omarchy", "omarchy", "Hyprland", ""])).toEqual(["Omarchy", "Hyprland"]);
  });

  it("caps Deepgram at 50 keyterms", () => {
    const vocab = Array.from({ length: 80 }, (_, i) => `t${i}`);
    const terms = keyterms(vocab);
    expect(DEEPGRAM_MAX_KEYTERMS).toBe(50);
    expect(terms).toHaveLength(50);
    expect(terms[0]).toBe("t0");
    expect(terms[49]).toBe("t49");
  });
});
