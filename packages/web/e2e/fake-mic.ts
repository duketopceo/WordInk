import type { Page } from "@playwright/test";
import { CLIP_SAMPLE_SOURCE } from "../../core/e2e/clip.js";

/** WebKit has no fake capture device: stub getUserMedia with a MediaStreamDestination playing the clip. */
export async function fakeMic(page: Page, browserName: string): Promise<void> {
  if (browserName !== "webkit") return;
  await page.addInitScript((sampleSource: string) => {
    const sample = new Function(`return ${sampleSource}`)() as (i: number, rate: number) => number;
    const media = navigator.mediaDevices ?? ({} as MediaDevices);
    Object.defineProperty(navigator, "mediaDevices", { value: media, configurable: true });
    media.getUserMedia = async () => {
      const ctx = new AudioContext();
      await ctx.resume();
      const buffer = ctx.createBuffer(1, 4 * ctx.sampleRate, ctx.sampleRate);
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
}
