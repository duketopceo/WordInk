// Browser API fakes for the unit tests: getUserMedia + AudioContext + AudioWorkletNode, WebSocket.
// The wasm core and the host's effect loop under test are real.
import { vi } from "vitest";

export interface FakeAudio {
  /** Tracks handed out by getUserMedia; `stopped` once released. */
  tracks: { stopped: boolean }[];
  getUserMedia: ReturnType<typeof vi.fn>;
  /** Pushes `seconds` of a 220 Hz tone (speech-level RMS) through the worklet port, in 20 ms frames. */
  speak(seconds: number, amplitude?: number): void;
}

export function installFakeAudio(options: { sampleRate?: number; deny?: string } = {}): FakeAudio {
  const sampleRate = options.sampleRate ?? 48_000;
  const nodes: FakeWorkletNode[] = [];
  const tracks: { stopped: boolean }[] = [];

  class FakeAudioContext {
    sampleRate = sampleRate;
    state = "running";
    destination = {};
    audioWorklet = { addModule: async (_url: string) => {} };
    async resume() {}
    async close() {
      this.state = "closed";
    }
    createMediaStreamSource() {
      return { connect() {}, disconnect() {} };
    }
  }
  class FakeWorkletNode {
    port: { onmessage: ((e: { data: Float32Array }) => void) | null } = { onmessage: null };
    constructor() {
      nodes.push(this);
    }
    connect() {}
    disconnect() {}
  }

  const getUserMedia = vi.fn(async () => {
    if (options.deny) throw new DOMException("Permission denied", options.deny);
    const track = {
      stopped: false,
      stop() {
        this.stopped = true;
      },
    };
    tracks.push(track);
    return { getTracks: () => [track] };
  });

  vi.stubGlobal("AudioContext", FakeAudioContext);
  vi.stubGlobal("AudioWorkletNode", FakeWorkletNode);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia } });

  let phase = 0;
  return {
    tracks,
    getUserMedia,
    speak(seconds, amplitude = 0.3) {
      const node = nodes.at(-1);
      if (!node?.port.onmessage) throw new Error("microphone is not open");
      const frame = sampleRate / 50;
      const frames = Math.round((seconds * sampleRate) / frame);
      for (let f = 0; f < frames; f++) {
        const data = new Float32Array(frame);
        for (let i = 0; i < frame; i++) data[i] = amplitude * Math.sin((2 * Math.PI * 220 * phase++) / sampleRate);
        node.port.onmessage({ data });
      }
    },
  };
}

export class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  readyState = FakeWebSocket.CONNECTING;
  binaryType = "blob";
  sent: (string | Uint8Array)[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string | ArrayBuffer }) => void) | null = null;
  onclose: ((e: { code: number }) => void) | null = null;

  constructor(
    readonly url: string,
    readonly protocols: string[],
  ) {
    FakeWebSocket.instances.push(this);
  }
  send(data: string | Uint8Array) {
    this.sent.push(data);
  }
  close() {
    this.readyState = FakeWebSocket.CLOSED;
  }
  // Test drivers: what the server does.
  serverOpen() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.();
  }
  serverMessage(text: string) {
    this.onmessage?.({ data: text });
  }
  serverClose(code: number) {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.({ code });
  }
}

/** Records every event a dictation emits, in order, as `type:value` strings. */
export function record(d: { on(type: string, cb: (v: unknown) => void): () => void }): string[] {
  const log: string[] = [];
  for (const type of ["state", "interim", "final", "error", "warning"]) {
    d.on(type, (v) => {
      const value = typeof v === "string" ? v : (v as { code: string }).code;
      log.push(`${type}:${value}`);
    });
  }
  return log;
}
