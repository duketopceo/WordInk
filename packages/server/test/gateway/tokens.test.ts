import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
  it("gateway/tokens.ts has no node: imports (Workers-safe)", () => {
    const src = readFileSync(join(import.meta.dirname, "../../src/gateway/tokens.ts"), "utf8");
    expect(src).not.toMatch(/from\s+["']node:|require\(\s*["']node:|import\(\s*["']node:/);
  });
});
