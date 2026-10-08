import type { ErrorCode } from "./errors.js";

/** Built-in cloud providers. Groq is the default (batch only, KTD10). */
export type CloudProvider = "groq" | "openai" | "deepgram";

/** What a host provider reports back for the current utterance. */
export type HostProviderResult =
  | { type: "interim"; text: string }
  | { type: "final"; text: string }
  | { type: "error"; code: Exclude<ErrorCode, "Config" | "MicDenied"> };

/** What a host provider can do, declared once at registration. */
export interface HostProviderCapabilities {
  /** Takes audio while the user speaks (and may report interim text). Batch providers get the whole utterance on release. */
  streaming: boolean;
  /** Rate in Hz of the mono PCM16 audio the provider wants. The core resamples to it (KTD4). */
  sampleRate: number;
  /**
   * How long, in ms, the host waits for a result after release before failing the utterance with
   * ProviderDown. Defaults to `TRANSCRIBE_TIMEOUT_MS`. Set it longer when the first result can include
   * a model download; the provider should still fail stalled work itself. Must be a finite number in
   * (0, 2147483647].
   */
  timeoutMs?: number;
}

/**
 * A provider implemented in JavaScript, such as `@wordink/local` (KTD5). The core delivers resampled
 * PCM16 audio to it and treats its results like any provider's, so the session behaves the same.
 *
 * Per utterance: `start`, then `pushAudio` (while speaking if streaming, else all at once on release),
 * then `finish`, after which the provider reports exactly one `final` or `error` through `onResult`.
 * `cancel` means the utterance was abandoned and no result is wanted.
 */
export interface HostProvider {
  /** Stable id, echoed in the core's effects. */
  readonly id: string;
  readonly capabilities: HostProviderCapabilities;
  start(sampleRate: number, hint?: string): void | Promise<void>;
  pushAudio(pcm: Int16Array): void | Promise<void>;
  finish(): void | Promise<void>;
  cancel?(): void | Promise<void>;
  /** Subscribe to results. May return an unsubscribe function, called on `destroy()`. */
  onResult(callback: (result: HostProviderResult) => void): void | (() => void);
}

export function isHostProvider(value: unknown): value is HostProvider {
  if (typeof value !== "object" || value === null) return false;
  const p = value as Partial<HostProvider>;
  return (
    typeof p.id === "string" &&
    typeof p.capabilities === "object" &&
    typeof p.start === "function" &&
    typeof p.pushAudio === "function" &&
    typeof p.finish === "function" &&
    typeof p.onResult === "function"
  );
}
