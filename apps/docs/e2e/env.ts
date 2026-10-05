// Shared e2e settings and network fakes. The pages under test use the real third-party URLs (jsDelivr
// for the CDN bundle and onnxruntime-web, huggingface.co for the model); these routes answer them from
// local files so the run is fast and the CDN bundle is the one just built.
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { BrowserContext, Page } from "@playwright/test";

export const SITE_PORT = 4341;
export const QUICKSTART_PORT = 4342;
/** A non-localhost name for the built site, so the demo runs as it does on GitHub Pages. */
export const DOCS_HOST = "docs.wordink.test";

const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));
export const DOCS_DIR = here("../");
/** The clean directory the quickstart spec builds its page in (emptied by the spec). */
export const QUICKSTART_DIR = here("./.work/quickstart/");
const FIXTURE_WAV = here("../../../packages/local/test/fixtures/hello-world.wav");
/** What the fake microphone plays once per mic open: see {@link writeFakeMicClip} and {@link fakeMicFromClip}. */
export const FAKE_MIC_WAV = here("./.work/hello-world-padded.wav");
const WEB_DIST = here("../../../packages/web/dist/");
// Same on-disk model cache as @wordink/local's e2e, so reruns skip the download.
const MODEL_CACHE = process.env.WORDINK_MODEL_CACHE ?? join(homedir(), ".cache", "wordink-models");
// onnxruntime-web is a dependency of transformers.js, which is a dependency of @wordink/local.
// pnpm puts it next to transformers.js.
const localPkg = realpathSync(join(DOCS_DIR, "node_modules", "@wordink", "local"));
const transformers = realpathSync(join(localPkg, "node_modules", "@huggingface", "transformers"));
const ORT_DIST = join(transformers, "..", "..", "onnxruntime-web", "dist");

const CORS = { "access-control-allow-origin": "*", "cache-control": "no-store" };

/**
 * Writes @wordink/local's hello-world fixture with 0.3 s of silence before and 2.5 s after, at half
 * volume. The leading silence mirrors a real press, the trailing silence fills the rest of a 3.5 s
 * hold, and the lower gain leaves headroom for the capture's automatic gain control, which clips the original.
 */
export function writeFakeMicClip(): void {
  const wav = readFileSync(FIXTURE_WAV);
  let at = 12;
  let data: Buffer | undefined;
  while (at + 8 <= wav.length) {
    const size = wav.readUInt32LE(at + 4);
    if (wav.toString("ascii", at, at + 4) === "data") data = wav.subarray(at + 8, at + 8 + size);
    at += 8 + size + (size & 1);
  }
  if (!data) throw new Error("no data chunk in the fixture");
  const rate = wav.readUInt32LE(24);
  const pre = Math.round(0.3 * rate) * 2;
  const pcm = Buffer.alloc(pre + data.length + Math.round(2.5 * rate) * 2);
  for (let i = 0; i < data.length; i += 2) pcm.writeInt16LE(Math.round(data.readInt16LE(i) * 0.5), pre + i);
  // RIFF header and the 16-byte fmt chunk, then a bare data chunk (dropping the LIST chunk).
  const header = Buffer.alloc(44);
  wav.copy(header, 0, 0, 36);
  header.write("data", 36);
  header.writeUInt32LE(36 + pcm.length, 4);
  header.writeUInt32LE(pcm.length, 40);
  mkdirSync(dirname(FAKE_MIC_WAV), { recursive: true });
  writeFileSync(FAKE_MIC_WAV, Buffer.concat([header, pcm]));
}

export interface Served {
  cdn: string[];
  model: string[];
  ort: string[];
}

/** Answer jsDelivr and huggingface.co from local files. Returns what was served, for assertions. */
export async function fakeThirdParties(context: BrowserContext): Promise<Served> {
  const served: Served = { cdn: [], model: [], ort: [] };

  // "The CDN": @wordink/web@<any version>/dist/* from the local build.
  await context.route(/^https:\/\/cdn\.jsdelivr\.net\/npm\/@wordink\/web@[^/]+\/dist\/([^/?]+)/, async (route) => {
    const file = basename(new URL(route.request().url()).pathname);
    served.cdn.push(file);
    const type = file.endsWith(".wasm") ? "application/wasm" : "text/javascript";
    await route.fulfill({ body: await readFile(join(WEB_DIST, file)), headers: { ...CORS, "content-type": type } });
  });

  // onnxruntime-web runtime files that transformers.js loads from jsDelivr by default.
  await context.route(/^https:\/\/cdn\.jsdelivr\.net\/npm\/onnxruntime-web@[^/]+\/dist\/([^/?]+)/, async (route) => {
    const file = basename(new URL(route.request().url()).pathname);
    served.ort.push(file);
    const type = file.endsWith(".wasm") ? "application/wasm" : "text/javascript";
    await route.fulfill({ body: await readFile(join(ORT_DIST, file)), headers: { ...CORS, "content-type": type } });
  });

  // Model files at the pinned revision: from the disk cache, or fetched once and cached.
  await context.route(/^https:\/\/huggingface\.co\/(.+\/resolve\/.+)$/, async (route) => {
    const path = new URL(route.request().url()).pathname.slice(1);
    const file = join(MODEL_CACHE, path);
    served.model.push(path);
    let body: Buffer;
    try {
      body = await readFile(file);
    } catch {
      const res = await route.fetch();
      // Only a whole-file 200 is cached (transformers.js also sends HEAD and Range probes).
      const whole = route.request().method() === "GET" && !route.request().headers()["range"] && res.status() === 200;
      if (!whole) return route.fulfill({ response: res, headers: { ...res.headers(), ...CORS } });
      body = await res.body();
      await mkdir(dirname(file), { recursive: true });
      await writeFile(`${file}.part`, body);
      await rename(`${file}.part`, file);
    }
    await route.fulfill({ body, headers: { ...CORS, "content-type": "application/octet-stream" } });
  });

  return served;
}

/** Hide WebGPU so the engine takes the WASM path (smaller download, deterministic across machines). */
export const withoutWebGPU = (page: Page) =>
  page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
  });

/**
 * Stubs getUserMedia so each mic open plays the padded clip once from its start. Chromium's
 * file-backed fake device starts looping at browser launch, so where a hold lands in the clip
 * depended on how long setup took, and a hold that caught the loop restarting ("…world. Hel")
 * transcribed to nothing. Call before navigating.
 */
export async function fakeMicFromClip(page: Page): Promise<void> {
  const wav = readFileSync(FAKE_MIC_WAV);
  const rate = wav.readUInt32LE(24);
  const pcm = wav.subarray(44).toString("base64");
  await page.addInitScript(
    ({ pcm, rate }: { pcm: string; rate: number }) => {
      const bytes = Uint8Array.from(atob(pcm), (c) => c.charCodeAt(0));
      const samples = new Int16Array(bytes.buffer);
      navigator.mediaDevices.getUserMedia = async () => {
        const ctx = new AudioContext();
        await ctx.resume();
        const buffer = ctx.createBuffer(1, samples.length, rate);
        const data = buffer.getChannelData(0);
        for (let i = 0; i < samples.length; i++) data[i] = samples[i]! / 32768;
        const source = ctx.createBufferSource();
        source.buffer = buffer;
        const dest = ctx.createMediaStreamDestination();
        source.connect(dest);
        source.start();
        return dest.stream;
      };
    },
    { pcm, rate },
  );
}

/** Hold the element's button for `ms` while the fake microphone plays the clip (see {@link fakeMicFromClip}). */
export async function holdToTalk(page: Page, ms: number): Promise<void> {
  const mic = page.locator("wordink-mic").first();
  await mic.locator('[part="button"]').hover();
  await page.mouse.down();
  await page.waitForFunction(() => document.querySelector("wordink-mic")?.getAttribute("data-state") === "listening");
  await page.waitForTimeout(ms);
  await page.mouse.up();
}
