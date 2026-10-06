import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { hashToken } from "../../src/gateway/tokens.js";

// The CLI is exercised as a spawned process against a temp HOME/XDG, never by importing
// internals. beforeAll rebuilds dist so the tests always run the current bin.
const PKG = join(import.meta.dirname, "..", "..");
const BIN = join(PKG, "dist", "bin", "wordink-gateway.js");

const KEY_A = "gsk_test_cli_KEY_A_0123456789";

let home: string;
let configPath: string;
const children: ChildProcess[] = [];

beforeAll(() => {
  execFileSync("pnpm", ["build"], { cwd: PKG, stdio: "pipe" });
  expect(existsSync(BIN)).toBe(true);
}, 60_000);

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), "wdk-cli-"));
  configPath = join(home, "config", "wordink", "gateway.json");
});

afterEach(async () => {
  for (const child of children.splice(0)) {
    child.kill("SIGKILL");
    await new Promise((r) => child.once("close", r));
  }
  rmSync(home, { recursive: true, force: true });
});

/** A clean environment: no ambient provider keys can leak into the CLI under test. */
function baseEnv(extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  return {
    PATH: process.env.PATH ?? "/usr/bin",
    HOME: home,
    XDG_CONFIG_HOME: join(home, "config"),
    XDG_STATE_HOME: join(home, "state"),
    ...extra,
  };
}

function writeConfig(config: unknown): void {
  mkdirSync(join(home, "config", "wordink"), { recursive: true });
  writeFileSync(configPath, JSON.stringify(config));
}

interface RunResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

function run(args: string[], env: Record<string, string> = {}): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const child = spawn("node", [BIN, ...args], { env: baseEnv(env) });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d: Buffer) => (stdout += d.toString()));
    child.stderr.on("data", (d: Buffer) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

/** Spawn `serve` and resolve once its stdout announces the listener. */
function serve(env: Record<string, string>): Promise<ChildProcess> {
  const child = spawn("node", [BIN, "serve"], { env: baseEnv(env) });
  children.push(child);
  return new Promise((resolve, reject) => {
    let out = "";
    const timer = setTimeout(() => reject(new Error(`serve produced no listener line; stderr so far: ${err}`)), 15_000);
    let err = "";
    child.stderr.on("data", (d: Buffer) => (err += d.toString()));
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
      if (/listening on/i.test(out)) {
        clearTimeout(timer);
        resolve(child);
      }
    });
    child.on("exit", () => {
      clearTimeout(timer);
      reject(new Error(`serve exited before listening; stderr: ${err}`));
    });
  });
}

async function freePort(): Promise<number> {
  const s = createServer();
  await new Promise<void>((r) => s.listen(0, "127.0.0.1", r));
  const port = (s.address() as AddressInfo).port;
  await new Promise<void>((r) => s.close(() => r()));
  return port;
}

describe("check", () => {
  it("reports present/missing per entry and never prints key material", async () => {
    writeConfig({
      providers: [
        { provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" },
        { provider: "openai", keyEnv: "WORDINK_TEST_KEY_B" },
      ],
    });
    const r = await run(["check"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(r.code).toBe(0);
    expect(r.stdout).toMatch(/groq[^\n]*\bpresent\b/i);
    expect(r.stdout).toMatch(/openai[^\n]*\bmissing\b/i);
    expect(r.stdout + r.stderr).not.toContain(KEY_A);
  });

  it("exits non-zero with a clear message when no entry resolves a key", async () => {
    writeConfig({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_UNSET" }] });
    const r = await run(["check"]);
    expect(r.code).not.toBe(0);
    expect(r.stderr + r.stdout).toMatch(/key/i);
  });

  it("exits non-zero when the config file is missing", async () => {
    const r = await run(["check"]);
    expect(r.code).not.toBe(0);
    expect(r.stderr + r.stdout).toContain("gateway.json");
  });
});

describe("config validation", () => {
  it.each<[string, unknown, RegExp]>([
    ["a JSON array root", [], /JSON object/i],
    ["providers not an array", { providers: "x" }, /providers.*array/i],
    ["an unknown provider", { providers: [{ provider: "azure", keyEnv: "K" }] }, /providers\[0\]\.provider/i],
    ["an inherited-property provider name", { providers: [{ provider: "constructor", keyEnv: "K" }] }, /providers\[0\]\.provider/i],
    ["a keyEnv that is not an env name", { providers: [{ provider: "groq", keyEnv: "NO SPACE" }] }, /keyEnv/i],
    ["a non-http(s) url", { providers: [{ provider: "groq", keyEnv: "K", url: "ftp://x" }] }, /url/i],
    ["a remote http url", { providers: [{ provider: "groq", keyEnv: "K", url: "http://example.com/x" }] }, /https.*loopback|loopback.*https/i],
    ["a port out of range", { providers: [], port: 70000 }, /port/i],
    ["a non-string vocabulary term", { providers: [], vocabulary: ["ok", 1] }, /vocabulary/i],
    ["a non-positive rateLimit", { providers: [], rateLimit: { max: -1 } }, /positive number/i],
    ["a non-boolean allowInsecureRemote", { providers: [], allowInsecureRemote: "yes" }, /true or false/i],
  ])("check rejects %s", async (_name, config, pattern) => {
    writeConfig(config);
    const r = await run(["check"]);
    expect(r.code).toBe(1);
    expect(r.stderr).toMatch(pattern);
  });

  it("rejects a non-JSON config body", async () => {
    mkdirSync(dirname(configPath), { recursive: true });
    writeFileSync(configPath, "{not json");
    const r = await run(["check"]);
    expect(r.code).toBe(1);
    expect(r.stderr).toMatch(/not valid JSON/i);
  });

  it("rejects extra arguments", async () => {
    for (const args of [["check", "typo"], ["serve", "--port", "9999"]]) {
      const r = await run(args);
      expect(r.code).toBe(1);
      expect(r.stderr).toMatch(/takes no arguments/i);
    }
  });
});

describe("tokens", () => {
  it("create prints the token once, list shows id and label only, revoke marks it revoked", async () => {
    const created = await run(["tokens", "create", "laptop"]);
    expect(created.code).toBe(0);
    const token = created.stdout.match(/wdk_[A-Za-z0-9_-]{43}/)?.[0];
    expect(token).toBeTruthy();
    // R10: printed exactly once, never stored or reprinted.
    expect(created.stdout.split(token!).length - 1).toBe(1);

    const listed = await run(["tokens", "list"]);
    expect(listed.code).toBe(0);
    expect(listed.stdout).toContain("laptop");
    const id = listed.stdout.match(/[0-9a-f]{8}-[0-9a-f-]{27}/)?.[0];
    expect(id).toBeTruthy();
    // Neither the token nor its hash ever appears in list output.
    expect(listed.stdout).not.toContain(token!);
    expect(listed.stdout).not.toContain(await hashToken(token!));

    const revoked = await run(["tokens", "revoke", id!]);
    expect(revoked.code).toBe(0);
    const after = await run(["tokens", "list"]);
    expect(after.stdout).toMatch(/revoked/);

    const again = await run(["tokens", "revoke", id!]);
    expect(again.code).not.toBe(0);
  });

  it("revoke of an unknown id exits non-zero", async () => {
    const r = await run(["tokens", "revoke", "no-such-id"]);
    expect(r.code).not.toBe(0);
    expect(r.stderr + r.stdout).toContain("no-such-id");
  });
});

describe("serve", () => {
  it("refuses to start when no provider entry resolves a key", async () => {
    writeConfig({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_UNSET" }] });
    const r = await run(["serve"]);
    expect(r.code).not.toBe(0);
    expect(r.stderr + r.stdout).toMatch(/no provider.*key/i);
  });

  it("refuses a non-loopback host without allowInsecureRemote", async () => {
    writeConfig({
      host: "0.0.0.0",
      providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }],
    });
    const r = await run(["serve"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(r.code).not.toBe(0);
    expect(r.stderr + r.stdout).toContain("allowInsecureRemote");
  });

  it("answers a real HTTP request through a fake-provider-backed config (loopback)", async () => {
    const fake = createServer((req, res) => {
      req.resume();
      req.on("end", () => {
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({ text: "from the fake provider" }));
      });
    });
    await new Promise<void>((r) => fake.listen(0, "127.0.0.1", r));
    const fakePort = (fake.address() as AddressInfo).port;

    const port = await freePort();
    writeConfig({
      port,
      providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A", url: `http://127.0.0.1:${fakePort}` }],
    });
    const made = await run(["tokens", "create", "voxtype"]);
    const token = made.stdout.match(/wdk_[A-Za-z0-9_-]{43}/)![0];

    await serve({ WORDINK_TEST_KEY_A: KEY_A });
    try {
      const form = new FormData();
      form.append("file", new Blob([new Uint8Array([1, 2, 3])], { type: "audio/wav" }), "audio.wav");
      form.append("model", "whisper-1");
      const res = await fetch(`http://127.0.0.1:${port}/v1/audio/transcriptions`, {
        method: "POST",
        headers: { authorization: `Bearer ${token}` },
        body: form,
      });
      expect(res.status).toBe(200);
      expect(await res.json()).toEqual({ text: "from the fake provider" });
    } finally {
      await new Promise((r) => fake.close(() => r(null)));
    }
  }, 30_000);
});
