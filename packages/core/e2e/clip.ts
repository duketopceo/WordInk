// A speech-like test clip: 220 Hz voiced bursts with harmonics, 300 ms on / 100 ms off.
export const CLIP_RATE = 48_000;

/** Source of the clip's sample function, shared with the WebKit getUserMedia stub (runs in the page). */
export const CLIP_SAMPLE_SOURCE = `(i, rate) => {
  const t = i / rate;
  const on = t % 0.4 < 0.3 ? 1 : 0;
  const f = 220;
  return on * (0.25 * Math.sin(2 * Math.PI * f * t) + 0.1 * Math.sin(4 * Math.PI * f * t) + 0.05 * Math.sin(6 * Math.PI * f * t));
}`;

const clipSample = new Function(`return ${CLIP_SAMPLE_SOURCE}`)() as (i: number, rate: number) => number;

/** The clip as a mono 16-bit PCM WAV file (for Chromium's --use-file-for-fake-audio-capture). */
export function clipWav(seconds = 4): Buffer {
  const n = Math.round(seconds * CLIP_RATE);
  const buf = Buffer.alloc(44 + n * 2);
  buf.write("RIFF", 0);
  buf.writeUInt32LE(36 + n * 2, 4);
  buf.write("WAVE", 8);
  buf.write("fmt ", 12);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22);
  buf.writeUInt32LE(CLIP_RATE, 24);
  buf.writeUInt32LE(CLIP_RATE * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write("data", 36);
  buf.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) buf.writeInt16LE(Math.round(clipSample(i, CLIP_RATE) * 32767), 44 + i * 2);
  return buf;
}
