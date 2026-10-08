import { afterEach, describe, expect, it, vi } from "vitest";
import { createDictation } from "../src/index.js";
import { installFakeAudio } from "./fakes.js";

const HTTP_TIMEOUT_MS = 30_000; // host.ts HTTP_TIMEOUT_MS

/** A Groq relay that never answers; records each request's abort signal. */
function hungUpload(): AbortSignal[] {
  const signals: AbortSignal[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_url: string | URL | Request, init?: RequestInit) =>
        new Promise<Response>((_, reject) => {
          const signal = init!.signal!;
          signals.push(signal);
          signal.addEventListener("abort", () => reject(new DOMException("aborted", "AbortError")));
        }),
    ),
  );
  return signals;
}

describe("batch upload (Groq) request lifetime", () => {
  afterEach(() => vi.useRealTimers());

  it("a hung upload is aborted after the HTTP timeout and ends in ProviderDown", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const audio = installFakeAudio();
    const signals = hungUpload();
    const d = createDictation({ provider: "groq", endpoint: "https://app.test/relay" });
    const errors: string[] = [];
    d.on("error", (e) => errors.push(e.code));
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.6);
    await d.stop();
    await vi.waitFor(() => expect(signals).toHaveLength(1));
    expect(d.state).toBe("transcribing");

    await vi.advanceTimersByTimeAsync(HTTP_TIMEOUT_MS);
    expect(signals[0]!.aborted).toBe(true);
    await vi.waitFor(() => expect(d.state).toBe("error"));
    expect(errors).toEqual(["ProviderDown"]);
    d.destroy();
  });

  it("destroy() aborts a pending upload", async () => {
    const audio = installFakeAudio();
    const signals = hungUpload();
    const d = createDictation({ provider: "groq", endpoint: "https://app.test/relay" });
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.6);
    await d.stop();
    await vi.waitFor(() => expect(signals).toHaveLength(1));
    d.destroy();
    expect(signals[0]!.aborted).toBe(true);
  });
});
