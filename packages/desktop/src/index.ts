import { SUPPORTED_API_VERSIONS, readDiscovery, type DiscoveryOptions } from "./discovery.js";
import { ApiError, UnsupportedVersionError } from "./errors.js";
import { errorOf, json, send, type Reply } from "./http.js";

export { ApiError, NotRunningError, UnauthorizedError, UnsupportedVersionError, WordInkDesktopError } from "./errors.js";
export { SUPPORTED_API_VERSIONS, discoveryPath, type DiscoveryOptions } from "./discovery.js";

/** The `/control` protocol version this client sends. */
export const CONTROL_VERSION = 4;

export interface ConnectOptions extends DiscoveryOptions {
  /** Per-request timeout in milliseconds. Default 120000. */
  timeoutMs?: number;
}

export interface Health {
  version: string;
  apiVersion: string;
}

export interface Capabilities {
  audioFormats: string[];
  partialTranscripts: boolean;
  serviceCapture: boolean;
  streams: boolean;
  developerControl: boolean;
  control: boolean;
}

export interface TranscribeOptions {
  /** Content type of the audio. The app takes `audio/wav`. Default `audio/wav`. */
  mime?: string;
  /** A model `id` from the app's `/models`. Default: the first installed model. */
  model?: string;
  /** Language code. Default: the app's default. */
  language?: string;
}

export interface DictationResult {
  text: string;
  durationMs: number;
}

export interface Dictation {
  readonly id: number;
  readonly sampleRate: number;
  /** Stops recording and returns the transcript. */
  finish(): Promise<DictationResult>;
  /** Discards the recording. */
  cancel(): Promise<void>;
  /** Tells the app you are still there. */
  heartbeat(): Promise<void>;
}

export interface DesktopClient {
  readonly url: string;
  readonly apiVersion: string;
  health(): Promise<Health>;
  capabilities(): Promise<Capabilities>;
  /** Transcribes a WAV and returns the text. */
  transcribe(audio: Buffer | Uint8Array, options?: TranscribeOptions): Promise<string>;
  /** Starts a dictation the app records itself (where `capabilities().serviceCapture` is true). */
  dictate(options?: { source?: string }): Promise<Dictation>;
  /** Runs a `hex` control command, for example `control("status")`. Returns the app's JSON reply. */
  control(command: string, args?: Record<string, unknown>): Promise<unknown>;
}

/**
 * Finds the running app, checks its API version and returns a client.
 * Throws NotRunningError, UnsupportedVersionError or UnauthorizedError.
 */
export async function connect(options: ConnectOptions = {}): Promise<DesktopClient> {
  const found = await readDiscovery(options);
  if (!SUPPORTED_API_VERSIONS.includes(found.apiVersion)) {
    throw new UnsupportedVersionError(found.apiVersion, SUPPORTED_API_VERSIONS);
  }
  const timeoutMs = options.timeoutMs ?? 120_000;
  const call = (method: "GET" | "POST", path: string, extra: { headers?: Record<string, string>; body?: Uint8Array } = {}): Promise<Reply> =>
    send(found.url, found.token, { method, path, ...extra }, timeoutMs);
  const postJson = (path: string, value: unknown, headers: Record<string, string> = {}) =>
    call("POST", path, {
      headers: { "Content-Type": "application/json", ...headers },
      body: Buffer.from(JSON.stringify(value)),
    });

  const client: DesktopClient = {
    url: found.url,
    apiVersion: found.apiVersion,
    health: async () => json<Health>(await call("GET", "/health")),
    capabilities: async () => json<Capabilities>(await call("GET", "/capabilities")),

    async transcribe(audio, opts = {}) {
      let model = opts.model;
      if (!model) {
        const models = json<{ id: string; installed: boolean }[]>(await call("GET", "/models"));
        model = models.find((m) => m.installed)?.id;
        if (!model) throw new ApiError(409, "model-not-ready");
      }
      const query = new URLSearchParams({ model });
      if (opts.language) query.set("language", opts.language);
      const reply = await call("POST", `/transcriptions?${query}`, {
        headers: { "Content-Type": opts.mime ?? "audio/wav" },
        body: audio,
      });
      return json<{ transcript: string }>(reply).transcript;
    },

    async dictate(opts = {}) {
      const started = json<{ id: number; ownerToken: string; sampleRate: number }>(
        await postJson("/dictations", { source: opts.source ?? "wordink-desktop-node" }),
      );
      const owner = { "X-Hex-Dictation-Token": started.ownerToken };
      const route = (action: string) => `/dictations/${started.id}/${action}`;
      const expectNoContent = (reply: Reply) => {
        if (reply.status !== 204) throw errorOf(reply);
      };
      return {
        id: started.id,
        sampleRate: started.sampleRate,
        async finish() {
          const done = json<{ transcript: string; durationMs: number }>(
            await call("POST", route("finish"), { headers: owner }),
          );
          return { text: done.transcript, durationMs: done.durationMs };
        },
        async cancel() {
          expectNoContent(await call("POST", route("cancel"), { headers: owner }));
        },
        async heartbeat() {
          expectNoContent(await call("POST", route("heartbeat"), { headers: owner }));
        },
      };
    },

    async control(command, args = {}) {
      return json<unknown>(await postJson("/control", { version: CONTROL_VERSION, ...args, command }));
    },
  };
  // Proves the file is live and the token works before handing the client back.
  const health = await client.health();
  if (!SUPPORTED_API_VERSIONS.includes(health.apiVersion)) {
    throw new UnsupportedVersionError(health.apiVersion, SUPPORTED_API_VERSIONS);
  }
  return client;
}
