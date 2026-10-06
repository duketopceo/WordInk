// Node-only: imported by node.ts and the gateway bin, never by the Workers entry.
import { randomUUID } from "node:crypto";
import { mkdir, readFile, rename, stat, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import {
  constantTimeEqualHex,
  generateToken,
  hashToken,
  type TokenInfo,
  type TokenRecord,
  type TokenStore,
} from "./tokens.js";

interface StoredToken extends TokenRecord {
  hash: string;
}

interface TokenFile {
  version: 1;
  tokens: StoredToken[];
}

/** `$XDG_STATE_HOME/wordink/gateway-tokens.json`, else `~/.local/state/wordink/gateway-tokens.json`. */
export function defaultTokenPath(
  env: Record<string, string | undefined> = process.env,
  home: string = homedir(),
): string {
  const base = env.XDG_STATE_HOME || join(home, ".local", "state");
  return join(base, "wordink", "gateway-tokens.json");
}

/**
 * Tokens in a JSON file (hashes only), written atomically with mode 0600. The parsed file is cached
 * by (inode, mtime, size), so a revoke made by another process (the CLI) is seen on the next call.
 * A missing file is an empty store. An unreadable one verifies nothing (logged), and writes refuse
 * to replace it, so a corrupt file never silently loses the tokens it held.
 */
export class FileTokenStore implements TokenStore {
  private cache: { key: string; tokens: StoredToken[] } | undefined;
  /** The file key the last read was taken from, for the write race re-check. */
  private loadedKey: string | null = null;
  /** Warn-once latches: a persistent failure must not log once per request. */
  private statWarned = false;
  private unreadableWarnedFor: string | null = null;

  constructor(private readonly path: string = defaultTokenPath()) {}

  private async load(): Promise<StoredToken[]> {
    return (await this.read()).tokens;
  }

  /** Tokens for a write; throws when the file exists but cannot be parsed. */
  private async loadForWrite(): Promise<StoredToken[]> {
    const { tokens, unreadable } = await this.read();
    if (unreadable) throw new Error(`wordink gateway: token file ${this.path} is unreadable; fix or remove it first`);
    return tokens;
  }

  private async read(): Promise<{ tokens: StoredToken[]; unreadable: boolean }> {
    let key: string;
    try {
      const s = await stat(this.path);
      this.statWarned = false;
      key = `${s.ino}:${s.mtimeMs}:${s.size}`;
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") {
        this.loadedKey = null;
        return { tokens: [], unreadable: false };
      }
      if (!this.statWarned) {
        this.statWarned = true;
        console.warn(`wordink gateway: cannot stat token file ${this.path}`);
      }
      return { tokens: [], unreadable: true };
    }
    this.loadedKey = key;
    if (this.cache?.key === key) return { tokens: this.cache.tokens, unreadable: false };
    try {
      const parsed = JSON.parse(await readFile(this.path, "utf8")) as Partial<TokenFile>;
      if (!Array.isArray(parsed.tokens)) throw new Error("no tokens array");
      const tokens = parsed.tokens;
      this.cache = { key, tokens };
      return { tokens, unreadable: false };
    } catch {
      // A failed read is not cached: the next call retries, so a transient error (a
      // transient EACCES, a chmod away and back) recovers instead of latching dead.
      if (this.unreadableWarnedFor !== key) {
        this.unreadableWarnedFor = key;
        console.warn(`wordink gateway: token file ${this.path} is unreadable; no token will verify`);
      }
      return { tokens: [], unreadable: true };
    }
  }

  /** The file's current (ino, mtime, size) key; null when absent, a never-matching marker on error. */
  private async statKey(): Promise<string | null> {
    try {
      const s = await stat(this.path);
      return `${s.ino}:${s.mtimeMs}:${s.size}`;
    } catch (err) {
      // A stat error is not proof the file is missing: "changed" retries (or fails closed
      // on the re-read) rather than renaming blindly over a file that may exist.
      return (err as NodeJS.ErrnoException).code === "ENOENT" ? null : "unstatable";
    }
  }

  private async save(tokens: StoredToken[]): Promise<void> {
    await mkdir(dirname(this.path), { recursive: true, mode: 0o700 });
    const tmp = `${this.path}.${process.pid}.${randomUUID()}.tmp`;
    const body: TokenFile = { version: 1, tokens };
    await writeFile(tmp, JSON.stringify(body, null, 2) + "\n", { mode: 0o600 });
    await rename(tmp, this.path);
    this.cache = undefined;
  }

  async verify(token: string): Promise<TokenInfo | null> {
    if (!token) return null;
    const hash = await hashToken(token);
    let found: StoredToken | undefined;
    for (const t of await this.load()) {
      // No early exit: every entry is compared.
      if (constantTimeEqualHex(t.hash, hash)) found = t;
    }
    if (!found || found.revokedAt) return null;
    return { id: found.id, label: found.label };
  }

  /**
   * Read-modify-write, retried once when the file changed under us. The writers are CLI
   * invocations, so races are rare and a lockfile is not worth its stale-lock failure mode —
   * this re-check only keeps a racing create/revoke from silently dropping the other write.
   */
  private async mutate(update: (tokens: StoredToken[]) => StoredToken[] | null): Promise<boolean> {
    for (let tries = 0; ; tries++) {
      const loaded = await this.loadForWrite();
      const keyAtLoad = this.loadedKey;
      const next = update(loaded);
      if (next === null) return false;
      if ((await this.statKey()) === keyAtLoad) {
        await this.save(next);
        return true;
      }
      if (tries === 1) {
        throw new Error(`wordink gateway: token file ${this.path} changed while writing; try again`);
      }
    }
  }

  async create(label: string): Promise<TokenInfo & { token: string }> {
    const token = generateToken();
    const entry: StoredToken = {
      id: randomUUID(),
      label,
      hash: await hashToken(token),
      createdAt: new Date().toISOString(),
    };
    await this.mutate((tokens) => [...tokens, entry]);
    return { id: entry.id, label, token };
  }

  async list(): Promise<TokenRecord[]> {
    return (await this.load()).map(({ id, label, createdAt, revokedAt }) => ({
      id,
      label,
      createdAt,
      ...(revokedAt ? { revokedAt } : {}),
    }));
  }

  async revoke(id: string): Promise<boolean> {
    return await this.mutate((tokens) => {
      const target = tokens.find((t) => t.id === id && !t.revokedAt);
      if (!target) return null;
      return tokens.map((t) => (t === target ? { ...t, revokedAt: new Date().toISOString() } : t));
    });
  }
}
