#!/usr/bin/env node
// wordink-gateway: run and administer the WordInk desktop gateway (KTD8, R9, R13).
// The config file names the environment variables holding provider keys — never key
// material itself; the systemd wrapper (examples/gateway-systemd) fills them from omaseal.
import { createServer } from "node:http";
import { createNodeHandler, FileTokenStore, FileVocabularyStore, defaultTokenPath, defaultVocabularyPath } from "../node.js";
import { DEFAULT_MODELS } from "../gateway/providers.js";
import {
  UsageError,
  defaultConfigPath,
  isLoopback,
  keyEnvHint,
  loadConfig,
  resolveKeys,
} from "./gateway-config.js";
import { cmdVerify } from "./verify.js";

const PREFIX = "wordink-gateway:";
const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 8941;

const USAGE = `Usage: wordink-gateway <command>

  serve                  Run the gateway (config: $XDG_CONFIG_HOME/wordink/gateway.json)
  check                  Resolve the config and report which provider keys are present
  verify [--live] [--config <path>]
                         Prove the gateway's mechanics against a sandboxed fake
                         provider — no provider keys needed; --live also exercises
                         each real provider entry that resolves a key
  tokens create [--admin] <label>
                         Mint a device token (or an admin token with --admin);
                         the token is printed exactly once
  tokens list            List device tokens: id, scope, label, created, revoked (never the token)
  tokens revoke <id>     Revoke a device token; it is refused on its next request
`;

/** Without one live admin token every /v1/vocabulary call 403s — say so at check/serve (R11). */
async function warnIfNoAdmin(store: FileTokenStore): Promise<void> {
  try {
    const live = await store.list();
    if (!live.some((t) => t.scope === "admin" && !t.revokedAt)) {
      console.warn(`${PREFIX} warning: no live admin token — /v1/vocabulary will refuse everything. ` +
        `Mint one with \`wordink-gateway tokens create --admin <label>\`.`);
    }
  } catch {
    console.warn(`${PREFIX} warning: cannot read the token store to check for an admin token.`);
  }
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
  await warnIfNoAdmin(new FileTokenStore());
  if (config.vocabulary !== undefined) console.log(`vocabulary: ${config.vocabulary.length} config terms`);
  const vocabStore = new FileVocabularyStore();
  try {
    const runtime = await vocabStore.list();
    console.log(`vocabulary: +${runtime.length} runtime terms (${defaultVocabularyPath()})`);
  } catch {
    console.log(`vocabulary: ${defaultVocabularyPath()} is unreadable — transcriptions will use config terms only`);
  }
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

  const tokenStore = new FileTokenStore();
  await warnIfNoAdmin(tokenStore);
  const handler = createNodeHandler({
    keys: {},
    gateway: {
      tokenStore,
      providers: resolved,
      vocabulary: config.vocabulary,
      vocabularyStore: new FileVocabularyStore(),
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
      // `--admin` is a real flag, pulled out anywhere in the args — folding it into
      // the joined label would mint a token literally named "--admin laptop".
      const admin = rest.includes("--admin");
      const label = rest.filter((a) => a !== "--admin").join(" ").trim();
      if (!label) {
        console.error(`${PREFIX} usage: wordink-gateway tokens create [--admin] <label>`);
        return 1;
      }
      const made = await store.create(label, admin ? "admin" : "device");
      // The full token is printed exactly once (R10); only its hash is stored.
      console.log(made.token);
      console.log(
        `created id=${made.id} scope=${made.scope} label="${made.label}" — store the token now; it is never shown again.`,
      );
      return 0;
    }
    case "list": {
      const rows = await store.list();
      if (rows.length === 0) console.log("no device tokens");
      for (const row of rows) {
        console.log(
          `${row.id}  ${row.scope ?? "device"}  ${row.label}  created ${row.createdAt}${row.revokedAt ? `  revoked ${row.revokedAt}` : ""}`,
        );
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
      case "verify":
        return await cmdVerify(rest);
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
