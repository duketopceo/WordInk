// Shared gateway.json loading and validation for the wordink-gateway bin commands —
// `serve`/`check`/`tokens` use it against the operator's real config; `verify` reuses it
// for the sandbox config it writes and for read-only `--config` live-mode parsing.
import { readFile } from "node:fs/promises";
import { isIP } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";
import { isRecord, type RateLimitOptions } from "../internal.js";
import { DEFAULT_MODELS, type ProviderEntry, type ProviderName, type ResolvedEntry } from "../gateway/providers.js";

/** `$XDG_CONFIG_HOME/wordink/gateway.json`, else `~/.config/wordink/gateway.json`. */
export function defaultConfigPath(env: NodeJS.ProcessEnv = process.env, home = homedir()): string {
  return join(env.XDG_CONFIG_HOME || join(home, ".config"), "wordink", "gateway.json");
}

/** The operator's `gateway.json`. `providers` names env vars; key values never appear in it. */
export interface FileConfig {
  host?: string;
  port?: number;
  /** Required to bind a non-loopback host: tokens and audio cross the network in cleartext. */
  allowInsecureRemote?: boolean;
  /** The endpoint devices reach this gateway at (e.g. the tailnet name); `pair` uses it. */
  publicUrl?: string;
  providers: ProviderEntry[];
  vocabulary?: string[];
  rateLimit?: RateLimitOptions;
  maxBodyBytes?: number;
  upstreamTimeoutMs?: number;
  deadlineMs?: number;
}

/** Config errors are user-facing: reported on stderr, exit 1. */
export class UsageError extends Error {}

/** "Set one of: GROQ_API_KEY, OPENAI_API_KEY" — the env names a config wants populated. */
export function keyEnvHint(providers: readonly ProviderEntry[]): string {
  return providers.map((e) => e.keyEnv).join(", ") || "(none configured)";
}

export function optionalNumber(config: Record<string, unknown>, name: string): number | undefined {
  const value = config[name];
  if (value === undefined) return undefined;
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
    throw new UsageError(`"${name}" must be a positive number`);
  }
  return value;
}

const KEY_ENV_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

function parseEntry(value: unknown, index: number): ProviderEntry {
  const where = `providers[${index}]`;
  if (!isRecord(value)) throw new UsageError(`${where} must be an object like {"provider":"groq","keyEnv":"GROQ_API_KEY"}`);
  const provider = value.provider;
  // `hasOwn`, not `in`: "constructor"/"toString" would pass an `in` check via the prototype
  // chain and then resolve to a function as the provider's default model.
  if (typeof provider !== "string" || !Object.hasOwn(DEFAULT_MODELS, provider)) {
    throw new UsageError(`${where}.provider must be one of: ${Object.keys(DEFAULT_MODELS).join(", ")}`);
  }
  const keyEnv = value.keyEnv;
  if (typeof keyEnv !== "string" || !KEY_ENV_RE.test(keyEnv)) {
    throw new UsageError(`${where}.keyEnv must name an environment variable`);
  }
  const entry: ProviderEntry = { provider: provider as ProviderName, keyEnv };
  if (value.model !== undefined) {
    if (typeof value.model !== "string" || value.model === "") {
      throw new UsageError(`${where}.model must be a non-empty string`);
    }
    entry.model = value.model;
  }
  if (value.url !== undefined) {
    if (typeof value.url !== "string") throw new UsageError(`${where}.url must be an http(s) URL`);
    let url: URL;
    try {
      url = new URL(value.url);
    } catch {
      throw new UsageError(`${where}.url must be an http(s) URL`);
    }
    // The resolved provider key is sent to this URL: https anywhere, cleartext only to
    // loopback (the local fakes the test suite and a dev's endpoint need).
    const host = urlHostname(url);
    if (url.protocol !== "https:" && !(url.protocol === "http:" && isLoopback(host))) {
      throw new UsageError(
        `${where}.url must be an https URL, or http to a loopback host — the provider key is sent to it`,
      );
    }
    entry.url = value.url;
  }
  return entry;
}

export async function loadConfig(path: string): Promise<FileConfig> {
  let raw: string;
  try {
    raw = await readFile(path, "utf8");
  } catch {
    throw new UsageError(`cannot read ${path} — create it (see examples/gateway-systemd/gateway.example.json)`);
  }
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new UsageError(`${path} is not valid JSON`);
  }
  if (!isRecord(value)) throw new UsageError(`${path} must contain a JSON object`);

  const config: FileConfig = { providers: [] };
  if (value.host !== undefined) {
    if (typeof value.host !== "string" || value.host === "") {
      throw new UsageError(`"host" must be a non-empty string`);
    }
    config.host = value.host;
  }
  const port = optionalNumber(value, "port");
  if (port !== undefined) {
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      throw new UsageError(`"port" must be an integer between 1 and 65535`);
    }
    config.port = port;
  }
  if (value.allowInsecureRemote !== undefined) {
    if (typeof value.allowInsecureRemote !== "boolean") {
      throw new UsageError(`"allowInsecureRemote" must be true or false`);
    }
    config.allowInsecureRemote = value.allowInsecureRemote;
  }
  if (value.publicUrl !== undefined) {
    if (typeof value.publicUrl !== "string") {
      throw new UsageError(`"publicUrl" must be a string`);
    }
    config.publicUrl = normalizePublicUrl(value.publicUrl);
  }
  if (!Array.isArray(value.providers)) {
    throw new UsageError(`"providers" must be an array of {"provider","keyEnv"} entries`);
  }
  config.providers = value.providers.map(parseEntry);
  if (value.vocabulary !== undefined) {
    if (!Array.isArray(value.vocabulary) || value.vocabulary.some((t) => typeof t !== "string")) {
      throw new UsageError(`"vocabulary" must be an array of strings`);
    }
    config.vocabulary = value.vocabulary as string[];
  }
  if (value.rateLimit !== undefined) {
    if (!isRecord(value.rateLimit)) throw new UsageError(`"rateLimit" must be an object`);
    const rateLimit: RateLimitOptions = {};
    const windowMs = optionalNumber(value.rateLimit, "windowMs");
    const max = optionalNumber(value.rateLimit, "max");
    const dailyMax = optionalNumber(value.rateLimit, "dailyMax");
    if (windowMs !== undefined) rateLimit.windowMs = windowMs;
    if (max !== undefined) rateLimit.max = max;
    if (dailyMax !== undefined) rateLimit.dailyMax = dailyMax;
    config.rateLimit = rateLimit;
  }
  const maxBodyBytes = optionalNumber(value, "maxBodyBytes");
  const upstreamTimeoutMs = optionalNumber(value, "upstreamTimeoutMs");
  const deadlineMs = optionalNumber(value, "deadlineMs");
  if (maxBodyBytes !== undefined) config.maxBodyBytes = maxBodyBytes;
  if (upstreamTimeoutMs !== undefined) config.upstreamTimeoutMs = upstreamTimeoutMs;
  if (deadlineMs !== undefined) config.deadlineMs = deadlineMs;
  return config;
}

/** WHATWG hostname keeps [] on IPv6 literals; policy checks want the bare host. */
export function urlHostname(url: URL): string {
  return url.hostname.startsWith("[") ? url.hostname.slice(1, -1) : url.hostname;
}

/**
 * The endpoint a remote device (phone, laptop) uses to reach the gateway —
 * origin plus port only, matching the contract the iOS side enforces.
 */
export function normalizePublicUrl(input: string, name = "publicUrl"): string {
  const s = input.trim();
  if (s === "") throw new UsageError(`"${name}" must be a non-empty string`);
  let url: URL;
  try {
    // Bare hosts default to https (the tailnet/TLS case); LAN operators write http://.
    url = new URL(s.includes("://") ? s : `https://${s}`);
  } catch {
    throw new UsageError(`"${name}" must be an http(s) URL`);
  }
  if (url.username !== "" || url.password !== "") {
    throw new UsageError(`"${name}" carries no credentials — tokens go in the token field`);
  }
  if (url.search !== "" || url.hash !== "") {
    throw new UsageError(`"${name}" must not carry a query or fragment`);
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new UsageError(`"${name}" must be an http(s) URL`);
  }
  if (url.pathname !== "" && url.pathname !== "/") {
    throw new UsageError(`"${name}" is origin plus port — no path`);
  }
  return url.origin;
}

export function isLoopback(host: string): boolean {
  // `localhost` is the one name operators expect; anything else must be an IP literal —
  // a DNS name beginning "127." can resolve off-loopback and must not skip the remote gate.
  if (host === "localhost" || host === "::1") return true;
  return isIP(host) === 4 && host.startsWith("127.");
}

/** Entries with a usable key, in order; the rest are reported so the operator can fix them. */
export function resolveKeys(
  entries: readonly ProviderEntry[],
  env: NodeJS.ProcessEnv,
): { resolved: ResolvedEntry[]; skipped: ProviderEntry[] } {
  const resolved: ResolvedEntry[] = [];
  const skipped: ProviderEntry[] = [];
  for (const entry of entries) {
    // `typeof` guards inherited members too: env["__proto__"] is truthy on some hosts and
    // would otherwise be reported "present" and sent upstream as the Authorization value.
    const key = env[entry.keyEnv];
    if (typeof key === "string" && key.length > 0) resolved.push({ ...entry, key });
    else skipped.push(entry);
  }
  return { resolved, skipped };
}
