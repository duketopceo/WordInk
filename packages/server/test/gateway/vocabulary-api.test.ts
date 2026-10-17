import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createRelay, type GatewayConfig } from "../../src/index.js";
import { GATEWAY_VOCABULARY_PATH } from "../../src/gateway/index.js";
import {
  MAX_TERMS,
  VOCAB_MAX_BODY_BYTES,
  VocabularyStoreError,
} from "../../src/gateway/vocabulary-store.js";
import { GROQ_AUDIO_URL } from "../../src/gateway/providers.js";
import { makeUpstream, ok, sentForm, MemoryTokenStore, MemoryVocabularyStore, type FetchMock } from "./fakes.js";

const BASE = "https://relay.test";
const GROQ_KEY_A = "gsk_test_KEY_A_0123456789";
const groqA = { provider: "groq" as const, key: GROQ_KEY_A };
const AUDIO = new Uint8Array([82, 73, 70, 70, 1, 2, 3, 4]);


let fetchMock: FetchMock;
let on: ReturnType<typeof makeUpstream>["on"];
let tokenStore: MemoryTokenStore;
let vocab: MemoryVocabularyStore;
let deviceToken: string;
let adminToken: string;
let adminId: string;
let errorSpy: ReturnType<typeof vi.spyOn>;
let warnSpy: ReturnType<typeof vi.spyOn>;

beforeEach(async () => {
  ({ fetchMock, on } = makeUpstream());
  vi.stubGlobal("fetch", fetchMock);
  errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
  warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
  tokenStore = new MemoryTokenStore();
  vocab = new MemoryVocabularyStore();
  deviceToken = (await tokenStore.create("voxtype-laptop")).token;
  const admin = await tokenStore.create("wisp-agent", "admin");
  adminToken = admin.token;
  adminId = admin.id;
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function relay(gateway: Partial<GatewayConfig> = {}) {
  return createRelay({
    keys: {},
    gateway: { tokenStore, providers: [groqA], vocabularyStore: vocab, ...gateway },
  });
}

function vocabReq(method: string, body: unknown, token: string | false = adminToken): Request {
  return new Request(`${BASE}${GATEWAY_VOCABULARY_PATH}`, {
    method,
    headers: {
      "content-type": "application/json",
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
}

function transcribeReq(token: string = deviceToken): Request {
  const fd = new FormData();
  fd.append("file", new Blob([AUDIO], { type: "audio/wav" }), "audio.wav");
  fd.append("model", "whisper-1");
  return new Request(`${BASE}/v1/audio/transcriptions`, {
    method: "POST",
    headers: { authorization: `Bearer ${token}` },
    body: fd,
  });
}

async function expectOpenAIError(res: Response, status: number, code?: string): Promise<void> {
  expect(res.status).toBe(status);
  const body = (await res.json()) as { error: { message: string; type: string; code?: string } };
  expect(body.error.type).toEqual(expect.any(String));
  if (code) expect(body.error.code).toBe(code);
  expect(Object.keys(body as object)).toEqual(["error"]);
}

async function post(terms: string[], extra: Record<string, unknown> = {}): Promise<Response> {
  return relay()(vocabReq("POST", { terms, ...extra }));
}

describe("auth (R3)", () => {
  it("missing/unknown/revoked tokens get 401 invalid_api_key; the store is never touched", async () => {
    await expectOpenAIError(await relay()(vocabReq("GET", undefined, false)), 401, "invalid_api_key");
    await expectOpenAIError(await relay()(vocabReq("GET", undefined, "wdk_garbage")), 401, "invalid_api_key");
    await tokenStore.revoke(adminId);
    await expectOpenAIError(await relay()(vocabReq("GET", undefined)), 401, "invalid_api_key");
  });

  it("a valid device token gets 403 insufficient_permissions, not a mystery 401", async () => {
    await expectOpenAIError(await relay()(vocabReq("GET", undefined, deviceToken)), 403, "insufficient_permissions");
    for (const [method, body] of [["POST", { terms: ["x"] }], ["DELETE", { term: "x" }]] as const) {
      const res = await relay()(vocabReq(method, body, deviceToken));
      await expectOpenAIError(res, 403, "insufficient_permissions");
    }
  });

  it("the scope check precedes the limiter: a device call never spends an admin's budget", async () => {
    const seen: string[] = [];
    const handler = relay({
      rateLimit: (id) => {
        seen.push(id);
        return true;
      },
    });
    await handler(vocabReq("GET", undefined, deviceToken));
    expect(seen).toEqual([]); // rejected before the limiter ran
    await handler(vocabReq("GET", undefined));
    expect(seen).toEqual([adminId]);
  });

  it("admin tokens still transcribe and list models (scope is a superset)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = relay();
    expect((await handler(transcribeReq(adminToken))).status).toBe(200);
    const res = await handler(new Request(`${BASE}/v1/models`, { headers: { authorization: `Bearer ${adminToken}` } }));
    expect(res.status).toBe(200);
  });

  it("vocab calls share the caller's per-token bucket, GET included", async () => {
    const handler = relay({ rateLimit: { max: 1 } });
    expect((await handler(vocabReq("GET", undefined))).status).toBe(200);
    await expectOpenAIError(await handler(vocabReq("GET", undefined)), 429, "rate_limit_exceeded");
  });
});

describe("dispatch and methods (KTD13)", () => {
  it("PUT and friends 405 with allow: GET, POST, DELETE", async () => {
    const res = await relay()(vocabReq("PUT", { terms: ["x"] }));
    expect(res.status).toBe(405);
    expect(res.headers.get("allow")).toBe("GET, POST, DELETE");
  });

  it("/v1/vocabularyevil is not routed to the vocabulary handler", async () => {
    const res = await relay()(new Request(`${BASE}/v1/vocabularyevil`, { headers: { authorization: `Bearer ${adminToken}` } }));
    expect(res.status).toBe(404); // falls through to the relay's own 404 shape
  });

  it("the vocab route 404s when no vocabularyStore is configured (embedders opt in)", async () => {
    const res = await createRelay({ keys: {}, gateway: { tokenStore, providers: [groqA] } })(vocabReq("GET", undefined));
    expect(res.status).toBe(404);
  });
});

describe("GET /v1/vocabulary (R4)", () => {
  it("returns the union in the OpenAI list shape with source, pinned and provenance", async () => {
    vocab.records.push(
      { term: "wisp", addedBy: "wisp-agent", addedByTokenId: adminId, addedAt: "2026-10-10T00:00:00Z" },
      { term: "kurultai", addedBy: "wisp-agent", addedAt: "2026-10-10T00:00:01Z" },
    );
    const res = await relay({ vocabulary: ["Omarchy", "Hyprland"] })(vocabReq("GET", undefined));
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      object: string;
      data: Record<string, unknown>[];
      prompt_chars: number;
      keyterm_count: number;
    };
    expect(body.object).toBe("list");
    expect(body.data.map((d) => [d.term, d.source])).toEqual([
      ["Omarchy", "config"],
      ["Hyprland", "config"],
      ["wisp", "runtime"],
      ["kurultai", "runtime"],
    ]);
    const [omarchy, , wisp] = body.data;
    expect(omarchy).toMatchObject({ pinned: true, effective: { prompt: true, keyterm: true } });
    expect(omarchy).not.toHaveProperty("addedBy");
    expect(wisp).toMatchObject({ pinned: false, addedBy: "wisp-agent", addedByTokenId: adminId });
    expect(body.prompt_chars).toBe("Omarchy, Hyprland, wisp, kurultai".length);
    expect(body.keyterm_count).toBe(4);
  });

  it("marks terms past a merge budget as inert instead of silent (KTD14)", async () => {
    // Five ~192-char terms overflow the 800-char prompt at the fifth; the k-terms fill the
    // keyterm slice so the last five report inert on both budgets. Terms are <= 200 chars
    // each (MAX_TERM_CHARS), so the prompt budget breaks mid-list, not on a single term.
    vocab.records.push(
      ...Array.from({ length: 5 }, (_, i) => ({ term: `${"x".repeat(191)}${i}`, addedBy: "a", addedAt: "t" })),
      ...Array.from({ length: 50 }, (_, i) => ({ term: `k${i}`, addedBy: "a", addedAt: "t" })),
    );
    const res = await relay()(vocabReq("GET", undefined));
    const { data, prompt_chars, keyterm_count } = (await res.json()) as {
      data: Record<string, any>[];
      prompt_chars: number;
      keyterm_count: number;
    };
    expect(prompt_chars).toBe(192 * 4 + 2 * 3); // x0..x3 pack, x4 breaks the head
    expect(keyterm_count).toBe(50);
    expect(data[3]).toMatchObject({ effective: { prompt: true, keyterm: true } });
    expect(data[4]).toMatchObject({ effective: { prompt: false, keyterm: true } });
    expect(data[54]).toMatchObject({ effective: { prompt: false, keyterm: false } }); // stored but inert
  });

  it("a runtime override wins the union's casing while presence stays config-pinned", async () => {
    vocab.records.push({ term: "KURULTAI", addedBy: "a", addedAt: "t" });
    const res = await relay({ vocabulary: ["Kurultai"] })(vocabReq("GET", undefined));
    const { data } = (await res.json()) as { data: Record<string, unknown>[] };
    expect(data).toEqual([
      {
        term: "KURULTAI",
        source: "config",
        pinned: true,
        runtimeOverride: true,
        effective: { prompt: true, keyterm: true },
        addedBy: "a",
        addedAt: "t",
      },
    ]);
  });

  it("a broken store answers 500 server_error, not an empty list", async () => {
    vocab.failWith = new VocabularyStoreError("unreadable", "corrupt");
    await expectOpenAIError(await relay()(vocabReq("GET", undefined)), 500, undefined);
  });
});

describe("POST /v1/vocabulary (R5)", () => {
  it("adds a term and the next transcription carries it — no restart (AE1)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const res = await post(["kurultai"]);
    expect(res.status).toBe(200);
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([{ term: "kurultai", outcome: "added", effective: { prompt: true, keyterm: true } }]);

    const handler = relay({ vocabulary: ["Omarchy"] });
    expect((await handler(transcribeReq())).status).toBe(200);
    expect(sentForm(fetchMock).get("prompt")).toBe("Omarchy, kurultai");
  });

  it("folded outcomes: exact dupe → already_present; case-variant of runtime → updated (respells)", async () => {
    await post(["kurultai"]);
    const dup = (await (await post(["kurultai"])).json()) as { data: { outcome: string }[] };
    expect(dup.data[0]!.outcome).toBe("already_present");
    const variant = (await (await post(["Kurultai"])).json()) as { data: { outcome: string }[] };
    expect(variant.data[0]!.outcome).toBe("updated");
    expect(vocab.records.map((r) => r.term)).toEqual(["Kurultai"]); // respelled, one record
  });

  it("config matching: same spelling → pinned (nothing stored); variant → config_shadowed (override)", async () => {
    const pinned = (await (await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["Kurultai"] }))).json()) as {
      data: { outcome: string }[];
    };
    expect(pinned.data[0]!.outcome).toBe("pinned");
    expect(vocab.records).toHaveLength(0);

    const shadowed = (await (await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["KURULTAI"] }))).json()) as {
      data: { outcome: string }[];
    };
    expect(shadowed.data[0]!.outcome).toBe("config_shadowed");
    expect(vocab.records.map((r) => r.term)).toEqual(["KURULTAI"]);
  });

  it("remove entries delete inside the same call — the proxy-safe fallback", async () => {
    await post(["kurultai"]);
    const res = await relay()(vocabReq("POST", { remove: ["KURULTAI"] }));
    expect(res.status).toBe(200);
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([{ term: "kurultai", outcome: "removed" }]);
    expect(vocab.records).toHaveLength(0);
  });

  it("dryRun reports outcomes and post-change capacity without persisting", async () => {
    const res = await post(["kurultai"], { dryRun: true });
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body).toMatchObject({
      dry_run: true,
      post_change_terms: 1,
      prompt_chars: "kurultai".length,
      keyterm_count: 1,
      data: [{ term: "kurultai", outcome: "added", effective: { prompt: true, keyterm: true } }],
    });
    expect(vocab.records).toHaveLength(0);
  });

  it("a batch past the runtime cap 409s vocabulary_full — nothing partially persists", async () => {
    vocab.records.push({ term: "only", addedBy: "a", addedAt: "t" });
    const many = Array.from({ length: MAX_TERMS }, (_, i) => `term-${i}`);
    const res = await post(many);
    await expectOpenAIError(res, 409, "vocabulary_full");
    expect(vocab.records.map((r) => r.term)).toEqual(["only"]);
  });

  it.each<[string, unknown]>([
    ["a JSON array root", ["kurultai"]],
    ["an empty terms array", { terms: [] }],
    ["a non-string item", { terms: ["ok", 7] }],
    ["a >200-char term", { terms: ["x".repeat(201)] }],
    ["a control char", { terms: ["bad\tterm"] }],
    ["a whitespace-only term", { terms: ["   "] }],
    ["a non-array terms", { terms: "kurultai" }],
    ["a non-boolean dryRun", { terms: ["x"], dryRun: "yes" }],
    ["an over-cap terms array", { terms: Array.from({ length: MAX_TERMS + 1 }, (_, i) => `t${i}`) }],
  ])("400 on %s", async (_name, body) => {
    await expectOpenAIError(await relay()(vocabReq("POST", body)), 400);
    expect(vocab.records).toHaveLength(0);
  });

  it("413s a body over VOCAB_MAX_BODY_BYTES in the OpenAI shape", async () => {
    const res = await relay()(vocabReq("POST", { terms: ["x"], pad: "y".repeat(VOCAB_MAX_BODY_BYTES) }));
    await expectOpenAIError(res, 413);
    expect(vocab.records).toHaveLength(0);
  });
});

describe("DELETE /v1/vocabulary (R6)", () => {
  it("folds the match, echoes the stored spelling, tombstones nothing in-memory", async () => {
    await post(["Kurultai"]);
    const res = await relay()(vocabReq("DELETE", { term: "kurultai" }));
    expect(res.status).toBe(200);
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([{ term: "Kurultai", outcome: "removed" }]);
    expect(vocab.records).toHaveLength(0);
  });

  it("a config term with a runtime override loses the override → config spelling returns", async () => {
    await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["KURULTAI"] }));
    const res = await relay({ vocabulary: ["Kurultai"] })(vocabReq("DELETE", { term: "kurultai" }));
    expect(res.status).toBe(200);
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([{ term: "KURULTAI", outcome: "removed", reverted_to: "Kurultai" }]);
    // And the config spelling is what the union reports again.
    const get = (await (await relay({ vocabulary: ["Kurultai"] })(vocabReq("GET", undefined))).json()) as {
      data: Record<string, unknown>[];
    };
    expect(get.data).toEqual([
      expect.objectContaining({ term: "Kurultai", source: "config", pinned: true }),
    ]);
  });

  it("a config term with no runtime record 409s term_pinned; an absent term 404s", async () => {
    const res = await relay({ vocabulary: ["Kurultai"] })(vocabReq("DELETE", { term: "Kurultai" }));
    await expectOpenAIError(res, 409, "term_pinned");
    await expectOpenAIError(await relay()(vocabReq("DELETE", { term: "nope" })), 404, "term_not_found");
  });

  it("a missing/empty/garbage term field 400s", async () => {
    for (const body of [{}, { term: "" }, { term: 3 }]) {
      await expectOpenAIError(await relay()(vocabReq("DELETE", body)), 400);
    }
  });
});

describe("live transcription wiring (R8, R10)", () => {
  it("a runtime override respells the config term in the provider prompt", async () => {
    on(GROQ_KEY_A, ok("hi"));
    await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["KURULTAI"] }));
    const handler = relay({ vocabulary: ["Kurultai"] });
    expect((await handler(transcribeReq())).status).toBe(200);
    expect(sentForm(fetchMock).get("prompt")).toBe("KURULTAI");
  });

  it("an unreadable store degrades transcription to config-only with a warn-once log", async () => {
    on(GROQ_KEY_A, ok("hi"), ok("hi again"));
    vocab.failWith = new VocabularyStoreError("unreadable", "corrupt");
    const handler = relay({ vocabulary: ["Omarchy"] });
    expect((await handler(transcribeReq())).status).toBe(200);
    expect(sentForm(fetchMock).get("prompt")).toBe("Omarchy");
    expect((await handler(transcribeReq())).status).toBe(200); // still serving
    const warnings = warnSpy.mock.calls.flat().join(" ");
    expect(warnings.match(/vocabulary store unreadable/g) ?? []).toHaveLength(1);
  });

  it("a missing store serves transcriptions untouched (the config set applies)", async () => {
    on(GROQ_KEY_A, ok("hi"));
    const handler = createRelay({ keys: {}, gateway: { tokenStore, providers: [groqA], vocabulary: ["Omarchy"] } });
    expect((await handler(transcribeReq())).status).toBe(200);
    expect(sentForm(fetchMock).get("prompt")).toBe("Omarchy");
  });
});

describe("POST remove matrix and batch accounting (review fixes)", () => {
  it("POST remove reports each outcome: removed / reverted_to / term_pinned / not_found", async () => {
    await relay({ vocabulary: ["Config"] })(vocabReq("POST", { terms: ["runtime", "CONFIG"] }));
    const res = await relay({ vocabulary: ["Config"] })(
      vocabReq("POST", { remove: ["runtime", "config", "missing"] }),
    );
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([
      { term: "runtime", outcome: "removed" },
      { term: "CONFIG", outcome: "removed", reverted_to: "Config" }, // the override went; config stays
      { term: "missing", outcome: "not_found" },
    ]);
    expect(vocab.records).toHaveLength(0);
    // A config term with no override is pinned, not deletable.
    const pinned = (await (await relay({ vocabulary: ["Config"] })(vocabReq("POST", { remove: ["Config"] }))).json()) as {
      data: Record<string, unknown>[];
    };
    expect(pinned.data).toEqual([{ term: "Config", outcome: "term_pinned" }]);
  });

  it("dryRun reports remove outcomes — stored spelling echoed, nothing persisted", async () => {
    await post(["Kurultai"]);
    const res = await relay()(vocabReq("POST", { remove: ["kurultai", "ghost"], dryRun: true }));
    const body = (await res.json()) as Record<string, unknown>;
    expect(body).toMatchObject({
      dry_run: true,
      post_change_terms: 0,
      data: [
        { term: "Kurultai", outcome: "removed" },
        { term: "ghost", outcome: "not_found" },
      ],
    });
    expect(vocab.records).toHaveLength(1); // untouched
  });

  it("post-change accounting reflects removes: counts and effective flags track the union", async () => {
    await post(["gone", "kept"]);
    const res = await relay()(vocabReq("POST", { remove: ["gone"] }));
    const body = (await res.json()) as Record<string, unknown>;
    expect(body).toMatchObject({ post_change_terms: 1, keyterm_count: 1 });
    expect(body.prompt_chars).toBe("kept".length); // "gone" left the union
  });

  it("a folded-duplicate remove resolves sequentially: the second is an honest not_found", async () => {
    await post(["dup"]);
    const res = await relay()(vocabReq("POST", { remove: ["dup", "DUP"] }));
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([
      { term: "dup", outcome: "removed" },
      { term: "DUP", outcome: "not_found" },
    ]);
  });

  it("add+remove in one request applies both; the removed term leaves the union", async () => {
    await post(["old"]);
    const res = await relay()(vocabReq("POST", { terms: ["fresh"], remove: ["old"] }));
    const body = (await res.json()) as { data: Record<string, unknown>[]; post_change_terms: number };
    expect(body.data).toEqual([
      { term: "fresh", outcome: "added", effective: { prompt: true, keyterm: true } },
      { term: "old", outcome: "removed" },
    ]);
    expect(body.post_change_terms).toBe(1);
    expect(vocab.records.map((r) => r.term)).toEqual(["fresh"]);
  });

  it("a rotation batch at the cap succeeds when removes free the slots adds need", async () => {
    vocab.records.push(
      ...Array.from({ length: MAX_TERMS }, (_, i) => ({ term: `old-${i}`, addedBy: "a", addedAt: "t" })),
    );
    const res = await relay()(vocabReq("POST", { terms: ["fresh"], remove: ["old-0"] }));
    expect(res.status).toBe(200); // 200 net terms: no false vocabulary_full
    expect(vocab.records).toHaveLength(MAX_TERMS);
  });

  it("folded-duplicate adds count once: post_change_terms sees the record, not the rows", async () => {
    const res = await post(["kurultai", "KURULTAI"]);
    const body = (await res.json()) as { data: { outcome: string }[]; post_change_terms: number };
    expect(body.data.map((d) => d.outcome)).toEqual(["added", "updated"]);
    expect(body.post_change_terms).toBe(1); // not 2 — the double-count bug
    expect(vocab.records).toHaveLength(1);
  });

  it("config-shadowed folded duplicates count once toward the cap too", async () => {
    const res = await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["KURULTAI", "kurultai"] }));
    const body = (await res.json()) as { data: { outcome: string }[]; post_change_terms: number };
    expect(body.data.map((d) => d.outcome)).toEqual(["config_shadowed", "config_shadowed"]);
    expect(body.post_change_terms).toBe(1);
    expect(vocab.records.map((r) => r.term)).toEqual(["kurultai"]); // last spelling won
  });

  it("dryRun reports capacity_exceeded instead of 409ing on an over-cap hypothetical", async () => {
    vocab.records.push(
      ...Array.from({ length: MAX_TERMS }, (_, i) => ({ term: `old-${i}`, addedBy: "a", addedAt: "t" })),
    );
    const res = await post(["one-more"], { dryRun: true });
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body).toMatchObject({ dry_run: true, capacity_exceeded: true, post_change_terms: MAX_TERMS + 1 });
    expect(vocab.records).toHaveLength(MAX_TERMS);
  });

  it("respelling an override back to the config spelling reverts — no zombie record", async () => {
    await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["KURULTAI"] }));
    const res = await relay({ vocabulary: ["Kurultai"] })(vocabReq("POST", { terms: ["Kurultai"] }));
    const body = (await res.json()) as { data: Record<string, unknown>[] };
    expect(body.data).toEqual([
      { term: "Kurultai", outcome: "reverted", reverted_to: "Kurultai", effective: { prompt: true, keyterm: true } },
    ]);
    expect(vocab.records).toHaveLength(0); // the override is gone, not invisible
    // And there is no zombie for DELETE to trip on — the config term is simply pinned.
    await expectOpenAIError(
      await relay({ vocabulary: ["Kurultai"] })(vocabReq("DELETE", { term: "kurultai" })),
      409,
      "term_pinned",
    );
  });
});

describe("request-shape and internal errors", () => {
  it("an unparseable JSON body 400s, not a 500", async () => {
    const res = await relay()(
      new Request(`${BASE}${GATEWAY_VOCABULARY_PATH}`, {
        method: "POST",
        headers: { "content-type": "application/json", authorization: `Bearer ${adminToken}` },
        body: "{not json",
      }),
    );
    await expectOpenAIError(res, 400);
  });

  it("an unexpected store failure answers the generic 500, not the raw error", async () => {
    vocab.failWith = new Error("disk exploded");
    const res = await relay()(vocabReq("GET", undefined));
    expect(res.status).toBe(500);
    const body = (await res.json()) as { error: { message: string; type: string } };
    expect(body.error.type).toBe("server_error");
    expect(body.error.message).not.toContain("disk exploded"); // internals stay server-side
    expect(errorSpy).toHaveBeenCalled();
  });

  it("the method check precedes auth: an unauthenticated PUT is 405, not 401", async () => {
    const res = await relay()(vocabReq("PUT", { terms: ["x"] }, false));
    expect(res.status).toBe(405);
  });
});

describe("GET reporting fields", () => {
  it("reports runtime_terms and max_terms alongside the budgets", async () => {
    vocab.records.push({ term: "wisp", addedBy: "a", addedAt: "t" });
    const res = await relay({ vocabulary: ["Omarchy"] })(vocabReq("GET", undefined));
    const body = (await res.json()) as Record<string, unknown>;
    expect(body).toMatchObject({ runtime_terms: 1, max_terms: MAX_TERMS });
  });
});
