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
 * Writes @wordink/local's hello-world fixture with 0.3 s of silence before and after, normalized
 * toward full scale (gain capped at 1.3 so a quiet fixture isn't overdriven). The leading silence
 * mirrors a real press and absorbs the audio graph's startup latency; the clip must be shorter
 * than the 3.5 s hold so the utterance's tail always lands inside the capture — a 2.5 s tail
 * pushed "world" past the release point and the transcript came back "Hello, Will." (#40).
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
  const pcm = Buffer.alloc(pre + data.length + Math.round(0.3 * rate) * 2);
  let fixturePeak = 1;
  for (let i = 0; i < data.length; i += 2) fixturePeak = Math.max(fixturePeak, Math.abs(data.readInt16LE(i)));
  const gain = Math.min(1.3, (0.98 * 32768) / fixturePeak);
  for (let i = 0; i < data.length; i += 2) pcm.writeInt16LE(Math.round(data.readInt16LE(i) * gain), pre + i);
  // A silent clip fails every dictation as NoSpeech 3.5 s + inference in — catch it here instead.
  let writtenPeak = 0;
  for (let i = 0; i + 2 <= pcm.length; i += 2) writtenPeak = Math.max(writtenPeak, Math.abs(pcm.readInt16LE(i)));
  if (writtenPeak < 100) throw new Error(`fake mic clip is silent (peak ${writtenPeak})`);
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
 * Captures deterministic audio by replacing the capture worklet rather than the device: the
 * addModule load is redirected to a same-named processor that emits the padded clip over
 * port.postMessage, real-time paced on the render clock and resampled to the context rate.
 * BufferSource→MediaStreamDestination→MediaStreamSource proved unreliable under headless
 * Chromium (the track delivered silence to every consumer — #40), while postMessage delivery
 * is exact and can't starve. getUserMedia only has to return a live audio track for
 * createMediaStreamSource; its content is ignored. Call before navigating.
 */
export async function fakeMicFromClip(page: Page): Promise<void> {
  const wav = readFileSync(FAKE_MIC_WAV);
  const rate = wav.readUInt32LE(24);
  const pcm = wav.subarray(44);
  const samples: number[] = [];
  for (let i = 0; i + 2 <= pcm.length; i += 2) samples.push(pcm.readInt16LE(i));
  await page.addInitScript(
    ({ samples, rate }: { samples: number[]; rate: number }) => {
      // Same interface as the app's capture processor: ~20 ms Float32 posts. The clip is
      // emitted once from the press, then the worklet outputs silence like an open mic.
      const moduleSrc = [
        `const CLIP = new Int16Array([${samples.join(",")}]);`,
        `const STEP = ${rate} / sampleRate;`,
        `class WordInkCapture extends AudioWorkletProcessor {`,
        `  constructor() { super(); this.size = Math.max(128, Math.round(sampleRate / 50));`,
        `    this.buf = new Float32Array(this.size); this.n = 0; this.pos = 0; }`,
        `  process(inputs) {`,
        `    const frames = (inputs[0] && inputs[0][0] && inputs[0][0].length) || 128;`,
        `    for (let i = 0; i < frames; i++) {`,
        `      const idx = Math.floor(this.pos), frac = this.pos - idx;`,
        `      const a = idx < CLIP.length ? CLIP[idx] / 32768 : 0;`,
        `      const b = idx + 1 < CLIP.length ? CLIP[idx + 1] / 32768 : 0;`,
        `      this.buf[this.n++] = a + (b - a) * frac;`,
        `      this.pos += STEP;`,
        `      if (this.n === this.size) { this.port.postMessage(this.buf, [this.buf.buffer]);`,
        `        this.buf = new Float32Array(this.size); this.n = 0; }`,
        `    }`,
        `    return true;`,
        `  }`,
        `}`,
        `registerProcessor("wordink-capture", WordInkCapture);`,
      ].join("\n");
      const blobUrl = URL.createObjectURL(new Blob([moduleSrc], { type: "text/javascript" }));
      const origAddModule = AudioWorklet.prototype.addModule;
      // The only worklet module the app loads is wordink-capture; substitute ours for any load.
      AudioWorklet.prototype.addModule = function (this: AudioWorklet) {
        return origAddModule.call(this, blobUrl);
      };
      navigator.mediaDevices.getUserMedia = async () => {
        const ctx = new AudioContext();
        return ctx.createMediaStreamDestination().stream;
      };
    },
    { samples, rate },
  );
}

/** Hold the element's button for `ms` while the fake microphone plays the clip (see {@link fakeMicFromClip}). */
export async function holdToTalk(page: Page, ms: number): Promise<number> {
  const mic = page.locator("wordink-mic").first();
  await mic.locator('[part="button"]').hover();
  await page.mouse.down();
  await page.waitForFunction(
    () => document.querySelector("wordink-mic")?.getAttribute("data-state") === "listening",
    { timeout: 30_000 },
  );
  // Sample the element's level meter through the hold so a silent capture is diagnosable (#40).
  const meter = mic.locator('[part="meter"]');
  const deadline = Date.now() + ms;
  let peak = 0;
  while (Date.now() < deadline) {
    const v = await meter.evaluate((el) => el.style.getPropertyValue("--wordink-level")).catch(() => "");
    peak = Math.max(peak, parseFloat(v) || 0);
    await page.waitForTimeout(200);
  }
  await page.mouse.up();
  return peak;
}

/**
 * Hold, release, and wait for the session to settle (`idle` on a final, `error` on NoSpeech et al.),
 * then return the field's text. `attempts` re-dictates while `match` doesn't hold — the bundled
 * model is Moonshine-tiny and occasionally returns empty or garbled text even on clean captures
 * (#40); a second attempt encodes the same "speak again" recovery a real user performs.
 */
export async function dictate(
  page: Page,
  target: string,
  match: (text: string) => boolean,
  { attempts = 2, holdMs = 3500 } = {},
): Promise<{ text: string; state: string | null; peak: number }> {
  const mic = page.locator("wordink-mic").first();
  let peak = 0;
  let text = "";
  let state: string | null = null;
  for (let i = 0; i < attempts; i++) {
    peak = Math.max(peak, await holdToTalk(page, holdMs));
    await page.waitForFunction(
      () => {
        const s = document.querySelector("wordink-mic")?.getAttribute("data-state");
        return s === "idle" || s === "error";
      },
      // The provider watchdog fails a stuck transcription at ~30 s, so 60 s bounds an attempt
      // well under the test timeout and leaves room for a second try.
      { timeout: 60_000 },
    );
    state = await mic.getAttribute("data-state");
    text = await page.locator(target).inputValue().catch(() => "");
    if (text.trim() && match(text)) break;
  }
  return { text, state, peak };
}
