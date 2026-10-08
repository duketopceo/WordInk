import type { Dictation, DictationEvents, DictationOptions, DictationState } from "@wordink/core";
import { act, cleanup, render, renderHook } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/** A scripted stand-in for @wordink/core's Dictation: tests emit its events by hand. */
class FakeDictation implements Dictation {
  state: DictationState = "idle";
  readonly ready = Promise.resolve();
  readonly options: DictationOptions;
  readonly start = vi.fn(async () => {});
  readonly stop = vi.fn(async () => {});
  readonly press = vi.fn(async () => {});
  readonly release = vi.fn(async () => {});
  /** Stands in for the core's mic track; the real destroy() stops it. */
  micTrackLive = false;
  readonly destroy = vi.fn(() => {
    this.micTrackLive = false;
    this.listeners.clear();
  });
  private readonly listeners = new Map<string, Set<(v: never) => void>>();

  constructor(options: DictationOptions) {
    this.options = options;
  }

  on<K extends keyof DictationEvents>(type: K, cb: (value: DictationEvents[K]) => void): () => void {
    let set = this.listeners.get(type);
    if (!set) this.listeners.set(type, (set = new Set()));
    set.add(cb as (v: never) => void);
    return () => set.delete(cb as (v: never) => void);
  }

  emit<K extends keyof DictationEvents>(type: K, value: DictationEvents[K]): void {
    if (type === "state") {
      this.state = value as DictationState;
      if (value === "listening") this.micTrackLive = true;
    }
    for (const cb of this.listeners.get(type) ?? []) (cb as (v: DictationEvents[K]) => void)(value);
  }
}

const created: FakeDictation[] = [];
const createDictation = vi.fn((options: DictationOptions): Dictation => {
  const d = new FakeDictation(options);
  created.push(d);
  return d;
});

vi.mock("@wordink/core", () => ({ createDictation: (o: DictationOptions) => createDictation(o) }));

const { useDictation, WordInkMic } = await import("../src/index.js");

const last = () => created[created.length - 1]!;

beforeEach(() => {
  created.length = 0;
  createDictation.mockClear();
});

afterEach(() => cleanup());

describe("useDictation", () => {
  it("moves state through listening, transcribing and idle and calls onTranscript once with the final text", async () => {
    const onTranscript = vi.fn();
    const { result } = renderHook(() => useDictation({ provider: "groq", endpoint: "/api/wordink", onTranscript }));

    expect(result.current.state).toBe("idle");
    expect(createDictation).not.toHaveBeenCalled(); // lazy: created on the first gesture

    await act(() => result.current.press());
    expect(createDictation).toHaveBeenCalledTimes(1);
    expect(last().options).toMatchObject({ provider: "groq", endpoint: "/api/wordink" });
    expect(last().press).toHaveBeenCalledTimes(1);

    act(() => last().emit("state", "listening"));
    expect(result.current.state).toBe("listening");
    act(() => last().emit("level", 0.42));
    act(() => last().emit("interim", "hello wor"));
    expect(result.current.level).toBe(0.42);
    expect(result.current.interim).toBe("hello wor");

    await act(() => result.current.release());
    expect(last().release).toHaveBeenCalledTimes(1);
    act(() => last().emit("state", "transcribing"));
    expect(result.current.state).toBe("transcribing");

    act(() => last().emit("final", "hello world"));
    act(() => last().emit("state", "idle"));
    expect(result.current.state).toBe("idle");
    expect(result.current.interim).toBe("");
    expect(result.current.level).toBe(0);
    expect(onTranscript).toHaveBeenCalledTimes(1);
    expect(onTranscript).toHaveBeenCalledWith("hello world");
    expect(createDictation).toHaveBeenCalledTimes(1);
  });

  it("stops capture and releases the mic track when unmounted during listening", async () => {
    const { result, unmount } = renderHook(() => useDictation({ provider: "groq" }));
    await act(() => result.current.start());
    act(() => last().emit("state", "listening"));
    expect(last().micTrackLive).toBe(true);

    unmount();
    expect(last().destroy).toHaveBeenCalledTimes(1);
    expect(last().micTrackLive).toBe(false);
  });

  it("does not recreate the dictation when onTranscript changes between renders", async () => {
    const first = vi.fn();
    const second = vi.fn();
    const { result, rerender } = renderHook(({ cb }) => useDictation({ provider: "groq", onTranscript: cb }), {
      initialProps: { cb: first },
    });
    await act(() => result.current.press());
    rerender({ cb: second });
    await act(() => result.current.press());

    expect(createDictation).toHaveBeenCalledTimes(1);
    expect(last().destroy).not.toHaveBeenCalled();
    act(() => last().emit("final", "text"));
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith("text");
  });

  it("recreates the dictation on the next gesture after an option changes", async () => {
    const { result, rerender } = renderHook(({ mode }) => useDictation({ provider: "groq", mode }), {
      initialProps: { mode: "hold" as "hold" | "toggle" },
    });
    await act(() => result.current.press());
    const before = last();
    rerender({ mode: "toggle" });
    expect(before.destroy).toHaveBeenCalledTimes(1);

    await act(() => result.current.press());
    expect(createDictation).toHaveBeenCalledTimes(2);
    expect(last().options.mode).toBe("toggle");
  });

  it("reports errors through state, error and onError", async () => {
    const onError = vi.fn();
    const { result } = renderHook(() => useDictation({ provider: "groq", onError }));
    await act(() => result.current.start());
    const err = Object.assign(new Error("Microphone blocked"), { code: "MicDenied", hint: "Allow it" });
    act(() => {
      last().emit("error", err as DictationEvents["error"]);
      last().emit("state", "error");
    });
    expect(result.current.state).toBe("error");
    expect(result.current.error).toBe(err);
    expect(onError).toHaveBeenCalledWith(err);
  });
});

describe("WordInkMic", () => {
  it("forwards onTranscript from the element's wordink-transcript event", () => {
    const onTranscript = vi.fn();
    const ref = createRef<HTMLElementTagNameMap["wordink-mic"]>();
    render(<WordInkMic ref={ref} htmlFor="message" endpoint="/api/wordink" onTranscript={onTranscript} />);

    const el = ref.current!;
    expect(el.tagName).toBe("WORDINK-MIC");
    expect(el.getAttribute("for")).toBe("message");
    expect(el.getAttribute("endpoint")).toBe("/api/wordink");

    el.dispatchEvent(
      new CustomEvent("wordink-transcript", { detail: { text: "hello world" }, bubbles: true, composed: true, cancelable: true }),
    );
    expect(onTranscript).toHaveBeenCalledTimes(1);
    expect(onTranscript.mock.calls[0]![0]).toBe("hello world");
  });

  it("sets complex config as element properties", () => {
    const ref = createRef<HTMLElementTagNameMap["wordink-mic"]>();
    const transform = (t: string) => t.toUpperCase();
    render(<WordInkMic ref={ref} htmlFor="m" devKey="gsk_test" hint="WordInk" transform={transform} />);
    const el = ref.current!;
    expect(el.devKey).toBe("gsk_test");
    expect(el.hint).toBe("WordInk");
    expect(el.getAttribute("devKey")).toBeNull();
    expect(typeof el.transform).toBe("function");
  });
});
