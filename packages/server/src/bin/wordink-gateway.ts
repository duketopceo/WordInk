#!/usr/bin/env node
// wordink-gateway: run and administer the WordInk desktop gateway (KTD8, R9, R13).
// The config file names the environment variables holding provider keys — never key
// material itself; the systemd wrapper (examples/gateway-systemd) fills them from omaseal.
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { isIP } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";
import { createNodeHandler, FileTokenStore, defaultTokenPath } from "../node.js";
import { isRecord, type RateLimitOptions } from "../internal.js";
import { DEFAULT_MODELS, type ProviderEntry, type ProviderName, type ResolvedEntry } from "../gateway/providers.js";

const PREFIX = "wordink-gateway:";
const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 8941;
const KEY_ENV_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;

const USAGE = `Usage: wordink-gateway <command>

  serve                  Run the gateway (config: $XDG_CONFIG_HOME/wordink/gateway.json)
  check                  Resolve the config and report which provider keys are present
  tokens create <label>  Mint a device token; the token is printed exactly once
  tokens list            List device tokens: id, label, created, revoked (never the token)
  tokens revoke <id>     Revoke a device token; it is refused on its next request
`;

/** `$XDG_CONFIG_HOME/wordink/gateway.json`, else `~/.config/wordink/gateway.json`. */
function defaultConfigPath(env: NodeJS.ProcessEnv = process.env, home = homedir()): string {
  return join(env.XDG_CONFIG_HOME || join(home, ".config"), "wordink", "gateway.json");
}

/** The operator's `gateway.json`. `providers` names env vars; key values never appear in it. */
interface FileConfig {
  host?: string;
  port?: number;
  /** Required to bind a non-loopback host: tokens and audio cross the network in cleartext. */
  allowInsecureRemote?: boolean;
  providers: ProviderEntry[];
  vocabulary?: string[];
  rateLimit?: RateLimitOptions;
  maxBodyBytes?: number;
  upstreamTimeoutMs?: number;
  deadlineMs?: number;
}

/** Config errors are user-facing: reported on stderr, exit 1. */
class UsageError extends Error {}

/** "Set one of: GROQ_API_KEY, OPENAI_API_KEY" — the env names a config wants populated. */
function keyEnvHint(providers: readonly ProviderEntry[]): string {
  return providers.map((e) => e.keyEnv).join(", ") || "(none configured)";
}

function optionalNumber(config: Record<string, unknown>, name: string): number | undefined {
  const value = config[name];
  if (value === undefined) return undefined;
  if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) {
    throw new UsageError(`"${name}" must be a positive number`);
  }
  return value;
}

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
    const host = url.hostname.startsWith("[") ? url.hostname.slice(1, -1) : url.hostname;
    if (url.protocol !== "https:" && !(url.protocol === "http:" && isLoopback(host))) {
      throw new UsageError(
        `${where}.url must be an https URL, or http to a loopback host — the provider key is sent to it`,
      );
    }
    entry.url = value.url;
  }
  return entry;
}

async function loadConfig(path: string): Promise<FileConfig> {
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

function isLoopback(host: string): boolean {
  // `localhost` is the one name operators expect; anything else must be an IP literal —
  // a DNS name beginning "127." can resolve off-loopback and must not skip the remote gate.
  if (host === "localhost" || host === "::1") return true;
  return isIP(host) === 4 && host.startsWith("127.");
}

/** Entries with a usable key, in order; the rest are reported so the operator can fix them. */
function resolveKeys(
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

async function cmdCheck(): Promise<number> {
  const path = defaultConfigPath();
  const config = await loadConfig(path);
  const host = config.host ?? DEFAULT_HOST;
  const port = config.port ?? DEFAULT_PORT;
  const { resolved, skipped } = resolveKeys(config.providers, process.env);

  console.log(`config: ${path}`);
  console.log(`listen: ${host}:${port}`);
  if (!isLoopback(host)) {
    console.log(
      config.allowInsecureRemote === true
        ? `warning: ${host} is not loopback — tokens and audio cross the network in cleartext`
        : `warning: ${host} is not loopback — serve will refuse unless "allowInsecureRemote": true`,
    );
  }
  console.log(`tokens: ${defaultTokenPath()}`);
  if (config.vocabulary !== undefined) console.log(`vocabulary: ${config.vocabulary.length} terms`);
  console.log("providers, in fallback order:");
  config.providers.forEach((entry, i) => {
    const key = process.env[entry.keyEnv];
    console.log(
      `  ${i + 1}. ${entry.provider}  model=${entry.model ?? DEFAULT_MODELS[entry.provider]}  ` +
        `keyEnv=${entry.keyEnv}  key ${typeof key === "string" && key.length > 0 ? "present" : "missing"}`,
    );
  });

  if (resolved.length === 0) {
    console.error(
      `${PREFIX} no provider entry resolves a key, so serve would refuse to start. ` +
        `Set one of: ${keyEnvHint(config.providers)}.`,
    );
    return 1;
  }
  if (skipped.length > 0) {
    console.log(`${skipped.length} ${skipped.length === 1 ? "entry is" : "entries are"} skipped at serve time (no key).`);
  }
  return 0;
}

async function cmdServe(): Promise<number> {
  const path = defaultConfigPath();
  const config = await loadConfig(path);
  const { resolved, skipped } = resolveKeys(config.providers, process.env);
  for (const entry of skipped) {
    console.warn(`${PREFIX} skipping ${entry.provider} entry: $${entry.keyEnv} is not set`);
  }
  if (resolved.length === 0) {
    console.error(
      `${PREFIX} no provider entry resolved a key; refusing to start. ` +
        `Set one of: ${keyEnvHint(config.providers)}.`,
    );
    return 1;
  }

  const host = config.host ?? DEFAULT_HOST;
  const port = config.port ?? DEFAULT_PORT;
  if (!isLoopback(host)) {
    if (config.allowInsecureRemote !== true) {
      console.error(
        `${PREFIX} refusing to bind ${host}: device tokens and audio would cross the network in ` +
          `cleartext. Keep the default 127.0.0.1, or set "allowInsecureRemote": true in ${path} ` +
          `only when TLS terminates in front.`,
      );
      return 1;
    }
    console.warn(
      `${PREFIX} warning: binding ${host} — tokens and audio cross the network in cleartext ` +
        `unless TLS terminates in front.`,
    );
  }

  const handler = createNodeHandler({
    keys: {},
    gateway: {
      tokenStore: new FileTokenStore(),
      providers: resolved,
      vocabulary: config.vocabulary,
      rateLimit: config.rateLimit,
      maxBodyBytes: config.maxBodyBytes,
      upstreamTimeoutMs: config.upstreamTimeoutMs,
      deadlineMs: config.deadlineMs,
    },
    // trustProxy stays off: there is no proxy contract for a local service (KTD8).
  });
  const server = createServer(handler);
  return await new Promise<number>((resolve) => {
    server.on("error", (err) => {
      console.error(`${PREFIX} cannot listen on ${host}:${port} (${err.message})`);
      resolve(1);
    });
    server.listen(port, host, () => {
      console.log(
        `${PREFIX} listening on http://${host}:${port} ` +
          `(${resolved.length} provider ${resolved.length === 1 ? "entry" : "entries"}, ` +
          `tokens at ${defaultTokenPath()})`,
      );
    });
  });
}

async function cmdTokens(args: string[]): Promise<number> {
  const store = new FileTokenStore();
  const [sub, ...rest] = args;
  switch (sub) {
    case "create": {
      const label = rest.join(" ").trim();
      if (!label) {
        console.error(`${PREFIX} usage: wordink-gateway tokens create <label>`);
        return 1;
      }
      const made = await store.create(label);
      // The full token is printed exactly once (R10); only its hash is stored.
      console.log(made.token);
      console.log(`created id=${made.id} label="${made.label}" — store the token now; it is never shown again.`);
      return 0;
    }
    case "list": {
      const rows = await store.list();
      if (rows.length === 0) console.log("no device tokens");
      for (const row of rows) {
        console.log(`${row.id}  ${row.label}  created ${row.createdAt}${row.revokedAt ? `  revoked ${row.revokedAt}` : ""}`);
      }
      return 0;
    }
    case "revoke": {
      const id = rest[0];
      if (!id) {
        console.error(`${PREFIX} usage: wordink-gateway tokens revoke <id>`);
        return 1;
      }
      if (await store.revoke(id)) {
        console.log(`revoked ${id}`);
        return 0;
      }
      console.error(`${PREFIX} no live token with id ${id}`);
      return 1;
    }
    default:
      console.error(`${PREFIX} usage: wordink-gateway tokens create <label> | list | revoke <id>`);
      return 1;
  }
}

async function main(argv: string[]): Promise<number> {
  const [command, ...rest] = argv;
  try {
    switch (command) {
      case "serve":
      case "check":
        if (rest.length > 0) {
          console.error(`${PREFIX} "${command}" takes no arguments`);
          return 1;
        }
        return command === "serve" ? await cmdServe() : await cmdCheck();
      case "tokens":
        return await cmdTokens(rest);
      case "help":
      case "--help":
      case "-h":
        console.log(USAGE);
        return 0;
      default:
        if (command !== undefined) console.error(`${PREFIX} unknown command "${command}"`);
        console.error(USAGE);
        return 1;
    }
  } catch (err) {
    // Known operational errors (UsageError, the token store's unreadable-file error) carry a
    // clean message; anything unexpected still prints its stack for debugging. None carry
    // key material: the config names env vars and the store keeps hashes only.
    if (err instanceof UsageError) console.error(`${PREFIX} ${err.message}`);
    else console.error(err);
    return 1;
  }
}

process.exitCode = await main(process.argv.slice(2));
