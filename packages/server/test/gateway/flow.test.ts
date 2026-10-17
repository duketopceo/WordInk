import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  applyFlowCleanup,
  applyTonePreset,
  buildFlowCleanupPrompt,
  createRelay,
  expandSnippets,
  formatSpokenLists,
  generateToken,
  hashToken,
  learnVocabularyFromEdit,
  resolveCourseCorrections,
  stripFillers,
  type FlowOptions,
  type FlowTone,
  type TokenInfo,
  type TokenRecord,
  type TokenStore,
} from "../../src/index.js";
import { makeUpstream, ok } from "./fakes.js";

interface FlowContractFixture {
  stripFillers: { id: string; input: string; expected: string }[];
  resolveCourseCorrections: { id: string; input: string; expected: string }[];
  formatSpokenLists: { id: string; input: string; expected: string }[];
  expandSnippets: { id: string; input: string; snippets: Record<string, string>; expected: string }[];
  applyTonePreset: { id: string; input: string; tone: FlowTone; expected: string }[];
  applyFlowCleanup: { id: string; input: string; options: FlowOptions; expected: string }[];
  learnVocabularyFromEdit: {
    id: string;
    dictated: string;
    edited: string;
    expected: { addedTerms: string[]; replacements: Record<string, string> };
  }[];
}

const FLOW_CONTRACT = JSON.parse(
  readFileSync(new URL("../../../../tests/fixtures/flow-contract.json", import.meta.url), "utf8"),
) as FlowContractFixture;

describe("stripFillers", () => {
  it("removes hesitation sounds (um, uh, erm, ah) and preserves capitalization", () => {
    expect(stripFillers("Um, we should, uh, ship the release today.")).toBe(
      "We should ship the release today.",
    );
    expect(stripFillers("Erm I think ah this works")).toBe("I think this works");
  });

  it("removes comma-flanked discourse fillers (like, you know) while keeping real words", () => {
    expect(stripFillers("We need, like, three more tests, you know, before merging.")).toBe(
      "We need, three more tests, before merging.",
    );
    expect(stripFillers("You know, I like umbrellas and you know Alice.")).toBe(
      "I like umbrellas and you know Alice.",
    );
  });
});

describe("resolveCourseCorrections", () => {
  it("resolves mid-sentence backtracking markers (no wait, scratch that, actually no, I mean)", () => {
    expect(resolveCourseCorrections("let us meet at 2 pm no wait 3 pm")).toBe(
      "let us meet at 3 pm",
    );
    expect(resolveCourseCorrections("send it to Bob scratch that Alice")).toBe(
      "send it to Alice",
    );
    expect(resolveCourseCorrections("deploy to staging, actually no, production")).toBe(
      "deploy to production",
    );
    expect(resolveCourseCorrections("we need five I mean six reviewers")).toBe(
      "we need six reviewers",
    );
  });

  it("replaces the whole clause when the correction is a new full clause", () => {
    expect(
      resolveCourseCorrections("I will call him tomorrow, scratch that, let us email him today"),
    ).toBe("let us email him today");
  });
});

describe("formatSpokenLists", () => {
  it("formats spoken ordinal sequences (first, second, third) into a numbered list", () => {
    expect(formatSpokenLists("first ship the gateway second test iOS third write docs")).toBe(
      "1. Ship the gateway\n2. Test iOS\n3. Write docs",
    );
  });

  it("preserves a lead-in phrase before ordinal list items", () => {
    expect(
      formatSpokenLists("Three priorities today: first fix CI, second cut a release, third update docs."),
    ).toBe("Three priorities today:\n1. Fix CI\n2. Cut a release\n3. Update docs");
  });

  it("formats spoken bullet sequences (bullet one, bullet two) into a bulleted list", () => {
    expect(formatSpokenLists("bullet one buy milk bullet two walk the dog")).toBe(
      "- Buy milk\n- Walk the dog",
    );
  });
});

describe("expandSnippets", () => {
  it("expands phrase triggers case-insensitively on word boundaries", () => {
    const snippets = {
      "my calendly": "https://cal.com/luke",
      "my email": "luke@wordink.io",
    };
    expect(expandSnippets("send my calendly or My Email today", snippets)).toBe(
      "send https://cal.com/luke or luke@wordink.io today",
    );
  });

  it("prefers longer triggers when triggers overlap", () => {
    const snippets = {
      "cal": "https://cal.com",
      "cal link": "https://cal.com/luke/30min",
    };
    expect(expandSnippets("here is my cal link", snippets)).toBe(
      "here is my https://cal.com/luke/30min",
    );
  });
});

describe("applyTonePreset", () => {
  it("casual: lowercases single-sentence conversational text and strips trailing period", () => {
    expect(applyTonePreset("Sounds good to me.", "casual")).toBe("sounds good to me");
    expect(applyTonePreset("Are you joining the call?", "casual")).toBe("are you joining the call?");
    expect(applyTonePreset("That is awesome!", "casual")).toBe("that is awesome!");
  });

  it("work: applies clean sentence case and terminal punctuation", () => {
    expect(applyTonePreset("we finished the gateway review. it looks solid", "work")).toBe(
      "We finished the gateway review. It looks solid.",
    );
  });

  it("email: formats greeting, body paragraph, and sign-off with blank lines", () => {
    expect(
      applyTonePreset("hi sarah thanks for sending the roadmap over best luke", "email"),
    ).toBe("Hi Sarah,\n\nThanks for sending the roadmap over.\n\nBest,\nLuke");
  });

  it("code: converts spoken casing and symbol phrases into identifiers", () => {
    expect(applyTonePreset("camel case foo bar", "code")).toBe("fooBar");
    expect(applyTonePreset("snake case foo bar", "code")).toBe("foo_bar");
    expect(applyTonePreset("pascal case flow controller", "code")).toBe("FlowController");
    expect(applyTonePreset("kebab case word ink web", "code")).toBe("word-ink-web");
    expect(applyTonePreset("screaming snake case max retry count", "code")).toBe("MAX_RETRY_COUNT");
  });

  it("verbatim: returns text completely unmodified", () => {
    const raw = "um let us meet at 2 pm no wait 3 pm";
    expect(applyTonePreset(raw, "verbatim")).toBe(raw);
    expect(applyFlowCleanup(raw, { tone: "verbatim" })).toBe(raw);
  });
});

describe("applyFlowCleanup", () => {
  it("runs the full deterministic pipeline in sequence", () => {
    const raw = "um send it to Bob scratch that Alice with my calendly";
    const cleaned = applyFlowCleanup(raw, {
      tone: "work",
      snippets: { "my calendly": "https://cal.com/luke" },
    });
    expect(cleaned).toBe("Send it to Alice with https://cal.com/luke.");
  });
});

describe("buildFlowCleanupPrompt", () => {
  it("builds a cleanup prompt with tone, vocabulary, and snippets", () => {
    const prompt = buildFlowCleanupPrompt({
      tone: "email",
      vocabulary: ["WordInk", "Omarchy"],
      snippets: { "my calendly": "https://cal.com/luke" },
    });
    expect(prompt).toContain("Tone preset: email");
    expect(prompt).toContain("WordInk, Omarchy");
    expect(prompt).toContain('"my calendly" -> "https://cal.com/luke"');
  });

  it("switches to Command mode when selectedText is provided", () => {
    const prompt = buildFlowCleanupPrompt({
      tone: "work",
      selectedText: "Let us meet tomorrow.",
    });
    expect(prompt).toContain("Transform the selected text according to the user's spoken instruction");
    expect(prompt).toContain("Let us meet tomorrow.");
  });
});

describe("learnVocabularyFromEdit", () => {
  it("learns corrected proper nouns and custom spellings from post-insertion edits", () => {
    const diff = learnVocabularyFromEdit(
      "We deployed word ink on omarchy with stephen today.",
      "We deployed WordInk on Omarchy with Stefan today.",
    );
    expect(diff.addedTerms).toEqual(["WordInk", "Omarchy", "Stefan"]);
    expect(diff.replacements).toEqual({
      "word ink": "WordInk",
      omarchy: "Omarchy",
      stephen: "Stefan",
    });
  });

  it("ignores pure deletions", () => {
    const diff = learnVocabularyFromEdit(
      "Please send the quarterly report to Alice right now.",
      "Please send the report to Alice.",
    );
    expect(diff).toEqual({ addedTerms: [], replacements: {} });
  });

  it("ignores large rewrites exceeding 40% of tokens", () => {
    const diff = learnVocabularyFromEdit(
      "Let us schedule the design review for Tuesday morning.",
      "Cancel the entire meeting and ship the hotfix tonight.",
    );
    expect(diff).toEqual({ addedTerms: [], replacements: {} });
  });
});

describe("gateway integration with flow post-processing", () => {
  const GROQ_KEY = "gsk_test_FLOW_0123456789";

  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("applies deterministic Flow cleanup to /v1/audio/transcriptions when gateway.flow is configured", async () => {
    const { fetchMock, on } = makeUpstream();
    vi.stubGlobal("fetch", fetchMock);
    on(GROQ_KEY, ok("um let us meet at 2 pm no wait 3 pm with my calendly"));

    const rawToken = generateToken();
    const tokenHash = await hashToken(rawToken);
    const tokenStore: TokenStore = {
      async verify(t: string): Promise<TokenInfo | null> {
        return (await hashToken(t)) === tokenHash ? { id: "dev-1", label: "ios-keyboard" } : null;
      },
      async create(): Promise<{ id: string; label: string; token: string }> {
        throw new Error("unused");
      },
      async list(): Promise<TokenRecord[]> {
        return [];
      },
      async revoke(): Promise<boolean> {
        return false;
      },
    };

    const handler = createRelay({
      keys: {},
      gateway: {
        tokenStore,
        providers: [{ provider: "groq", key: GROQ_KEY }],
        flow: {
          tone: "work",
          snippets: { "my calendly": "https://cal.com/luke" },
        },
      },
    });

    const fd = new FormData();
    fd.append("file", new Blob([new Uint8Array([1, 2, 3])], { type: "audio/wav" }), "audio.wav");
    fd.append("model", "whisper-1");

    const res = await handler(
      new Request("http://127.0.0.1:8941/v1/audio/transcriptions", {
        method: "POST",
        headers: { authorization: `Bearer ${rawToken}` },
        body: fd,
      }),
    );

    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({
      text: "Let us meet at 3 pm with https://cal.com/luke.",
    });
  });
});

describe("shared multi-OS tests/fixtures/flow-contract.json parity", () => {
  it("verifies all stripFillers contract cases", () => {
    for (const c of FLOW_CONTRACT.stripFillers) {
      expect(stripFillers(c.input), c.id).toBe(c.expected);
    }
  });

  it("verifies all resolveCourseCorrections contract cases", () => {
    for (const c of FLOW_CONTRACT.resolveCourseCorrections) {
      expect(resolveCourseCorrections(c.input), c.id).toBe(c.expected);
    }
  });

  it("verifies all formatSpokenLists contract cases", () => {
    for (const c of FLOW_CONTRACT.formatSpokenLists) {
      expect(formatSpokenLists(c.input), c.id).toBe(c.expected);
    }
  });

  it("verifies all expandSnippets contract cases", () => {
    for (const c of FLOW_CONTRACT.expandSnippets) {
      expect(expandSnippets(c.input, c.snippets), c.id).toBe(c.expected);
    }
  });

  it("verifies all applyTonePreset contract cases", () => {
    for (const c of FLOW_CONTRACT.applyTonePreset) {
      expect(applyTonePreset(c.input, c.tone), c.id).toBe(c.expected);
    }
  });

  it("verifies all applyFlowCleanup contract cases", () => {
    for (const c of FLOW_CONTRACT.applyFlowCleanup) {
      expect(applyFlowCleanup(c.input, c.options), c.id).toBe(c.expected);
    }
  });

  it("verifies all learnVocabularyFromEdit contract cases", () => {
    for (const c of FLOW_CONTRACT.learnVocabularyFromEdit) {
      expect(learnVocabularyFromEdit(c.dictated, c.edited), c.id).toEqual(c.expected);
    }
  });
});
