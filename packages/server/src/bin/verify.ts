// `wordink-gateway verify [--live] [--config <path>]` — one command that proves the
// gateway's mechanics on any host with Node 20+, no provider keys required. Everything
// runs against a temp XDG sandbox and a loopback fake provider; --live additionally
// exercises each real provider entry (read-only from the operator's config) with the
// bundled speech fixture. Tokens mint only into the temp store — never a real one.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DEFAULT_MODELS, type ProviderEntry } from "../gateway/providers.js";
import { UsageError, defaultConfigPath, loadConfig, resolveKeys } from "./gateway-config.js";

const PREFIX = "wordink-gateway:";
const BIN = join(dirname(fileURLToPath(import.meta.url)), "wordink-gateway.js");
const FIXTURE = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "fixtures", "hello-world.wav");
const FAKE_KEY_ENV = "WORDINK_VERIFY_KEY";
const FAKE_TEXT = "from the fake provider";
const TOKEN_RE = /wdk_[A-Za-z0-9_-]{43}/;
const TOKEN_ID_RE = /[0-9a-f]{8}-[0-9a-f-]{27}/;

interface Check {
  name: string;
  status: "PASS" | "FAIL" | "SKIP";
  detail?: string;
}

class CheckFailed extends Error {}

function report(checks: Check[], name: string, status: Check["status"], detail?: string): void {
  checks.push({ name, status, ...(detail !== undefined ? { detail } : {}) });
  console.log(`${status.padEnd(4)} ${name}${detail ? ` — ${detail}` : ""}`);
}

/** Run the sibling bin as a child process; resolves with captured output. */
function run(
  args: string[],
  env: NodeJS.ProcessEnv,
  timeoutMs = 30_000,
): Promise<{ code: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [BIN, ...args], { env });
    let stdout = "";
    let stderr = "";
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`timed out after ${timeoutMs}ms: ${args.join(" ")}`));
    }, timeoutMs);
    child.stdout.on("data", (d: Buffer) => (stdout += d.toString()));
    child.stderr.on("data", (d: Buffer) => (stderr += d.toString()));
    child.on("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    child.on("close", (code) => {
      clearTimeout(timer);
      resolve({ code, stdout, stderr });
    });
  });
}

/** Spawn `serve` against `env`, resolving once its stdout announces the listener. */
function spawnServe(env: NodeJS.ProcessEnv): Promise<ChildProcess> {
  const child = spawn(process.execPath, [BIN, "serve"], { env });
  return new Promise((resolve, reject) => {
    let out = "";
    let err = "";
    const onExit = () => {
      clearTimeout(timer);
      reject(new Error(`serve exited before listening; stderr: ${err}`));
    };
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      reject(new Error(`serve produced no listener line within 15s; stderr so far: ${err}`));
    }, 15_000);
    child.stderr.on("data", (d: Buffer) => (err += d.toString()));
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
      if (/listening on/i.test(out)) {
        clearTimeout(timer);
        child.removeListener("exit", onExit);
        resolve(child);
      }
    });
    child.on("exit", onExit);
  });
}

function stop(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return new Promise((resolve) => {
    child.once("close", () => resolve());
    child.kill("SIGKILL");
  });
}

async function freePort(): Promise<number> {
  const s = createServer();
  await new Promise<void>((r) => s.listen(0, "127.0.0.1", r));
  const port = (s.address() as AddressInfo).port;
  await new Promise<void>((r) => s.close(() => r()));
  return port;
}

/** A stub transcription endpoint: answers every POST `{"text": FAKE_TEXT}`. */
async function fakeProvider(): Promise<{ server: Server; url: string }> {
  const server = createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ text: FAKE_TEXT }));
    });
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  return { server, url: `http://127.0.0.1:${(server.address() as AddressInfo).port}` };
}

/** A clean child environment: temp XDG dirs, no ambient provider keys can leak in. */
function sandboxEnv(home: string, extra: Record<string, string> = {}): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {
    PATH: process.env.PATH ?? "/usr/bin",
    HOME: home,
    XDG_CONFIG_HOME: join(home, "config"),
    XDG_STATE_HOME: join(home, "state"),
    ...extra,
  };
  // Windows needs these for node to spawn children at all.
  if (process.env.SystemRoot) env.SystemRoot = process.env.SystemRoot;
  if (process.env.SYSTEMROOT) env.SYSTEMROOT = process.env.SYSTEMROOT;
  return env;
}

function writeSandboxConfig(home: string, config: unknown): void {
  const dir = join(home, "config", "wordink");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "gateway.json"), JSON.stringify(config));
}

async function mintToken(
  env: NodeJS.ProcessEnv,
  admin = false,
): Promise<{ token: string; id: string }> {
  const args = admin ? ["tokens", "create", "--admin", "verify"] : ["tokens", "create", "verify"];
  const r = await run(args, env);
  if (r.code !== 0) throw new CheckFailed(`tokens create exited ${r.code}: ${r.stderr.trim()}`);
  const token = r.stdout.match(TOKEN_RE)?.[0];
  const id = r.stdout.match(TOKEN_ID_RE)?.[0];
  if (!token || !id) throw new CheckFailed("tokens create printed no wdk_ token / id");
  return { token, id };
}

async function fetchJson(url: string, init?: RequestInit): Promise<{ status: number; body: unknown }> {
  const res = await fetch(url, { ...init, signal: AbortSignal.timeout(10_000) });
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    /* non-JSON body is fine for status assertions */
  }
  return { status: res.status, body };
}

function wavForm(bytes: Uint8Array, model: string): FormData {
  const form = new FormData();
  form.append("file", new Blob([bytes], { type: "audio/wav" }), "audio.wav");
  form.append("model", model);
  return form;
}

export async function cmdVerify(args: string[]): Promise<number> {
  const live = args.includes("--live");
  const configIdx = args.indexOf("--config");
  const configPath = configIdx >= 0 ? args[configIdx + 1] : undefined;
  const rest = args.filter((a, i) => {
    if (a === "--live" || a === "--config") return false;
    return configIdx < 0 || i !== configIdx + 1;
  });
  if (configIdx >= 0 && (configPath === undefined || configPath.startsWith("--"))) {
    console.error(`${PREFIX} usage: wordink-gateway verify [--live] [--config <path>]`);
    return 1;
  }
  if (!live && configPath !== undefined) {
    console.error(`${PREFIX} verify: --config only applies to --live runs (offline checks use a sandbox config)`);
    return 1;
  }
  if (rest.length > 0) {
    console.error(`${PREFIX} verify: unexpected arguments: ${rest.join(" ")}`);
    return 1;
  }

  const checks: Check[] = [];
  const children: ChildProcess[] = [];
  let fake: Server | undefined;
  const home = mkdtempSync(join(tmpdir(), "wdk-verify-"));
  try {
    // --- offline mechanics, all inside the temp sandbox ---
    const fakeUp = await fakeProvider();
    fake = fakeUp.server;
    const port = await freePort();
    writeSandboxConfig(home, {
      port,
      providers: [{ provider: "groq", keyEnv: FAKE_KEY_ENV, url: fakeUp.url }],
    });
    const env = sandboxEnv(home, { [FAKE_KEY_ENV]: "verify-fake-key" });
    report(checks, "config resolves", "PASS", `sandbox ${home}`);

    const checked = await run(["check"], env);
    if (checked.code !== 0) throw new CheckFailed(`check exited ${checked.code}: ${checked.stderr.trim()}`);
    report(checks, "check", "PASS", "provider key resolves");

    const serveProc = await spawnServe(env);
    children.push(serveProc);
    report(checks, "serve binds", "PASS", `port ${port}`);

    const device = await mintToken(env);
    const admin = await mintToken(env, true);
    report(checks, "tokens minted", "PASS", "device + admin (sandbox store only)");

    const base = `http://127.0.0.1:${port}`;
    const models = await fetchJson(`${base}/v1/models`, {
      headers: { authorization: `Bearer ${device.token}` },
    });
    if (models.status !== 200) throw new CheckFailed(`/v1/models answered ${models.status}`);
    const modelIds = Array.isArray((models.body as { data?: { id: string }[] })?.data)
      ? (models.body as { data: { id: string }[] }).data.map((m) => m.id)
      : [];
    if (!modelIds.includes(DEFAULT_MODELS.groq)) {
      throw new CheckFailed(`/v1/models lacks ${DEFAULT_MODELS.groq}: ${JSON.stringify(modelIds)}`);
    }
    report(checks, "GET /v1/models", "PASS", modelIds.join(", "));

    const wav = readFileSync(FIXTURE);
    const tx = await fetchJson(`${base}/v1/audio/transcriptions`, {
      method: "POST",
      headers: { authorization: `Bearer ${device.token}` },
      body: wavForm(wav, DEFAULT_MODELS.groq),
    });
    const text = (tx.body as { text?: string })?.text;
    if (tx.status !== 200 || text !== FAKE_TEXT) {
      throw new CheckFailed(`transcription answered ${tx.status} ${JSON.stringify(tx.body)}`);
    }
    report(checks, "POST /v1/audio/transcriptions", "PASS", `"${text}"`);

    const denied = await fetchJson(`${base}/v1/vocabulary`, {
      headers: { authorization: `Bearer ${device.token}` },
    });
    if (denied.status !== 403) throw new CheckFailed(`device vocabulary GET answered ${denied.status}, want 403`);
    report(checks, "vocabulary scope gate", "PASS", "device token refused 403");

    const added = await fetchJson(`${base}/v1/vocabulary`, {
      method: "POST",
      headers: { authorization: `Bearer ${admin.token}`, "content-type": "application/json" },
      body: JSON.stringify({ terms: ["WordInkVerify"] }),
    });
    if (added.status !== 200) throw new CheckFailed(`vocabulary POST answered ${added.status}`);
    const listed = await fetchJson(`${base}/v1/vocabulary`, {
      headers: { authorization: `Bearer ${admin.token}` },
    });
    const entries = (listed.body as { data?: { term: string }[] })?.data ?? [];
    if (listed.status !== 200 || !entries.some((e) => e.term === "WordInkVerify")) {
      throw new CheckFailed(`written term missing from vocabulary GET: ${JSON.stringify(listed.body)}`);
    }
    report(checks, "vocabulary write+read", "PASS", "term effective after POST");

    const revoked = await run(["tokens", "revoke", device.id], env);
    if (revoked.code !== 0) throw new CheckFailed(`tokens revoke exited ${revoked.code}`);
    const after = await fetchJson(`${base}/v1/models`, {
      headers: { authorization: `Bearer ${device.token}` },
    });
    if (after.status !== 401) throw new CheckFailed(`revoked token answered ${after.status}, want 401`);
    report(checks, "token revoke", "PASS", "next request refused 401");

    // --- --live: one isolated serve per resolved provider entry ---
    if (live) {
      const realPath = configPath ?? defaultConfigPath();
      const real = await loadConfig(realPath); // UsageError names the path when absent
      const { skipped } = resolveKeys(real.providers, process.env);
      // Keep keyEnv attached (ResolvedEntry types it away) — the key name, not value,
      // is what the isolated child config needs.
      const resolved = real.providers.flatMap((entry) => {
        const key = process.env[entry.keyEnv];
        return typeof key === "string" && key.length > 0 ? [{ ...entry, key }] : [];
      });
      if (resolved.length === 0) {
        report(
          checks,
          "live providers",
          "SKIP",
          `no key resolved in ${realPath} (wants: ${skipped.map((e) => e.keyEnv).join(", ") || "none"})`,
        );
      }
      for (const entry of resolved) {
        const label = `${entry.provider} (${entry.model ?? DEFAULT_MODELS[entry.provider]})`;
        try {
          const transcript = await liveEntry(home, entry, children);
          if (!/hello/i.test(transcript) || !/world/i.test(transcript)) {
            throw new CheckFailed(`transcript "${transcript}" lacks hello+world`);
          }
          report(checks, `live ${label}`, "PASS", `"${transcript}"`);
        } catch (err) {
          report(
            checks,
            `live ${label}`,
            "FAIL",
            err instanceof Error ? err.message : String(err),
          );
        }
      }
    }
  } catch (err) {
    if (err instanceof UsageError) {
      console.error(`${PREFIX} ${err.message}`);
      return 1;
    }
    report(checks, "verify", "FAIL", err instanceof Error ? err.message : String(err));
  } finally {
    for (const child of children.splice(0)) await stop(child);
    if (fake) await new Promise<void>((r) => fake!.close(() => r()));
    rmSync(home, { recursive: true, force: true });
  }

  const failed = checks.filter((c) => c.status === "FAIL");
  if (failed.length > 0) {
    console.error(`${PREFIX} ${failed.length} check${failed.length === 1 ? "" : "s"} failed: ${failed.map((f) => f.name).join("; ")}`);
    return 1;
  }
  console.log(`${PREFIX} all ${checks.length} checks passed`);
  return 0;
}

/** One transcription through one real provider entry, in its own sandboxed serve. */
async function liveEntry(
  home: string,
  entry: ProviderEntry & { key: string },
  children: ChildProcess[],
): Promise<string> {
  const dir = mkdtempSync(join(home, "live-"));
  const port = await freePort();
  const { key, ...entrySansKey } = entry;
  writeSandboxConfig(dir, { port, providers: [entrySansKey] });
  const env = sandboxEnv(dir, { [entry.keyEnv]: key });
  const serveProc = await spawnServe(env);
  children.push(serveProc);
  try {
    const { token } = await mintToken(env);
    const wav = readFileSync(FIXTURE);
    const res = await fetchJson(`http://127.0.0.1:${port}/v1/audio/transcriptions`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}` },
      body: wavForm(wav, entry.model ?? DEFAULT_MODELS[entry.provider]),
    });
    const text = (res.body as { text?: string })?.text;
    if (res.status !== 200 || typeof text !== "string") {
      throw new CheckFailed(`answered ${res.status}: ${JSON.stringify(res.body)}`);
    }
    return text;
  } finally {
    await stop(serveProc);
    children.splice(children.indexOf(serveProc), 1);
  }
}
