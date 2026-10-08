import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { chmodSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, sep } from "node:path";
import { generateToken, hashToken } from "../../src/gateway/tokens.js";
import { FileTokenStore, defaultTokenPath } from "../../src/gateway/file-tokens.js";

let dir: string;
let path: string;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "wdk-tokens-"));
  path = join(dir, "state", "gateway-tokens.json");
});
afterEach(() => {
  vi.restoreAllMocks();
  rmSync(dir, { recursive: true, force: true });
});

describe("generateToken / hashToken", () => {
  it("makes wdk_ + 32 random bytes base64url, unique each call", () => {
    const a = generateToken();
    expect(a).toMatch(/^wdk_[A-Za-z0-9_-]{43}$/);
    expect(generateToken()).not.toBe(a);
  });
  it("hashes to SHA-256 hex", async () => {
    expect(await hashToken("abc")).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  });
});

describe("FileTokenStore", () => {
  it("create returns the token once and persists only its hash, mode 0600 in a 0700 dir", async () => {
    const store = new FileTokenStore(path);
    const made = await store.create("laptop");
    expect(made.token).toMatch(/^wdk_/);
    expect(made.label).toBe("laptop");
    const raw = readFileSync(path, "utf8");
    expect(raw).not.toContain(made.token);
    expect(raw).toContain(await hashToken(made.token));
    expect(statSync(path).mode & 0o777).toBe(0o600);
    expect(statSync(join(dir, "state")).mode & 0o777).toBe(0o700);
  });

  it("verify accepts a good token and rejects unknown and revoked ones", async () => {
    const store = new FileTokenStore(path);
    const a = await store.create("a");
    const b = await store.create("b");
    expect(await store.verify(a.token)).toEqual({ id: a.id, label: "a" });
    expect(await store.verify(generateToken())).toBeNull();
    expect(await store.verify("")).toBeNull();
    expect(await store.revoke(a.id)).toBe(true);
    expect(await store.verify(a.token)).toBeNull();
    expect(await store.verify(b.token)).toEqual({ id: b.id, label: "b" });
  });

  it("revoke returns false for an unknown id", async () => {
    expect(await new FileTokenStore(path).revoke("nope")).toBe(false);
  });

  it("sees a revoke made by another instance on the next verify", async () => {
    const serving = new FileTokenStore(path);
    const { id, token } = await serving.create("phone");
    expect(await serving.verify(token)).not.toBeNull(); // warms the cache
    const cli = new FileTokenStore(path);
    expect(await cli.revoke(id)).toBe(true);
    expect(await serving.verify(token)).toBeNull();
  });

  it("list exposes id, label, createdAt, revokedAt only", async () => {
    const store = new FileTokenStore(path);
    const a = await store.create("a");
    await store.create("b");
    await store.revoke(a.id);
    const rows = await store.list();
    expect(rows).toHaveLength(2);
    for (const r of rows) {
      expect(Object.keys(r).every((k) => ["id", "label", "createdAt", "revokedAt"].includes(k))).toBe(true);
    }
    expect(rows.find((r) => r.id === a.id)?.revokedAt).toBeTruthy();
    expect(JSON.stringify(rows)).not.toContain(a.token);
    expect(JSON.stringify(rows)).not.toContain(await hashToken(a.token));
  });

  it("treats a missing file as an empty store", async () => {
    const store = new FileTokenStore(path);
    expect(await store.list()).toEqual([]);
    expect(await store.verify(generateToken())).toBeNull();
  });

  it("corrupt JSON verifies as null and warns", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { token } = await new FileTokenStore(path).create("x");
    writeFileSync(path, "{not json");
    expect(await new FileTokenStore(path).verify(token)).toBeNull();
    expect(warn).toHaveBeenCalled();
  });

  it("refuses to create or revoke over a corrupt file, leaving it untouched", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, "{not json");
    const store = new FileTokenStore(path);
    await expect(store.create("y")).rejects.toThrow(/unreadable/);
    await expect(store.revoke("some-id")).rejects.toThrow(/unreadable/);
    expect(readFileSync(path, "utf8")).toBe("{not json");
  });

  it("a valid-JSON wrong-shape file is unreadable too", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, '{"tokens":{}}');
    const store = new FileTokenStore(path);
    expect(await store.verify(generateToken())).toBeNull();
    await expect(store.create("y")).rejects.toThrow(/unreadable/);
  });

  it("deleting the file after a warm cache verifies nothing (ENOENT bypasses the cache)", async () => {
    const store = new FileTokenStore(path);
    const { token } = await store.create("x");
    expect(await store.verify(token)).not.toBeNull();
    rmSync(path);
    expect(await store.verify(token)).toBeNull();
    expect(await store.list()).toEqual([]);
  });

  it("recovers from a transient read failure instead of latching unreadable", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const store = new FileTokenStore(path);
    const { token } = await store.create("x"); // the write clears the cache
    // stat still works (the directory is searchable) but the read itself fails.
    chmodSync(path, 0o000);
    expect(await store.verify(token)).toBeNull();
    chmodSync(path, 0o600);
    // Same inode/mtime/size, so the file key is unchanged — recovery must not
    // depend on the file being rewritten.
    expect(await store.verify(token)).not.toBeNull();
  });
});

describe("defaultTokenPath", () => {
  it("uses XDG_STATE_HOME, else ~/.local/state", () => {
    expect(defaultTokenPath({ XDG_STATE_HOME: "/x/state" }, "/home/u")).toBe(
      "/x/state/wordink/gateway-tokens.json",
    );
    expect(defaultTokenPath({}, "/home/u")).toBe("/home/u/.local/state/wordink/gateway-tokens.json");
  });
});

describe("import graph", () => {
  // The Cloudflare entry's reachable set: every src/ module except the declared
  // Node-only ones. A node: import in any of these breaks the Workers bundle (KTD3).
  const SRC = join(import.meta.dirname, "../../src");
  const NODE_ONLY = new Set(["node.ts", join("gateway", "file-tokens.ts")]);
  const SKIP_DIR = `bin${sep}`;
  const NODE_IMPORT = /from\s+["']node:|require\(\s*["']node:|import\(\s*["']node:/;

  function workersSources(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? workersSources(join(dir, e.name)) : e.name.endsWith(".ts") ? [join(dir, e.name)] : [],
    );
  }

  it("no Workers-reachable source imports node:*", () => {
    const files = workersSources(SRC).filter((f) => {
      const rel = relative(SRC, f);
      return !rel.startsWith(SKIP_DIR) && !NODE_ONLY.has(rel);
    });
    expect(files.length).toBeGreaterThan(5); // guard against a silently-empty scan
    for (const file of files) {
      expect(readFileSync(file, "utf8"), relative(SRC, file)).not.toMatch(NODE_IMPORT);
    }
  });
});
