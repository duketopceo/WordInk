// Integration: a speech-like clip goes through the real AudioWorklet and wasm core and reaches a
// mocked Groq relay route as a valid WAV (U4).
import { expect, test } from "@playwright/test";
import { CLIP_SAMPLE_SOURCE } from "./clip.js";

test.beforeEach(async ({ page, browserName }) => {
  if (browserName !== "webkit") return;
  // WebKit has no fake capture device: stub getUserMedia with a MediaStreamDestination playing the clip.
  await page.addInitScript((sampleSource: string) => {
    const sample = new Function(`return ${sampleSource}`)() as (i: number, rate: number) => number;
    const media = navigator.mediaDevices ?? ({} as MediaDevices);
    Object.defineProperty(navigator, "mediaDevices", { value: media, configurable: true });
    media.getUserMedia = async () => {
      const ctx = new AudioContext();
      await ctx.resume();
      const seconds = 4;
      const buffer = ctx.createBuffer(1, seconds * ctx.sampleRate, ctx.sampleRate);
      const data = buffer.getChannelData(0);
      for (let i = 0; i < data.length; i++) data[i] = sample(i, ctx.sampleRate);
      const source = ctx.createBufferSource();
      source.buffer = buffer;
      source.loop = true;
      const dest = ctx.createMediaStreamDestination();
      source.connect(dest);
      source.start();
      return dest.stream;
    };
  }, CLIP_SAMPLE_SOURCE);
});

/** Parses a RIFF/WAVE header out of a multipart body. */
function findWav(body: Buffer) {
  const at = body.indexOf("RIFF");
  expect(at, "multipart body contains a RIFF file").toBeGreaterThanOrEqual(0);
  const wav = body.subarray(at);
  expect(wav.toString("latin1", 8, 12)).toBe("WAVE");
  expect(wav.toString("latin1", 12, 16)).toBe("fmt ");
  const dataSize = wav.readUInt32LE(40);
  return {
    format: wav.readUInt16LE(20),
    channels: wav.readUInt16LE(22),
    sampleRate: wav.readUInt32LE(24),
    byteRate: wav.readUInt32LE(28),
    bits: wav.readUInt16LE(34),
    dataTag: wav.toString("latin1", 36, 40),
    dataSize,
    riffSize: wav.readUInt32LE(4),
    samples: Array.from({ length: dataSize / 2 }, (_, i) => wav.readInt16LE(44 + i * 2)),
  };
}

test("a recorded clip reaches the mocked Groq relay as a valid 16 kHz mono WAV", async ({ page }) => {
  let body: Buffer | undefined;
  let contentType = "";
  await page.route("**/relay/groq/transcriptions", async (route) => {
    body = route.request().postDataBuffer() ?? undefined;
    contentType = (await route.request().headerValue("content-type")) ?? "";
    await route.fulfill({ status: 200, contentType: "text/plain", body: "hello from the relay\n" });
  });

  await page.goto("/e2e/index.html");
  await page.waitForFunction(() => "wordink" in window);
  await page.evaluate(() => (window as unknown as { wordink: { dictation: { ready: Promise<void> } } }).wordink.dictation.ready);

  await page.click("#mic");
  await expect(page.locator("#state")).toHaveText("listening");
  await page.waitForTimeout(1500);
  await page.click("#mic");
  await expect(page.locator("#final")).toHaveText("hello from the relay");
  await expect(page.locator("#state")).toHaveText("idle");

  const log = await page.evaluate(() => (window as unknown as { wordink: { log: string[] } }).wordink.log);
  expect(log).toEqual([
    "state:requesting-mic",
    "state:listening",
    "state:transcribing",
    "final:hello from the relay",
    "state:idle",
  ]);

  expect(contentType).toMatch(/^multipart\/form-data; boundary=/);
  expect(body).toBeDefined();
  const multipart = body!.toString("latin1");
  expect(multipart).toContain('name="file"; filename="audio.wav"');
  expect(multipart).toContain("Content-Type: audio/wav");
  expect(multipart).toMatch(/name="model"\r\n\r\nwhisper-large-v3-turbo\r\n/);
  expect(multipart).toMatch(/name="response_format"\r\n\r\ntext\r\n/);

  const wav = findWav(body!);
  expect(wav).toMatchObject({ format: 1, channels: 1, sampleRate: 16_000, byteRate: 32_000, bits: 16, dataTag: "data" });
  expect(wav.riffSize).toBe(36 + wav.dataSize);
  // About 1.5 s of audio was captured (allowing for mic start-up).
  expect(wav.dataSize / 32_000).toBeGreaterThan(0.8);
  expect(wav.dataSize / 32_000).toBeLessThan(3);
  // It carries the clip, not silence.
  const peak = wav.samples.reduce((m, s) => Math.max(m, Math.abs(s)), 0);
  expect(peak).toBeGreaterThan(3_000);
});
