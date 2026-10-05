/**
 * The browser host (KTD2): runs the wasm core, performs the effects it emits (microphone, fetch,
 * WebSocket, host providers) and feeds the results back as events.
 */
import init, { WasmSession } from "../wasm/wordink_core.js";
import { type Credentials, mintToken, relayUrl, resolveCredentials } from "./credentials.js";
import { configError, type ErrorCode, sessionError, WordInkError } from "./errors.js";
import { type CloudProvider, type HostProvider, type HostProviderResult, isHostProvider } from "./providers.js";
import { createAudioContext, type Mic, openMic } from "./worklet.js";

/** Visible dictation state (R6). */
export type DictationState = "idle" | "requesting-mic" | "listening" | "transcribing" | "error";

export interface DictationOptions {
  /** `groq` (default cloud provider), `openai`, `deepgram`, or a host-implemented provider. */
  provider: CloudProvider | HostProvider;
  /** Your `@wordink/server` relay base URL, e.g. `/api/wordink` (KTD3). */
  endpoint?: string;
  /** A provider key used directly from the page. Allowed on localhost only, for experiments. */
  devKey?: string;
  /** `hold` (push-to-talk, the default) or `toggle` (tap to start, tap to stop). */
  mode?: "hold" | "toggle";
  /** Custom vocabulary or prompt hint (R15). */
  hint?: string;
  /** Provider model override. */
  model?: string;
  /**
   * Post-processing (R14, KTD11), run on the final text before the `final` event. Call your own
   * backend, never an LLM with a key in the page, and treat the dictated text as untrusted input.
   * On timeout or failure the raw text is used and a `warning` is emitted.
   */
  transform?: (text: string) => Promise<string> | string;
  /** Timeout for `transform`. Default 3000 ms. */
  transformTimeoutMs?: number;
}

export type WarningCode = "TransformTimeout" | "TransformFailed";

/** A non-fatal problem: the dictation still completed. */
export interface WordInkWarning {
  code: WarningCode;
  message: string;
  cause?: unknown;
}

export interface DictationEvents {
  state: DictationState;
  /** Input level while listening, RMS 0..1, about 20 per second. */
  level: number;
  /** Transcript so far (streaming providers). Replaces the previous interim. */
  interim: string;
  /** The finished (and transformed) text. */
  final: string;
  error: WordInkError;
  warning: WordInkWarning;
}

export interface Dictation {
  /** Current state. */
  readonly state: DictationState;
  /** Resolves once the core wasm is loaded. */
  readonly ready: Promise<void>;
  /** Start listening if idle (or after an error). Call from a user gesture. */
  start(): Promise<void>;
  /** Stop listening and transcribe, if listening. */
  stop(): Promise<void>;
  /** Raw button down (hold: starts; toggle: starts or stops). Call from a user gesture. */
  press(): Promise<void>;
  /** Raw button up (hold: stops; toggle: ignored). */
  release(): Promise<void>;
  /** Stops capture, closes connections, cancels work and removes listeners. */
  destroy(): void;
  /** Subscribes to an event; returns an unsubscribe function. */
  on<K extends keyof DictationEvents>(type: K, callback: (value: DictationEvents[K]) => void): () => void;
}

export const DEFAULT_TRANSFORM_TIMEOUT_MS = 3000;
/** Requests with no response by then are reported as failed (the core maps that to ProviderDown). */
export const HTTP_TIMEOUT_MS = 30_000;
/** WebSocket close code for a connection lost without a close frame. */
const WS_ABNORMAL_CLOSE = 1006;
/** A provider socket that has not opened by then is reported as failed to open. */
export const WS_CONNECT_TIMEOUT_MS = 10_000;
/** An utterance still transcribing after this long (a stalled socket or host provider) fails with ProviderDown. */
export const TRANSCRIBE_TIMEOUT_MS = 30_000;

let wasmReady: Promise<unknown> | undefined;

function loadCore(): Promise<unknown> {
  // A failed load is not cached, so a later press retries it.
  wasmReady ??= init().catch((err: unknown) => {
    wasmReady = undefined;
    throw err;
  });
  return wasmReady;
}

function wasmLoadError(cause: unknown): WordInkError {
  return new WordInkError(
    "ProviderDown",
    "The WordInk engine (wasm) failed to load.",
    "Check your connection and that wordink_core_bg.wasm from @wordink/core is served by your bundler or CDN, then try again.",
    { cause },
  );
}

/** Effects as decoded from the core, with binary payloads attached. */
type CoreEffect =
  | { t: "state"; s: DictationState; code?: ErrorCode }
  | { t: "request-mic" }
  | { t: "stop-mic" }
  | { t: "level"; rms: number }
  | { t: "interim"; text: string }
  | { t: "final"; text: string }
  | {
      t: "http";
      id: number;
      method: string;
      url: string;
      headers: [string, string][];
      parts: ({ name: string; text: string } | { name: string; filename: string; type: string; blob: number; data: Uint8Array })[];
    }
  | { t: "ws-open"; id: number; url: string; protocols: string[] }
  | { t: "ws-send"; id: number; text?: string; blob?: number; data?: Uint8Array }
  | { t: "ws-close"; id: number }
  | { t: "hp-start"; id: string; rate: number; hint?: string }
  | { t: "hp-audio"; id: string; pcm: number; samples: Int16Array }
  | { t: "hp-finish"; id: string }
  | { t: "hp-cancel"; id: string };

/** Parses a core call's effects and takes its blobs before the next call clears them. */
function decode(session: WasmSession, json: string): CoreEffect[] {
  const effects = JSON.parse(json) as CoreEffect[];
  for (const e of effects) {
    if (e.t === "http") {
      for (const p of e.parts) if ("blob" in p) p.data = session.take_bytes(p.blob);
    } else if (e.t === "ws-send" && e.blob !== undefined) {
      e.data = session.take_bytes(e.blob);
    } else if (e.t === "hp-audio") {
      e.samples = session.take_pcm(e.pcm);
    }
  }
  return effects;
}

type Listener = (value: never) => void;

const TIMEOUT = Symbol("timeout");

class DictationHost implements Dictation {
  private loaded: Promise<void>;
  private readonly options: DictationOptions;
  private readonly hostProvider: HostProvider | undefined;
  private readonly credentials: Credentials | undefined;
  private readonly listeners = new Map<string, Set<Listener>>();
  private currentState: DictationState = "idle";
  private session: WasmSession | undefined;
  private ops: Promise<void> = Promise.resolve();
  private queue: CoreEffect[] = [];
  private draining = false;
  private paused = false;
  private destroyed = false;
  private pendingContext: AudioContext | undefined;
  private mic: Mic | undefined;
  private micGeneration = 0;
  private micErrorName: string | undefined;
  private readonly sockets = new Map<number, WebSocket>();
  private readonly requests = new Set<AbortController>();
  private hostActive = false;
  private watchdog: ReturnType<typeof setTimeout> | undefined;
  private readonly connectTimers = new Map<WebSocket, ReturnType<typeof setTimeout>>();
  private unsubscribeHost: (() => void) | undefined;

  constructor(options: DictationOptions) {
    this.options = options;
    const { provider, mode, transformTimeoutMs } = options;
    if (mode !== undefined && mode !== "hold" && mode !== "toggle") {
      throw configError(`Unknown mode "${String(mode)}".`, 'Use "hold" or "toggle".');
    }
    if (transformTimeoutMs !== undefined && !(transformTimeoutMs > 0)) {
      throw configError("`transformTimeoutMs` must be a positive number.", "Omit it for the 3000 ms default.");
    }
    if (typeof provider === "string") {
      if (provider !== "groq" && provider !== "openai" && provider !== "deepgram") {
        throw configError(`Unknown provider "${provider}".`, 'Use "groq", "openai", "deepgram" or a HostProvider.');
      }
      this.credentials = resolveCredentials(provider, options, globalThis.location?.hostname);
    } else if (isHostProvider(provider)) {
      const rate = provider.capabilities.sampleRate;
      if (!Number.isInteger(rate) || rate <= 0) {
        throw configError(`Host provider "${provider.id}" declared an invalid sample rate.`, "Declare a positive integer rate, e.g. 16000.");
      }
      this.hostProvider = provider;
      const off = provider.onResult((r) => this.onHostResult(r));
      if (typeof off === "function") this.unsubscribeHost = off;
    } else {
      throw configError("`provider` is missing or invalid.", 'Use "groq", "openai", "deepgram" or a HostProvider.');
    }
    this.loaded = loadCore().then(() => undefined);
    // Surface load failures through press()/start(), not as an unhandled rejection.
    this.loaded.catch(() => {});
  }

  get ready(): Promise<void> {
    return this.loaded;
  }

  get state(): DictationState {
    return this.currentState;
  }

  on<K extends keyof DictationEvents>(type: K, callback: (value: DictationEvents[K]) => void): () => void {
    let set = this.listeners.get(type);
    if (!set) this.listeners.set(type, (set = new Set()));
    set.add(callback as Listener);
    return () => set.delete(callback as Listener);
  }

  press(): Promise<void> {
    this.prepareContext();
    return this.enqueue(() => this.doPress());
  }

  release(): Promise<void> {
    return this.enqueue(async () => {
      if (this.session) this.feed(this.session, (s) => s.release());
    });
  }

  start(): Promise<void> {
    this.prepareContext();
    return this.enqueue(async () => {
      if (this.isStartable()) await this.doPress();
    });
  }

  stop(): Promise<void> {
    return this.enqueue(async () => {
      const s = this.session;
      const state = s?.state();
      if (!s || (state !== "requesting-mic" && state !== "listening")) return;
      this.feed(s, (x) => (this.options.mode === "toggle" ? x.press() : x.release()));
    });
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.clearWatchdog();
    this.closeMic();
    for (const ws of this.sockets.values()) this.closeSocket(ws);
    this.sockets.clear();
    for (const c of this.requests) c.abort();
    this.requests.clear();
    if (this.hostActive) void this.callHost(() => this.hostProvider?.cancel?.());
    this.hostActive = false;
    this.unsubscribeHost?.();
    this.session?.free();
    this.session = undefined;
    this.queue = [];
    this.listeners.clear();
  }

  // --- input -----------------------------------------------------------------------------------

  private isStartable(): boolean {
    // While a transform is pending the core is already idle but the UI still shows transcribing.
    if (this.paused) return false;
    const state = this.session?.state() ?? "idle";
    return state === "idle" || state === "error";
  }

  /** Inside the user gesture: create the AudioContext now so Safari lets it run. */
  private prepareContext(): void {
    if (this.destroyed || this.pendingContext || this.mic || !this.isStartable()) return;
    if (typeof AudioContext === "undefined") return;
    this.pendingContext = createAudioContext();
  }

  private enqueue(op: () => Promise<void>): Promise<void> {
    const next = this.ops.then(() => (this.destroyed ? undefined : op()));
    this.ops = next.catch(() => {});
    return next;
  }

  private async doPress(): Promise<void> {
    try {
      await this.load();
    } catch (err) {
      if (this.destroyed) return;
      this.dropContext();
      this.setState("error");
      this.emit("error", wasmLoadError(err));
      return;
    }
    if (this.destroyed || this.paused) return;
    const starting = this.isStartable();
    if (!this.session || (starting && this.needsToken())) {
      let credential: string;
      try {
        credential = await this.sessionCredential();
      } catch (err) {
        if (this.destroyed) return;
        this.dropContext();
        this.setState("error");
        this.emit("error", err instanceof WordInkError ? err : sessionError("ProviderDown"));
        return;
      }
      if (this.destroyed) return;
      this.replaceSession(credential);
    }
    if (this.session) this.feed(this.session, (s) => s.press());
  }

  /** The core load, retried if an earlier attempt failed. */
  private load(): Promise<void> {
    const next = this.loaded.catch(() => loadCore().then(() => undefined));
    next.catch(() => {});
    this.loaded = next;
    return next;
  }

  /** OpenAI and Deepgram through a relay need a fresh short-lived token per session. */
  private needsToken(): boolean {
    return this.credentials?.kind === "relay" && this.options.provider !== "groq";
  }

  private async sessionCredential(): Promise<string> {
    const { provider } = this.options;
    if (this.hostProvider) return this.hostProvider.id;
    const creds = this.credentials!;
    if (creds.kind === "devKey") return creds.key;
    if (provider === "groq") return relayUrl(creds.endpoint, "");
    this.setState("requesting-mic");
    return this.withRequestAbort((signal) =>
      mintToken(provider as "openai" | "deepgram", creds.endpoint, fetch, signal),
    );
  }

  /** Runs a request that aborts after `HTTP_TIMEOUT_MS` or on `destroy()`. */
  private async withRequestAbort<T>(run: (signal: AbortSignal) => Promise<T>): Promise<T> {
    const controller = new AbortController();
    this.requests.add(controller);
    const timer = setTimeout(() => controller.abort(), HTTP_TIMEOUT_MS);
    try {
      return await run(controller.signal);
    } finally {
      clearTimeout(timer);
      this.requests.delete(controller);
    }
  }

  private replaceSession(credential: string): void {
    for (const ws of this.sockets.values()) this.closeSocket(ws);
    this.sockets.clear();
    this.session?.free();
    const { provider, mode, model, hint } = this.options;
    const kind = this.hostProvider
      ? "host"
      : provider === "groq"
        ? this.credentials?.kind === "relay"
          ? "groq-relay"
          : "groq-key"
        : (provider as string);
    const caps = this.hostProvider?.capabilities;
    this.session = new WasmSession(
      kind,
      mode === "toggle",
      credential,
      model ?? "",
      "",
      hint ?? "",
      caps?.streaming ?? false,
      caps?.sampleRate ?? 0,
    );
  }

  // --- effect loop -----------------------------------------------------------------------------

  /** Calls into the core and performs the resulting effects. Results for a replaced session are dropped. */
  private feed(session: WasmSession, call: (s: WasmSession) => string): void {
    if (this.destroyed || session !== this.session) return;
    this.queue.push(...decode(session, call(session)));
    this.drain();
  }

  private drain(): void {
    if (this.draining) return;
    this.draining = true;
    try {
      while (!this.paused && !this.destroyed && this.queue.length > 0) {
        const pending = this.perform(this.queue.shift()!);
        if (pending) {
          // Hold later effects (the move to idle) until the transform settles.
          this.paused = true;
          void pending.finally(() => {
            this.paused = false;
            this.drain();
          });
        }
      }
    } finally {
      this.draining = false;
    }
  }

  private perform(e: CoreEffect): Promise<void> | void {
    const session = this.session!;
    switch (e.t) {
      case "state":
        // Arm on entry only, so a repeated transcribing effect can't slide the deadline.
        if (e.s !== "transcribing") this.clearWatchdog();
        else if (this.currentState !== "transcribing") this.startWatchdog(session);
        this.setState(e.s);
        if (e.s === "error") this.emit("error", sessionError(e.code ?? "ProviderDown", this.micErrorName));
        if (e.s !== "listening" && e.s !== "transcribing") this.hostActive = false;
        return;
      case "request-mic":
        void this.openMic(session);
        return;
      case "stop-mic":
        this.closeMic();
        return;
      case "level":
        this.emit("level", e.rms / 1_000_000);
        return;
      case "interim":
        this.emit("interim", e.text);
        return;
      case "final":
        return this.options.transform ? this.finalize(e.text) : this.emit("final", e.text);
      case "http":
        this.http(session, e);
        return;
      case "ws-open":
        this.openSocket(session, e.id, e.url, e.protocols);
        return;
      case "ws-send": {
        const ws = this.sockets.get(e.id);
        if (ws?.readyState === WebSocket.OPEN) ws.send(e.data ?? e.text ?? "");
        return;
      }
      case "ws-close": {
        const ws = this.sockets.get(e.id);
        this.sockets.delete(e.id);
        if (ws) this.closeSocket(ws);
        return;
      }
      case "hp-start": {
        this.hostActive = true;
        void this.callHost(() => this.hostProvider?.start(e.rate, e.hint), session);
        return;
      }
      case "hp-audio":
        void this.callHost(() => this.hostProvider?.pushAudio(e.samples), session);
        return;
      case "hp-finish":
        void this.callHost(() => this.hostProvider?.finish(), session);
        return;
      case "hp-cancel":
        this.hostActive = false;
        void this.callHost(() => this.hostProvider?.cancel?.());
        return;
    }
  }

  /** Fails an utterance whose provider never answers, so the user can retry (the core has no clock). */
  private startWatchdog(session: WasmSession): void {
    this.clearWatchdog();
    this.watchdog = setTimeout(() => {
      this.watchdog = undefined;
      if (this.destroyed || session !== this.session || session.state() !== "transcribing") return;
      for (const [id, ws] of [...this.sockets]) this.failSocket(session, id, ws);
      if (this.hostProvider && session.state() === "transcribing") {
        this.hostActive = false;
        void this.callHost(() => this.hostProvider?.cancel?.());
        this.feed(session, (s) => s.host_error("ProviderDown"));
      }
    }, this.hostProvider?.capabilities.timeoutMs ?? TRANSCRIBE_TIMEOUT_MS);
  }

  private clearWatchdog(): void {
    clearTimeout(this.watchdog);
    this.watchdog = undefined;
  }

  private setState(state: DictationState): void {
    if (state === this.currentState) return;
    this.currentState = state;
    this.emit("state", state);
  }

  private emit<K extends keyof DictationEvents>(type: K, value: DictationEvents[K]): void {
    for (const cb of this.listeners.get(type) ?? []) {
      try {
        (cb as (v: DictationEvents[K]) => void)(value);
      } catch (err) {
        console.error(`[@wordink/core] "${type}" listener threw:`, err);
      }
    }
  }

  private async finalize(raw: string): Promise<void> {
    const timeoutMs = this.options.transformTimeoutMs ?? DEFAULT_TRANSFORM_TIMEOUT_MS;
    let text = raw;
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      const timeout = new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(TIMEOUT), timeoutMs);
      });
      const out = await Promise.race([Promise.resolve().then(() => this.options.transform!(raw)), timeout]);
      if (typeof out !== "string") throw new TypeError("transform must return a string");
      text = out;
    } catch (cause) {
      const timedOut = cause === TIMEOUT;
      this.emit("warning", {
        code: timedOut ? "TransformTimeout" : "TransformFailed",
        message: timedOut
          ? `transform took longer than ${timeoutMs} ms; inserted the raw transcript.`
          : "transform failed; inserted the raw transcript.",
        ...(timedOut ? {} : { cause }),
      });
    } finally {
      clearTimeout(timer);
    }
    if (!this.destroyed) this.emit("final", text);
  }

  // --- microphone ------------------------------------------------------------------------------

  private async openMic(session: WasmSession): Promise<void> {
    const generation = ++this.micGeneration;
    const ctx = this.pendingContext ?? createAudioContext();
    this.pendingContext = undefined;
    this.micErrorName = undefined;
    try {
      const mic = await openMic(ctx, (samples) => this.feed(session, (s) => s.push_audio(samples)));
      if (generation !== this.micGeneration || this.destroyed) {
        mic.close();
        return;
      }
      this.mic = mic;
      this.feed(session, (s) => s.mic_granted(mic.sampleRate));
    } catch (err) {
      if (generation !== this.micGeneration) return;
      this.micErrorName = err instanceof Error ? err.name : undefined;
      this.feed(session, (s) => s.mic_denied());
    }
  }

  private closeMic(): void {
    this.micGeneration++;
    this.mic?.close();
    this.mic = undefined;
    this.dropContext();
  }

  private dropContext(): void {
    void this.pendingContext?.close().catch(() => {});
    this.pendingContext = undefined;
  }

  // --- network ---------------------------------------------------------------------------------

  private http(session: WasmSession, e: Extract<CoreEffect, { t: "http" }>): void {
    const form = new FormData();
    for (const p of e.parts) {
      if ("data" in p) form.append(p.name, new Blob([p.data as Uint8Array<ArrayBuffer>], { type: p.type }), p.filename);
      else form.append(p.name, p.text);
    }
    this.withRequestAbort(async (signal) => {
      const r = await fetch(e.url, {
        method: e.method,
        headers: e.headers,
        body: form,
        signal,
        // Relays authenticate with the app's own session (cookies); a direct dev-key call sends none.
        credentials: this.credentials?.kind === "relay" ? "include" : "omit",
      });
      return { status: r.status, body: await r.text() };
    })
      .then(({ status, body }) => this.feed(session, (s) => s.http_response(e.id, status, body)))
      .catch(() => this.feed(session, (s) => s.http_failed(e.id)));
  }

  /** Closes a socket and reports it to the core as failed (the code browsers give a lost socket). */
  private failSocket(session: WasmSession, id: number, ws: WebSocket): void {
    this.sockets.delete(id);
    this.closeSocket(ws);
    this.feed(session, (s) => s.ws_closed(id, WS_ABNORMAL_CLOSE));
  }

  private openSocket(session: WasmSession, id: number, url: string, protocols: string[]): void {
    let ws: WebSocket;
    try {
      ws = new WebSocket(url, protocols);
    } catch {
      // An invalid URL or subprotocol: report it like a socket that failed to open.
      queueMicrotask(() => this.feed(session, (s) => s.ws_closed(id, WS_ABNORMAL_CLOSE)));
      return;
    }
    ws.binaryType = "arraybuffer";
    this.sockets.set(id, ws);
    this.connectTimers.set(
      ws,
      setTimeout(() => {
        // Never opened: report it like a socket that failed to open.
        if (this.sockets.get(id) === ws) this.failSocket(session, id, ws);
      }, WS_CONNECT_TIMEOUT_MS),
    );
    ws.onopen = () => {
      this.clearConnectTimer(ws);
      this.feed(session, (s) => s.ws_opened(id));
    };
    ws.onmessage = (m: MessageEvent<string | ArrayBuffer>) => {
      const data = m.data;
      if (typeof data === "string") this.feed(session, (s) => s.ws_text(id, data));
      else this.feed(session, (s) => s.ws_binary(id, new Uint8Array(data)));
    };
    ws.onclose = (c) => {
      this.clearConnectTimer(ws);
      if (this.sockets.get(id) === ws) this.sockets.delete(id);
      this.feed(session, (s) => s.ws_closed(id, c.code));
    };
  }

  private clearConnectTimer(ws: WebSocket): void {
    clearTimeout(this.connectTimers.get(ws));
    this.connectTimers.delete(ws);
  }

  private closeSocket(ws: WebSocket): void {
    this.clearConnectTimer(ws);
    ws.onopen = ws.onmessage = ws.onclose = null;
    if (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN) ws.close(1000);
  }

  // --- host providers --------------------------------------------------------------------------

  /** Runs a host provider method; a throw or rejection fails the utterance with ProviderDown. */
  private async callHost(fn: () => void | Promise<void>, session?: WasmSession): Promise<void> {
    try {
      await fn();
    } catch (err) {
      console.error("[@wordink/core] host provider failed:", err);
      if (session) this.feed(session, (s) => s.host_error("ProviderDown"));
    }
  }

  private onHostResult(result: HostProviderResult): void {
    const s = this.session;
    if (!s) return;
    if (result.type === "interim") this.feed(s, (x) => x.host_interim(result.text));
    else if (result.type === "final") this.feed(s, (x) => x.host_final(result.text));
    else this.feed(s, (x) => x.host_error(result.code));
  }
}

/**
 * Creates a dictation controller. Throws a `WordInkError` with code `Config` for invalid options,
 * synchronously and before any network call (for example a `devKey` off localhost).
 */
export function createDictation(options: DictationOptions): Dictation {
  return new DictationHost(options);
}
