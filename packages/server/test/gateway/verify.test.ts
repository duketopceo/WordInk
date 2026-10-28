import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

// verify is exercised as a spawned process against a temp HOME/XDG; beforeAll rebuilds
// dist so the tests always run the current bin. The operator's real token store lives at
// the default state path of the env we pass — verify must never touch it.
const PKG = join(import.meta.dirname, "..", "..");
const BIN = join(PKG, "dist", "bin", "wordink-gateway.js");
const SECRET = "gsk_live_SECRET_NEVER_PRINTED_0123456789";

let home: string;
let stateDir: string;
const children: ChildProcess[] = [];

beforeAll(() => {
  // dist is built once by vitest globalSetup — never rebuild here (parallel
  // test files racing tsc emit caused children to read half-written output).
  expect(existsSync(BIN)).toBe(true);
});

beforeEach(() => {
  home = mkdtempSync(join(tmpdir(), "wdk-verify-test-"));
  stateDir = join(home, "state", "wordink");
});

afterEach(async () => {
  for (const child of children.splice(0)) {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      await new Promise((r) => child.once("close", r));
    }
  }
  rmSync(home, { recursive: true, force: true });
});

function baseEnv(extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  return {
    PATH: process.env.PATH ?? "/usr/bin",
    HOME: home,
    XDG_CONFIG_HOME: join(home, "config"),
    XDG_STATE_HOME: join(home, "state"),
    ...extra,
  };
}

interface RunResult {
  code: number | null;
  stdout: string;
  stderr: string;
}

function run(args: string[], env: Record<string, string> = {}): Promise<RunResult> {
  return new Promise((resolve, reject) => {
    const child = spawn("node", [BIN, "verify", ...args], { env: baseEnv(env) });
    children.push(child);
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d: Buffer) => (stdout += d.toString()));
    child.stderr.on("data", (d: Buffer) => (stderr += d.toString()));
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, stdout, stderr }));
  });
}

describe("verify", () => {
  it("proves the full mechanics offline with no provider keys", async () => {
    const r = await run([]);
    expect(r.code).toBe(0);
    for (const name of [
      "config resolves",
      "check",
      "serve binds",
      "tokens minted",
      "GET /v1/models",
      "POST /v1/audio/transcriptions",
      "vocabulary scope gate",
      "vocabulary write+read",
      "token revoke",
    ]) {
      const escaped = name.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
      expect(r.stdout, name).toMatch(new RegExp(`PASS\\s+${escaped}`));
    }
  }, 60_000);

  it("never touches the operator's real token store", async () => {
    mkdirSync(stateDir, { recursive: true });
    const storePath = join(stateDir, "gateway-tokens.json");
    const seeded = JSON.stringify({ version: 1, tokens: [{ id: "keepme", hash: "x" }] });
    writeFileSync(storePath, seeded);

    const r = await run([]);
    expect(r.code).toBe(0);
    expect(readFileSync(storePath, "utf8")).toBe(seeded);
  }, 60_000);

  it("prints no token or key material in its output", async () => {
    const r = await run(["--live"], { UNUSED_LIVE_KEY: SECRET });
    expect(r.stdout + r.stderr).not.toMatch(/wdk_[A-Za-z0-9_-]{43}/);
    expect(r.stdout + r.stderr).not.toContain(SECRET);
  }, 60_000);

  it("--live without a config fails fast naming the path", async () => {
    const r = await run(["--live"]);
    expect(r.code).toBe(1);
    expect(r.stderr + r.stdout).toContain("gateway.json");
  }, 60_000);

  it("--live skips providers when the config resolves no keys", async () => {
    mkdirSync(join(home, "config", "wordink"), { recursive: true });
    writeFileSync(
      join(home, "config", "wordink", "gateway.json"),
      JSON.stringify({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_UNSET" }] }),
    );
    const r = await run(["--live"]);
    expect(r.code).toBe(0);
    expect(r.stdout).toMatch(/SKIP\s+live providers/);
  }, 60_000);

  it("--live exercises a real entry through its own isolated serve", async () => {
    const fake = createServer((req, res) => {
      req.resume();
      req.on("end", () => {
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify({ text: "Hello, world" }));
      });
    });
    await new Promise<void>((r) => fake.listen(0, "127.0.0.1", r));
    const fakePort = (fake.address() as AddressInfo).port;
    mkdirSync(join(home, "config", "wordink"), { recursive: true });
    writeFileSync(
      join(home, "config", "wordink", "gateway.json"),
      JSON.stringify({
        providers: [
          { provider: "groq", keyEnv: "WORDINK_LIVE_FAKE", url: `http://127.0.0.1:${fakePort}` },
          { provider: "openai", keyEnv: "WORDINK_LIVE_UNSET" },
        ],
      }),
    );
    try {
      const r = await run(["--live"], { WORDINK_LIVE_FAKE: SECRET });
      expect(r.code).toBe(0);
      expect(r.stdout).toMatch(/PASS\s+live groq/);
      expect(r.stdout).toContain('"Hello, world"');
      expect(r.stdout + r.stderr).not.toContain(SECRET);
    } finally {
      await new Promise((r) => fake.close(() => r(null)));
    }
  }, 60_000);

  it("--live reports a real provider's failure without masking it", async () => {
    const fake = createServer((_req, res) => {
      res.statusCode = 500;
      res.end("{}");
    });
    await new Promise<void>((r) => fake.listen(0, "127.0.0.1", r));
    mkdirSync(join(home, "config", "wordink"), { recursive: true });
    writeFileSync(
      join(home, "config", "wordink", "gateway.json"),
      JSON.stringify({
        providers: [
          // The single entry's upstream 500s: verify must report FAIL, not SKIP or pass.
          { provider: "openai", keyEnv: "WORDINK_OK", url: `http://127.0.0.1:${(fake.address() as AddressInfo).port}` },
        ],
      }),
    );
    try {
      const r = await run(["--live"], { WORDINK_OK: "sk-test" });
      expect(r.code).toBe(1);
      expect(r.stdout).toMatch(/FAIL\s+live openai/);
    } finally {
      await new Promise((r) => fake.close(() => r(null)));
    }
  }, 60_000);

  it("passes while another gateway holds the default port", async () => {
    const squat = createServer((_req, res) => res.end("x"));
    // A real gateway may already hold 8941 (dev boxes run it daily) — either way the
    // precondition "something is bound to the default port" holds.
    await new Promise<void>((r) => {
      squat.once("error", () => r());
      squat.listen(8941, "127.0.0.1", () => r());
    });
    try {
      const r = await run([]);
      expect(r.code).toBe(0);
    } finally {
      await new Promise((r) => squat.close(() => r(null)));
    }
  }, 60_000);

  it("rejects unknown arguments", async () => {
    const r = await run(["--bogus"]);
    expect(r.code).toBe(1);
    expect(r.stderr).toMatch(/unexpected arguments/i);
  });
});
