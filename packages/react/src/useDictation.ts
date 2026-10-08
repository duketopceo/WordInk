import {
  createDictation,
  type CloudProvider,
  type Dictation,
  type DictationOptions,
  type DictationState,
  type HostProvider,
  type WordInkError,
} from "@wordink/core";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

export interface UseDictationOptions {
  /** `"groq"`, `"openai"`, `"deepgram"`, or a `HostProvider` (such as `@wordink/local`). */
  provider: CloudProvider | HostProvider;
  /** Your `@wordink/server` relay base URL. */
  endpoint?: string | undefined;
  /** Provider key used directly from the page. Localhost only (see `@wordink/core`). */
  devKey?: string | undefined;
  /** `"hold"` (push-to-talk, default) or `"toggle"`; decides what `press` / `release` do. */
  mode?: "hold" | "toggle" | undefined;
  /** Custom vocabulary or prompt hint. */
  hint?: string | undefined;
  /** Provider model override. */
  model?: string | undefined;
  /** Post-processing run on the final text before `onTranscript`. Call your own backend. */
  transform?: ((text: string) => Promise<string> | string) | undefined;
  /** Timeout for `transform`, in ms (default 3000). */
  transformTimeoutMs?: number | undefined;
  /** Called once per dictation with the final text. */
  onTranscript?: ((text: string) => void) | undefined;
  /** Called with a `WordInkError` (`code`, `message`, `hint`). */
  onError?: ((error: WordInkError) => void) | undefined;
}

export interface UseDictationResult {
  state: DictationState;
  /** Input level (RMS, 0 to 1) while listening, otherwise 0. */
  level: number;
  /** Transcript so far while speaking (streaming providers), otherwise `""`. */
  interim: string;
  /** The last error, cleared when a new dictation starts. */
  error: WordInkError | null;
  /** Start listening. Call from a user gesture. */
  start: () => Promise<void>;
  /** Stop listening and transcribe. */
  stop: () => Promise<void>;
  /** Raw button down (hold: starts; toggle: starts or stops). Call from a user gesture. */
  press: () => Promise<void>;
  /** Raw button up (hold: stops; toggle: ignored). */
  release: () => Promise<void>;
}

/**
 * Push-to-talk dictation for React. The underlying dictation is created on the first `start` /
 * `press` (a user gesture), recreated after any config option changes, and destroyed on unmount,
 * which releases the microphone. `onTranscript`, `onError` and `transform` can change on every
 * render without restarting a dictation.
 */
export function useDictation(options: UseDictationOptions): UseDictationResult {
  const [state, setState] = useState<DictationState>("idle");
  const [level, setLevel] = useState(0);
  const [interim, setInterim] = useState("");
  const [error, setError] = useState<WordInkError | null>(null);

  const latest = useRef(options);
  useLayoutEffect(() => {
    latest.current = options;
  });

  const dictation = useRef<Dictation | null>(null);
  const unsubscribe = useRef<(() => void)[]>([]);

  const fail = useCallback((err: WordInkError) => {
    setError(err);
    latest.current.onError?.(err);
  }, []);

  const ensure = useCallback((): Dictation | null => {
    if (dictation.current) return dictation.current;
    const o = latest.current;
    const config: DictationOptions = { provider: o.provider };
    if (o.endpoint !== undefined) config.endpoint = o.endpoint;
    if (o.devKey !== undefined) config.devKey = o.devKey;
    if (o.mode !== undefined) config.mode = o.mode;
    if (o.hint !== undefined) config.hint = o.hint;
    if (o.model !== undefined) config.model = o.model;
    if (o.transformTimeoutMs !== undefined) config.transformTimeoutMs = o.transformTimeoutMs;
    // Read through the ref so a new transform function each render doesn't recreate the dictation.
    if (o.transform) config.transform = (text) => (latest.current.transform ? latest.current.transform(text) : text);
    let d: Dictation;
    try {
      d = createDictation(config);
    } catch (err) {
      setState("error");
      fail(err as WordInkError);
      return null;
    }
    dictation.current = d;
    unsubscribe.current = [
      d.on("state", (s) => {
        setState(s);
        if (s !== "listening") setLevel(0);
        if (s !== "listening" && s !== "transcribing") setInterim("");
        if (s === "requesting-mic" || s === "listening") setError(null);
      }),
      d.on("level", setLevel),
      d.on("interim", setInterim),
      d.on("final", (text) => latest.current.onTranscript?.(text)),
      d.on("error", fail),
    ];
    return d;
  }, [fail]);

  // Tear down on unmount and whenever a config option changes; the next gesture creates a new one.
  const { provider, endpoint, devKey, mode, hint, model, transformTimeoutMs } = options;
  const hasTransform = options.transform !== undefined;
  useEffect(
    () => () => {
      for (const off of unsubscribe.current) off();
      unsubscribe.current = [];
      if (dictation.current) {
        dictation.current.destroy();
        dictation.current = null;
        setState("idle");
        setLevel(0);
        setInterim("");
      }
    },
    [provider, endpoint, devKey, mode, hint, model, transformTimeoutMs, hasTransform],
  );

  const start = useCallback(async () => ensure()?.start(), [ensure]);
  const stop = useCallback(async () => dictation.current?.stop(), []);
  const press = useCallback(async () => ensure()?.press(), [ensure]);
  const release = useCallback(async () => dictation.current?.release(), []);

  return { state, level, interim, error, start, stop, press, release };
}
