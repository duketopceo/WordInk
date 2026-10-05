// A failed wasm load must not be cached forever: the next press retries it.
import { describe, expect, it, vi } from "vitest";
import init from "../wasm/wordink_core.js";
import { createDictation, type WordInkError } from "../src/index.js";
import { installFakeAudio } from "./fakes.js";

vi.mock("../wasm/wordink_core.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../wasm/wordink_core.js")>();
  return { ...original, default: vi.fn(original.default) };
});

describe("core wasm load", () => {
  it("a failed load surfaces as an error on press, and the next press retries the load", async () => {
    // Fails for the eager load at construction and for the retry on the first press.
    const offline = new TypeError("Failed to fetch wordink_core_bg.wasm");
    vi.mocked(init).mockRejectedValueOnce(offline).mockRejectedValueOnce(offline);
    installFakeAudio();
    vi.stubGlobal("fetch", vi.fn(async () => new Response("unused")));
    const d = createDictation({ provider: "groq", endpoint: "https://app.test/api/wordink" });
    const errors: WordInkError[] = [];
    d.on("error", (e) => errors.push(e));

    await expect(d.press()).resolves.toBeUndefined();
    expect(d.state).toBe("error");
    expect(errors).toHaveLength(1);
    expect(errors[0]!.code).toBe("ProviderDown");
    expect(errors[0]!.hint).toMatch(/wasm/i);

    await d.press();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    expect(init).toHaveBeenCalledTimes(3);
    await expect(d.ready).resolves.toBeUndefined();
    d.destroy();
  });
});
