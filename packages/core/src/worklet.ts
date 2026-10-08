/**
 * Microphone capture: getUserMedia into an AudioWorklet that posts mono Float32 frames to the main
 * thread (KTD4: no MediaRecorder, the core resamples from the device rate).
 *
 * The worklet module is an inlined string loaded from a Blob URL, so the package works from a CDN
 * with no extra file to host.
 */

const PROCESSOR = "wordink-capture";

/** Batches 128-frame render quanta into ~20 ms posts, transferring each buffer. */
export const WORKLET_SOURCE = `
class WordInkCapture extends AudioWorkletProcessor {
  constructor() {
    super();
    this.size = Math.max(128, Math.round(sampleRate / 50));
    this.buf = new Float32Array(this.size);
    this.n = 0;
  }
  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (ch) {
      let i = 0;
      while (i < ch.length) {
        const k = Math.min(ch.length - i, this.size - this.n);
        this.buf.set(ch.subarray(i, i + k), this.n);
        this.n += k;
        i += k;
        if (this.n === this.size) {
          this.port.postMessage(this.buf, [this.buf.buffer]);
          this.buf = new Float32Array(this.size);
          this.n = 0;
        }
      }
    }
    return true;
  }
}
registerProcessor(${JSON.stringify(PROCESSOR)}, WordInkCapture);
`;

let workletUrl: string | undefined;

function moduleUrl(): string {
  workletUrl ??= URL.createObjectURL(new Blob([WORKLET_SOURCE], { type: "text/javascript" }));
  return workletUrl;
}

/** An open microphone. `close()` stops the tracks (releasing the device) and the audio graph. */
export interface Mic {
  readonly sampleRate: number;
  close(): void;
}

/**
 * Creates (and resumes) the AudioContext. Call it synchronously inside the user gesture that starts
 * dictation: Safari only lets a context run if it was resumed during a gesture.
 */
export function createAudioContext(): AudioContext {
  const ctx = new AudioContext();
  void ctx.resume().catch(() => {});
  return ctx;
}

/**
 * Opens the microphone into `ctx` and calls `onAudio` with mono Float32 frames at `ctx.sampleRate`.
 * Rejects with the getUserMedia error (a DOMException such as NotAllowedError) on failure; `ctx` is
 * closed on failure and by `Mic.close()`.
 */
export async function openMic(ctx: AudioContext, onAudio: (samples: Float32Array) => void): Promise<Mic> {
  let stream: MediaStream | undefined;
  try {
    const media = globalThis.navigator?.mediaDevices;
    if (!media?.getUserMedia) {
      throw Object.assign(new Error("Microphone capture needs a secure context (https or localhost)."), {
        name: "InsecureContextError",
      });
    }
    stream = await media.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
    await ctx.audioWorklet.addModule(moduleUrl());
    const source = ctx.createMediaStreamSource(stream);
    const node = new AudioWorkletNode(ctx, PROCESSOR, {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [1],
    });
    node.port.onmessage = (e: MessageEvent<Float32Array>) => onAudio(e.data);
    source.connect(node);
    // The node outputs silence; connecting it keeps it in the rendered graph in every browser.
    node.connect(ctx.destination);
    const tracks = stream.getTracks();
    let closed = false;
    return {
      sampleRate: ctx.sampleRate,
      close() {
        if (closed) return;
        closed = true;
        node.port.onmessage = null;
        source.disconnect();
        node.disconnect();
        for (const t of tracks) t.stop();
        void ctx.close().catch(() => {});
      },
    };
  } catch (err) {
    for (const t of stream?.getTracks() ?? []) t.stop();
    void ctx.close().catch(() => {});
    throw err;
  }
}
