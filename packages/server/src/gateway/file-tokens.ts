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
 * A missing file is an empty store; an unreadable one is treated as empty and logged.
 */
export class FileTokenStore implements TokenStore {
  private cache: { key: string; tokens: StoredToken[] } | undefined;

  constructor(private readonly path: string = defaultTokenPath()) {}

  private async load(): Promise<StoredToken[]> {
    let key: string;
    try {
      const s = await stat(this.path);
      key = `${s.ino}:${s.mtimeMs}:${s.size}`;
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === "ENOENT") return [];
      console.warn(`wordink gateway: cannot stat token file ${this.path}`);
      return [];
    }
    if (this.cache?.key === key) return this.cache.tokens;
    let tokens: StoredToken[] = [];
    try {
      const parsed = JSON.parse(await readFile(this.path, "utf8")) as Partial<TokenFile>;
      if (!Array.isArray(parsed.tokens)) throw new Error("no tokens array");
      tokens = parsed.tokens;
    } catch {
      console.warn(`wordink gateway: token file ${this.path} is unreadable; treating it as empty`);
    }
    this.cache = { key, tokens };
    return tokens;
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

  async create(label: string): Promise<TokenInfo & { token: string }> {
    const token = generateToken();
    const entry: StoredToken = {
      id: randomUUID(),
      label,
      hash: await hashToken(token),
      createdAt: new Date().toISOString(),
    };
    await this.save([...(await this.load()), entry]);
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
    const tokens = await this.load();
    const target = tokens.find((t) => t.id === id && !t.revokedAt);
    if (!target) return false;
    await this.save(
      tokens.map((t) => (t === target ? { ...t, revokedAt: new Date().toISOString() } : t)),
    );
    return true;
  }
}
