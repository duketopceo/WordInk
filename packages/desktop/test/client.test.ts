import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  ApiError,
  NotRunningError,
  UnauthorizedError,
  UnsupportedVersionError,
  connect,
  discoveryPath,
} from "../src/index.js";

const TOKEN = "test-token";
const OWNER = "owner-token";

interface Seen {
  method: string;
  url: string;
  headers: IncomingMessage["headers"];
  body: Buffer;
}

let dir: string;
let server: Server;
let seen: Seen[];
let file: string;

function reply(res: ServerResponse, status: number, body?: unknown) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(body === undefined ? undefined : JSON.stringify(body));
}

async function start(apiVersion = "2", port?: number) {
  server = createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => {
      seen.push({ method: req.method ?? "", url: req.url ?? "", headers: req.headers, body: Buffer.concat(chunks) });
      if (req.headers.authorization !== `Bearer ${TOKEN}`) return reply(res, 401);
      const url = req.url ?? "";
      if (url === "/health") return reply(res, 200, { version: "test", apiVersion });
      if (url === "/capabilities") {
        return reply(res, 200, { audioFormats: ["audio/wav"], partialTranscripts: false, serviceCapture: true, streams: true, developerControl: false, control: true });
      }
      if (url === "/models") return reply(res, 200, [{ id: "m1", installed: false }, { id: "m2", installed: true }]);
      if (url.startsWith("/transcriptions")) return reply(res, 200, { transcript: "hello world", durationMs: 900 });
      if (url === "/dictations") return reply(res, 201, { id: 7, ownerToken: OWNER, sampleRate: 16000 });
      if (url.startsWith("/dictations/7/")) {
        if (req.headers["x-hex-dictation-token"] !== OWNER) return reply(res, 404, { code: "dictation-not-found" });
        if (url.endsWith("/finish")) return reply(res, 200, { transcript: "dictated", durationMs: 1200 });
        res.writeHead(204);
        return res.end();
      }
      if (url === "/control") return reply(res, 200, { version: 4, status: "done", message: "ok", changed: false });
      return reply(res, 404, { code: "not_found" });
    });
  });
  await new Promise<void>((resolve) => server.listen(port ?? 0, "127.0.0.1", resolve));
  const actual = (server.address() as AddressInfo).port;
  await writeFile(file, JSON.stringify({ port: actual, token: TOKEN, apiVersion, pid: process.pid }));
  return actual;
}

beforeEach(async () => {
  seen = [];
  dir = await mkdtemp(join(tmpdir(), "wordink-desktop-"));
  file = join(dir, "local-api.json");
});

afterEach(async () => {
  await new Promise<void>((resolve) => (server ? server.close(() => resolve()) : resolve()));
  await rm(dir, { recursive: true, force: true });
});

describe("connect", () => {
  it("reads the discovery file and reports health and capabilities", async () => {
    await start();
    const client = await connect({ discoveryPath: file });
    expect(client.apiVersion).toBe("2");
    expect(await client.health()).toEqual({ version: "test", apiVersion: "2" });
    expect((await client.capabilities()).serviceCapture).toBe(true);
    expect(seen[0]?.headers.authorization).toBe(`Bearer ${TOKEN}`);
  });

  it("throws NotRunningError when there is no discovery file", async () => {
    await expect(connect({ discoveryPath: file })).rejects.toBeInstanceOf(NotRunningError);
  });

  it("throws NotRunningError for a stale file whose port is closed", async () => {
    const port = await start();
    await new Promise<void>((resolve) => server.close(() => resolve()));
    await writeFile(file, JSON.stringify({ port, token: TOKEN, apiVersion: "2", pid: 1 }));
    await expect(connect({ discoveryPath: file })).rejects.toBeInstanceOf(NotRunningError);
  });

  it("refuses an unknown apiVersion without calling the app", async () => {
    await start("99");
    const error = await connect({ discoveryPath: file }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(UnsupportedVersionError);
    expect((error as UnsupportedVersionError).apiVersion).toBe("99");
    expect(seen).toHaveLength(0);
  });

  it("throws UnauthorizedError for a wrong token", async () => {
    const port = await start();
    await writeFile(file, JSON.stringify({ port, token: "wrong", apiVersion: "2", pid: 1 }));
    await expect(connect({ discoveryPath: file })).rejects.toBeInstanceOf(UnauthorizedError);
  });

  it("honors the WORDINK_LOCAL_API_FILE override", async () => {
    await start();
    const client = await connect({ env: { WORDINK_LOCAL_API_FILE: file } });
    expect((await client.health()).version).toBe("test");
  });
});

describe("requests", () => {
  it("transcribes with the first installed model when none is given", async () => {
    await start();
    const client = await connect({ discoveryPath: file });
    const text = await client.transcribe(new Uint8Array([1, 2, 3]), { language: "en" });
    expect(text).toBe("hello world");
    const post = seen.find((s) => s.url.startsWith("/transcriptions"));
    expect(post?.url).toBe("/transcriptions?model=m2&language=en");
    expect(post?.headers["content-type"]).toBe("audio/wav");
    expect([...(post?.body ?? [])]).toEqual([1, 2, 3]);
  });

  it("runs a dictation through finish with the owner token", async () => {
    await start();
    const client = await connect({ discoveryPath: file });
    const dictation = await client.dictate({ source: "test" });
    expect(dictation.id).toBe(7);
    await dictation.heartbeat();
    expect(await dictation.finish()).toEqual({ text: "dictated", durationMs: 1200 });
    expect(seen.at(-1)?.headers["x-hex-dictation-token"]).toBe(OWNER);
    expect(JSON.parse(seen.find((s) => s.url === "/dictations")?.body.toString() ?? "{}")).toEqual({ source: "test" });
  });

  it("cancels a dictation", async () => {
    await start();
    const client = await connect({ discoveryPath: file });
    const dictation = await client.dictate();
    await dictation.cancel();
    expect(seen.at(-1)?.url).toBe("/dictations/7/cancel");
  });

  it("sends control commands flat with the control version", async () => {
    await start();
    const client = await connect({ discoveryPath: file });
    await client.control("dictation", { action: "toggle" });
    const post = seen.find((s) => s.url === "/control");
    expect(JSON.parse(post?.body.toString() ?? "{}")).toEqual({ version: 4, action: "toggle", command: "dictation" });
  });

  it("surfaces the app's error code as ApiError", async () => {
    await start();
    const client = await connect({ discoveryPath: file });
    server.removeAllListeners("request");
    server.on("request", (_req, res) => reply(res, 409, { code: "model-not-ready" }));
    const caught = await client.transcribe(new Uint8Array(1), { model: "m1" }).catch((e: unknown) => e);
    expect(caught).toBeInstanceOf(ApiError);
    expect((caught as ApiError).code).toBe("model-not-ready");
  });
});

describe("discoveryPath", () => {
  it("picks the per-OS data folder", () => {
    const env = {};
    expect(discoveryPath({ platform: "linux", home: "/h", env })).toBe(join("/h", ".local", "share", "wordink", "local-api.json"));
    expect(discoveryPath({ platform: "linux", home: "/h", env: { XDG_DATA_HOME: "/x" } })).toBe(join("/x", "wordink", "local-api.json"));
    expect(discoveryPath({ platform: "darwin", home: "/h", env })).toBe(join("/h", "Library", "Application Support", "wordink", "local-api.json"));
    expect(discoveryPath({ platform: "win32", home: "/h", env: { APPDATA: "/a" } })).toBe(join("/a", "wordink", "local-api.json"));
  });
});
