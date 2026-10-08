import type { CloudProvider, DictationOptions, HostProvider, WordInkError } from "@wordink/core";
// Registers <wordink-mic> in browsers; a no-op where there is no DOM (server rendering).
import "@wordink/web";
import type {
  WordInkMic as WordInkMicElement,
  WordInkErrorDetail,
  WordInkInterimDetail,
  WordInkTranscriptDetail,
} from "@wordink/web";
import {
  type DetailedHTMLProps,
  type HTMLAttributes,
  type ReactNode,
  type Ref,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useRef,
} from "react";

export type { WordInkMicElement };

/** Attributes `<wordink-mic>` accepts in JSX. Complex config (keys, functions, objects) is set as properties by `WordInkMic`. */
export interface WordInkMicIntrinsicProps extends DetailedHTMLProps<HTMLAttributes<WordInkMicElement>, WordInkMicElement> {
  for?: string | undefined;
  mode?: "hold" | "toggle" | undefined;
  provider?: CloudProvider | undefined;
  endpoint?: string | undefined;
  shortcut?: string | undefined;
}

declare module "react" {
  namespace JSX {
    interface IntrinsicElements {
      "wordink-mic": WordInkMicIntrinsicProps;
    }
  }
}

type ElementProps = Omit<HTMLAttributes<WordInkMicElement>, "onError" | "onTranscript" | "children">;

export interface WordInkMicProps extends ElementProps {
  /** id of the field to dictate into (the `for` attribute). */
  htmlFor?: string | undefined;
  /** `hold` (push-to-talk, default) or `toggle`. */
  mode?: "hold" | "toggle" | undefined;
  /** `groq` (default), `openai`, `deepgram`, or a `HostProvider` object. */
  provider?: CloudProvider | HostProvider | undefined;
  /** Your `@wordink/server` relay base URL. */
  endpoint?: string | undefined;
  /** Keyboard shortcut held anywhere on the page, e.g. `Alt+D`. */
  shortcut?: string | undefined;
  /** Provider key used directly from the page. Localhost only. */
  devKey?: string | undefined;
  hint?: string | undefined;
  model?: string | undefined;
  transform?: DictationOptions["transform"] | undefined;
  transformTimeoutMs?: number | undefined;
  /** Final text. Call `event.preventDefault()` to insert it yourself instead of the element. */
  onTranscript?: ((text: string, event: CustomEvent<WordInkTranscriptDetail>) => void) | undefined;
  onInterim?: ((text: string, event: CustomEvent<WordInkInterimDetail>) => void) | undefined;
  onError?: ((error: WordInkError, event: CustomEvent<WordInkErrorDetail>) => void) | undefined;
  onStart?: ((event: CustomEvent<Record<string, never>>) => void) | undefined;
  /** A replacement button (slotted into the element). */
  children?: ReactNode;
  ref?: Ref<WordInkMicElement> | undefined;
}

/** A typed React wrapper for the `<wordink-mic>` element. `ref` gives the element itself. */
export function WordInkMic(props: WordInkMicProps) {
  const {
    htmlFor,
    mode,
    provider,
    endpoint,
    shortcut,
    devKey,
    hint,
    model,
    transform,
    transformTimeoutMs,
    onTranscript,
    onInterim,
    onError,
    onStart,
    children,
    ref,
    ...rest
  } = props;
  const el = useRef<WordInkMicElement>(null);
  useImperativeHandle(ref, () => el.current!, []);

  const latest = useRef(props);
  useLayoutEffect(() => {
    latest.current = props;
  });

  // React 19 only maps `on<name>` props to events named exactly `<name>`, so the hyphenated
  // wordink-* events are attached here, reading the latest handlers through a ref.
  useEffect(() => {
    const node = el.current;
    if (!node) return;
    const transcript = (e: CustomEvent<WordInkTranscriptDetail>) => latest.current.onTranscript?.(e.detail.text, e);
    const interim = (e: CustomEvent<WordInkInterimDetail>) => latest.current.onInterim?.(e.detail.text, e);
    const error = (e: CustomEvent<WordInkErrorDetail>) => latest.current.onError?.(e.detail.error, e);
    const start = (e: CustomEvent<Record<string, never>>) => latest.current.onStart?.(e);
    node.addEventListener("wordink-transcript", transcript);
    node.addEventListener("wordink-interim", interim);
    node.addEventListener("wordink-error", error);
    node.addEventListener("wordink-start", start);
    return () => {
      node.removeEventListener("wordink-transcript", transcript);
      node.removeEventListener("wordink-interim", interim);
      node.removeEventListener("wordink-error", error);
      node.removeEventListener("wordink-start", start);
    };
  }, []);

  // A stable wrapper, so a new transform function each render doesn't reset the element.
  const stableTransform = useCallback((text: string) => {
    const fn = latest.current.transform;
    return fn ? fn(text) : text;
  }, []);
  const hasTransform = transform !== undefined;

  // Property-only config. Each setter resets the element's dictation, so set only on change.
  useLayoutEffect(() => {
    const node = el.current;
    if (!node) return;
    if (typeof provider === "object") node.provider = provider;
    else if (typeof node.provider === "object") node.provider = provider ?? "groq";
  }, [provider]);
  useLayoutEffect(() => {
    if (el.current) el.current.devKey = devKey;
  }, [devKey]);
  useLayoutEffect(() => {
    if (el.current) el.current.hint = hint;
  }, [hint]);
  useLayoutEffect(() => {
    if (el.current) el.current.model = model;
  }, [model]);
  useLayoutEffect(() => {
    if (el.current) el.current.transformTimeoutMs = transformTimeoutMs;
  }, [transformTimeoutMs]);
  useLayoutEffect(() => {
    if (el.current) el.current.transform = hasTransform ? stableTransform : undefined;
  }, [hasTransform, stableTransform]);

  return (
    <wordink-mic
      {...rest}
      ref={el}
      for={htmlFor}
      mode={mode}
      provider={typeof provider === "string" ? provider : undefined}
      endpoint={endpoint}
      shortcut={shortcut}
    >
      {children}
    </wordink-mic>
  );
}
