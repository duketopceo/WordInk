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

  it("create --admin mints an admin token; the flag never lands inside the label", async () => {
    const device = await run(["tokens", "create", "laptop"]);
    expect(device.stdout).toContain("scope=device");
    const admin = await run(["tokens", "create", "--admin", "agent"]);
    expect(admin.code).toBe(0);
    expect(admin.stdout).toContain("scope=admin");
    const listed = await run(["tokens", "list"]);
    expect(listed.stdout).toMatch(/device\s+laptop/);
    expect(listed.stdout).toMatch(/admin\s+agent/);

    const bare = await run(["tokens", "create", "--admin"]);
    expect(bare.code).toBe(1);
    const spaced = await run(["tokens", "create", "--admin", "my", "laptop"]);
    expect(spaced.code).toBe(0);
    expect(spaced.stdout).toContain('label="my laptop"');
    expect(spaced.stdout).not.toContain("--admin");
  });

  it("revoke of an unknown id exits non-zero", async () => {
    const r = await run(["tokens", "revoke", "no-such-id"]);
    expect(r.code).not.toBe(0);
    expect(r.stderr + r.stdout).toContain("no-such-id");
  });
});

describe("pair", () => {
  it("mints a device token and prints a wordink-remote:// link; list never re-shows it", async () => {
    const r = await run(["pair", "--url", "https://max.ts.net", "--label", "my iphone", "--no-qr"]);
    expect(r.code).toBe(0);
    const link = r.stdout.match(/wordink-remote:\/\/pair\?url=[^&\s]+&token=(wdk_[A-Za-z0-9_-]{43})/);
    expect(link).toBeTruthy();
    expect(decodeURIComponent(link![0])).toContain("url=https://max.ts.net");
    expect(r.stdout).toContain('label="my iphone"');

    const listed = await run(["tokens", "list"]);
    expect(listed.stdout).toMatch(/device\s+my iphone/);
    expect(listed.stdout).not.toContain(link![1]);
    expect(listed.stdout).not.toContain(await hashToken(link![1]));
  });

  it("renders a QR block by default and skips it with --no-qr", async () => {
    const qr = await run(["pair", "--url", "https://max.ts.net"]);
    expect(qr.code).toBe(0);
    expect(qr.stdout).toContain("█");
    const bare = await run(["pair", "--url", "https://max.ts.net", "--no-qr"]);
    expect(bare.stdout).not.toContain("█");
  });

  it("defaults --url to the config's publicUrl", async () => {
    writeConfig({
      providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }],
      publicUrl: "https://gw.example.net",
    });
    const r = await run(["pair", "--no-qr"]);
    expect(r.code).toBe(0);
    expect(r.stdout).toContain("url=https%3A%2F%2Fgw.example.net");
  });

  it("fails with no --url and no config publicUrl", async () => {
    writeConfig({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }] });
    const r = await run(["pair", "--no-qr"]);
    expect(r.code).toBe(1);
    expect(r.stderr + r.stdout).toMatch(/--url|publicUrl/);
  });

  it("surfaces a malformed config instead of treating it as absent", async () => {
    writeConfig({
      providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }],
      publicUrl: "https://gw.example.net/path?q=1",
    });
    const r = await run(["pair", "--no-qr"]);
    expect(r.code).toBe(1);
    expect(r.stderr + r.stdout).toContain("publicUrl");
  });

  it("rejects cleartext to non-LAN hosts and loopback endpoints; LAN http pairs", async () => {
    const pub = await run(["pair", "--url", "http://203.0.113.5:8941", "--no-qr"]);
    expect(pub.code).toBe(1);
    expect(pub.stderr + pub.stdout).toMatch(/cleartext|LAN|Tailscale/i);

    const lan = await run(["pair", "--url", "http://192.168.7.9:8941", "--no-qr"]);
    expect(lan.code).toBe(0);
    expect(lan.stdout).toContain("url=http%3A%2F%2F192.168.7.9%3A8941");

    const loop = await run(["pair", "--url", "http://127.0.0.1:8941", "--no-qr"]);
    expect(loop.code).toBe(1);
    expect(loop.stderr + loop.stdout).toMatch(/not reachable/);
  });

  it("rejects http to IPv6 literals — link-local, ULA, and global are never LAN", async () => {
    for (const v6 of ["http://[fe80::1]:8941", "http://[fd00::1]:8941", "http://[2606:4700::1]:8941"]) {
      const r = await run(["pair", "--url", v6, "--no-qr"]);
      expect(r.code).toBe(1);
      expect(r.stderr + r.stdout).toMatch(/cleartext|LAN|Tailscale/i);
    }
    // Bare [::1] is also unreachable from a phone.
    const v6loop = await run(["pair", "--url", "http://[::1]:8941", "--no-qr"]);
    expect(v6loop.code).toBe(1);
  });

  it("rejects a --url carrying credentials, query, or path", async () => {
    for (const bad of ["https://u:p@x.net", "https://x.net/?q=1", "https://x.net/v1"]) {
      const r = await run(["pair", "--url", bad, "--no-qr"]);
      expect(r.code).toBe(1);
    }
  });

  it("rejects a flag-shaped or missing flag value instead of consuming the next flag", async () => {
    for (const args of [
      ["pair", "--url"],
      ["pair", "--url", "--no-qr"],
      ["pair", "--label"],
      ["pair", "--label", "--no-qr", "--url", "https://max.ts.net"],
    ]) {
      const r = await run(args);
      expect(r.code).toBe(1);
      expect(r.stderr + r.stdout).toContain("usage");
    }
    const listed = await run(["tokens", "list"]);
    expect(listed.stdout).not.toContain("--no-qr");
  });
});

describe("vocabulary visibility (R11)", () => {
  const vocabDir = () => join(home, "state", "wordink");

  it("check warns when no live admin token exists; minting one silences the warning", async () => {
    writeConfig({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }] });
    const r = await run(["check"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(r.stderr).toMatch(/no live admin token/);

    await run(["tokens", "create", "--admin", "agent"]);
    const again = await run(["check"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(again.stderr).not.toMatch(/no live admin token/);
  });

  it("serve warns too — the gateway would boot with every /v1/vocabulary call 403ing", async () => {
    writeConfig({
      port: await freePort(),
      providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }],
    });
    const child = spawn("node", [BIN, "serve"], { env: baseEnv({ WORDINK_TEST_KEY_A: KEY_A }) });
    children.push(child);
    let err = "";
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`no admin warning within 10s; stderr: ${err}`)), 10_000);
      child.stderr.on("data", (d: Buffer) => {
        err += d.toString();
        if (/no live admin token/.test(err)) {
          clearTimeout(timer);
          resolve();
        }
      });
    });
  }, 15_000);

  it("check reports config count and the runtime store state", async () => {
    writeConfig({
      providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }],
      vocabulary: ["Omarchy", "Kurultai"],
    });
    mkdirSync(vocabDir(), { recursive: true });
    writeFileSync(
      join(vocabDir(), "vocabulary.json"),
      JSON.stringify({ version: 1, terms: [{ term: "wisp", addedBy: "a", addedAt: "t" }] }),
    );
    const r = await run(["check"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(r.stdout).toContain("vocabulary: 2 config terms");
    expect(r.stdout).toMatch(/vocabulary: \+1 runtime terms/);
  });

  it("check says when the runtime vocabulary file is unreadable instead of hiding it", async () => {
    writeConfig({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }] });
    mkdirSync(vocabDir(), { recursive: true });
    writeFileSync(join(vocabDir(), "vocabulary.json"), "{corrupt");
    const r = await run(["check"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(r.code).toBe(0);
    expect(r.stdout).toMatch(/vocabulary: .*unreadable — transcriptions will use config terms only/);
  });

  it("an unreadable token store still warns instead of crashing check", async () => {
    writeConfig({ providers: [{ provider: "groq", keyEnv: "WORDINK_TEST_KEY_A" }] });
    mkdirSync(join(home, "state", "wordink"), { recursive: true });
    writeFileSync(join(home, "state", "wordink", "gateway-tokens.json"), "{corrupt");
    const r = await run(["check"], { WORDINK_TEST_KEY_A: KEY_A });
    expect(r.code).toBe(0);
    expect(r.stderr).toMatch(/cannot read the token store/);
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
