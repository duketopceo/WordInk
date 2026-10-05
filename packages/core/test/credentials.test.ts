import { describe, expect, it, vi } from "vitest";
import { createDictation, isLocalHostname, mintToken, WordInkError } from "../src/index.js";
import { installFakeAudio } from "./fakes.js";

function onHost(hostname: string) {
  vi.stubGlobal("location", { hostname, href: `https://${hostname}/` });
}

function catchError(fn: () => unknown): WordInkError {
  try {
    fn();
  } catch (err) {
    if (err instanceof WordInkError) return err;
    throw err;
  }
  throw new Error("expected a WordInkError");
}

describe("devKey (KTD3: localhost only)", () => {
  it("throws a Config error on a non-localhost hostname before any network call", () => {
    onHost("app.example.com");
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const WebSocketMock = vi.fn();
    vi.stubGlobal("WebSocket", WebSocketMock);

    for (const provider of ["groq", "openai", "deepgram"] as const) {
      const err = catchError(() => createDictation({ provider, devKey: "sk-live-secret" }));
      expect(err.code).toBe("Config");
      expect(err.message).toContain("localhost");
      expect(err.message).not.toContain("sk-live-secret");
      expect(err.hint).toMatch(/@wordink\/server/);
    }
    expect(fetchMock).not.toHaveBeenCalled();
    expect(WebSocketMock).not.toHaveBeenCalled();
  });

  it("throws when there is no location at all (not a page)", () => {
    vi.stubGlobal("location", undefined);
    expect(catchError(() => createDictation({ provider: "groq", devKey: "k" })).code).toBe("Config");
  });

  it("is allowed on localhost, 127.0.0.1 and [::1], with a console warning", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    for (const host of ["localhost", "127.0.0.1", "[::1]"]) {
      onHost(host);
      createDictation({ provider: "groq", devKey: "gsk_dev" }).destroy();
    }
    expect(warn).toHaveBeenCalledTimes(3);
    expect(String(warn.mock.calls[0]![0])).toMatch(/visible to anyone/);
    expect(isLocalHostname("localhost.example.com")).toBe(false);
    expect(isLocalHostname("127.0.0.2")).toBe(false);
  });

  it("sends the dev key straight to Groq on localhost", async () => {
    onHost("localhost");
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const audio = installFakeAudio();
    const fetchMock = vi.fn(async (_u: string | URL | Request, _i?: RequestInit) => new Response("hi"));
    vi.stubGlobal("fetch", fetchMock);
    const d = createDictation({ provider: "groq", devKey: "gsk_dev" });
    await d.start();
    await vi.waitFor(() => expect(d.state).toBe("listening"));
    audio.speak(0.5);
    await d.stop();
    await vi.waitFor(() => expect(d.state).toBe("idle"));
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("https://api.groq.com/openai/v1/audio/transcriptions");
    expect(init?.headers).toEqual([["Authorization", "Bearer gsk_dev"]]);
    expect(init?.credentials).toBe("omit");
    d.destroy();
  });

  it("requires exactly one of endpoint and devKey for cloud providers", () => {
    onHost("localhost");
    expect(catchError(() => createDictation({ provider: "groq" })).code).toBe("Config");
    expect(catchError(() => createDictation({ provider: "groq", endpoint: "/api", devKey: "k" })).code).toBe("Config");
  });
});

describe("mintToken (relay routes from @wordink/server)", () => {
  it("returns the OpenAI client secret from { value, expires_at }", async () => {
    const fetchMock = vi.fn(async () => Response.json({ value: "ek_abc", expires_at: 1_900_000_000 }));
    await expect(mintToken("openai", "https://r.test/api", fetchMock)).resolves.toBe("ek_abc");
    expect(fetchMock).toHaveBeenCalledWith("https://r.test/api/openai/token", expect.objectContaining({ method: "POST" }));
  });

  it("returns the Deepgram JWT from { access_token, expires_in }", async () => {
    const fetchMock = vi.fn(async () => Response.json({ access_token: "jwt.x.y", expires_in: 120 }));
    await expect(mintToken("deepgram", "https://r.test/api", fetchMock)).resolves.toBe("jwt.x.y");
    expect(fetchMock).toHaveBeenCalledWith("https://r.test/api/deepgram/token", expect.anything());
  });

  it.each([
    [403, "AuthFailed"],
    [401, "AuthFailed"],
    [429, "RateLimited"],
    [502, "ProviderDown"],
    [503, "ProviderDown"],
  ])("maps relay HTTP %i to %s", async (status, code) => {
    const fetchMock = vi.fn(async () => Response.json({ error: "x" }, { status }));
    await expect(mintToken("openai", "https://r.test", fetchMock)).rejects.toMatchObject({ code });
  });

  it("maps a network failure or a malformed body to ProviderDown", async () => {
    const offline = vi.fn(async () => {
      throw new TypeError("Failed to fetch");
    });
    await expect(mintToken("deepgram", "https://r.test", offline)).rejects.toMatchObject({ code: "ProviderDown" });
    const malformed = vi.fn(async () => Response.json({ token: "wrong-shape" }));
    await expect(mintToken("deepgram", "https://r.test", malformed)).rejects.toMatchObject({ code: "ProviderDown" });
  });
});
