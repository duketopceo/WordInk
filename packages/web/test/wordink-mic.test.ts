import type { Dictation, DictationEvents, DictationOptions, DictationState } from "@wordink/core";
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
  readonly destroy = vi.fn();
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
    if (type === "state") this.state = value as DictationState;
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

const { WordInkMic } = await import("../src/index.js");
type WordInkMic = InstanceType<typeof WordInkMic>;

function wordinkError(code: string, message: string, hint: string) {
  return Object.assign(new Error(message), { name: "WordInkError", code, hint }) as DictationEvents["error"];
}

function mount(html: string): WordInkMic {
  document.body.innerHTML = html;
  return document.querySelector("wordink-mic") as WordInkMic;
}

const shadow = (mic: Element) => mic.shadowRoot!;
const button = (mic: Element) => shadow(mic).querySelector('[part="button"]') as HTMLButtonElement;
const status = (mic: Element) => shadow(mic).querySelector('[part="status"]') as HTMLElement;
const meter = (mic: Element) => shadow(mic).querySelector('[part="meter"]') as HTMLElement;
const last = () => created[created.length - 1]!;

function pointer(el: Element, type: "pointerdown" | "pointerup" | "pointercancel") {
  el.dispatchEvent(new PointerEvent(type, { bubbles: true, composed: true, button: 0, pointerId: 1 }));
}

function key(el: EventTarget, type: "keydown" | "keyup", init: KeyboardEventInit) {
  const e = new KeyboardEvent(type, { bubbles: true, composed: true, cancelable: true, ...init });
  el.dispatchEvent(e);
  return e;
}

beforeEach(() => {
  created.length = 0;
  createDictation.mockClear();
  document.body.innerHTML = "";
  Object.defineProperty(document, "execCommand", { value: vi.fn(() => false), configurable: true, writable: true });
});

afterEach(() => {
  document.body.innerHTML = "";
});

describe("<wordink-mic> rendering", () => {
  it("is defined and renders an accessible button, a meter and a live status region", () => {
    expect(customElements.get("wordink-mic")).toBe(WordInkMic);
    const mic = mount(`<wordink-mic for="msg"></wordink-mic>`);
    expect(mic.getAttribute("data-state")).toBe("idle");
    const b = button(mic);
    expect(b.tagName).toBe("BUTTON");
    expect(b.type).toBe("button");
    expect(b.getAttribute("aria-pressed")).toBe("false");
    expect(b.getAttribute("aria-label")).toBe("Hold to dictate");
    expect(status(mic).getAttribute("aria-live")).toBe("polite");
    expect(status(mic).getAttribute("role")).toBe("status");
    expect(meter(mic)).not.toBeNull();
    expect(shadow(mic).querySelector("slot")).not.toBeNull();
  });

  it("labels the button for toggle mode", () => {
    const mic = mount(`<wordink-mic mode="toggle"></wordink-mic>`);
    expect(button(mic).getAttribute("aria-label")).toBe("Dictate");
  });

  it("creates no dictation until the first press (it must start in a user gesture)", () => {
    mount(`<wordink-mic></wordink-mic>`);
    expect(createDictation).not.toHaveBeenCalled();
  });
});

describe("configuration", () => {
  it("passes attributes and properties to createDictation", () => {
    const mic = mount(`<wordink-mic mode="toggle" provider="openai" endpoint="/api/wordink"></wordink-mic>`);
    const transform = async (t: string) => t.toUpperCase();
    mic.hint = "WordInk, Groq";
    mic.model = "gpt-4o-transcribe";
    mic.transform = transform;
    mic.transformTimeoutMs = 500;
    button(mic).click();
    expect(createDictation).toHaveBeenCalledTimes(1);
    expect(last().options).toEqual({
      provider: "openai",
      endpoint: "/api/wordink",
      mode: "toggle",
      hint: "WordInk, Groq",
      model: "gpt-4o-transcribe",
      transform,
      transformTimeoutMs: 500,
    });
  });

  it("defaults to the groq provider in hold mode, and takes a devKey or a HostProvider by property", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    mic.devKey = "gsk_dev";
    pointer(button(mic), "pointerdown");
    expect(last().options).toEqual({ provider: "groq", mode: "hold", devKey: "gsk_dev" });

    const host = { id: "local", capabilities: { streaming: false, sampleRate: 16000 } } as never;
    mic.provider = host;
    pointer(button(mic), "pointerup");
    pointer(button(mic), "pointerdown");
    expect(last().options.provider).toBe(host);
  });

  it("recreates the dictation after a config change, destroying the old one", () => {
    const mic = mount(`<wordink-mic endpoint="/a"></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    pointer(button(mic), "pointerup");
    const first = last();
    mic.setAttribute("endpoint", "/b");
    expect(first.destroy).toHaveBeenCalled();
    pointer(button(mic), "pointerdown");
    expect(created).toHaveLength(2);
    expect(last().options.endpoint).toBe("/b");
  });

  it("honors properties set before the element was upgraded", () => {
    document.body.innerHTML = `<wordink-mic-late></wordink-mic-late>`;
    const el = document.querySelector("wordink-mic-late") as unknown as WordInkMic;
    (el as { devKey?: string }).devKey = "early";
    customElements.define("wordink-mic-late", class extends WordInkMic {});
    pointer(button(el), "pointerdown");
    expect(last().options.devKey).toBe("early");
  });

  it("destroys the dictation when removed from the page", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    mic.remove();
    expect(last().destroy).toHaveBeenCalled();
  });
});

describe("input modes (R5)", () => {
  it("hold: pointerdown presses and pointerup releases; clicks are ignored", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    expect(last().press).toHaveBeenCalledTimes(1);
    expect(last().release).not.toHaveBeenCalled();
    pointer(button(mic), "pointerup");
    expect(last().release).toHaveBeenCalledTimes(1);
    button(mic).click();
    expect(last().press).toHaveBeenCalledTimes(1);
  });

  it("hold: pointercancel releases too", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    pointer(button(mic), "pointercancel");
    expect(last().release).toHaveBeenCalledTimes(1);
  });

  it("hold: Space or Enter held on the button presses, keyup releases, auto-repeat is ignored", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    const down = key(button(mic), "keydown", { key: " ", code: "Space" });
    expect(down.defaultPrevented).toBe(true);
    key(button(mic), "keydown", { key: " ", code: "Space", repeat: true });
    expect(last().press).toHaveBeenCalledTimes(1);
    key(button(mic), "keyup", { key: " ", code: "Space" });
    expect(last().release).toHaveBeenCalledTimes(1);
    key(button(mic), "keydown", { key: "Enter", code: "Enter" });
    expect(last().press).toHaveBeenCalledTimes(2);
  });

  it("hold: window blur releases a shortcut hold, and a new hold works afterwards", () => {
    mount(`<wordink-mic shortcut="Alt+D"></wordink-mic>`);
    key(document.body, "keydown", { key: "∂", code: "KeyD", altKey: true });
    expect(last().press).toHaveBeenCalledTimes(1);
    window.dispatchEvent(new Event("blur"));
    expect(last().release).toHaveBeenCalledTimes(1);
    key(document.body, "keydown", { key: "∂", code: "KeyD", altKey: true });
    expect(last().press).toHaveBeenCalledTimes(2);
  });

  it("hold: window blur releases a pointer hold exactly once", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    window.dispatchEvent(new Event("blur"));
    window.dispatchEvent(new Event("blur"));
    expect(last().release).toHaveBeenCalledTimes(1);
    pointer(button(mic), "pointerdown");
    expect(last().press).toHaveBeenCalledTimes(2);
  });

  it("hold: a hidden tab releases, a visible visibilitychange does not", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    document.dispatchEvent(new Event("visibilitychange"));
    expect(last().release).not.toHaveBeenCalled();
    hidden.mockReturnValue(true);
    document.dispatchEvent(new Event("visibilitychange"));
    expect(last().release).toHaveBeenCalledTimes(1);
    hidden.mockRestore();
  });

  it("hold: lostpointercapture releases", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    mic.dispatchEvent(new Event("lostpointercapture"));
    expect(last().release).toHaveBeenCalledTimes(1);
  });

  it("toggle: window blur and hidden tab do not release", () => {
    const mic = mount(`<wordink-mic mode="toggle"></wordink-mic>`);
    button(mic).click();
    window.dispatchEvent(new Event("blur"));
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    document.dispatchEvent(new Event("visibilitychange"));
    hidden.mockRestore();
    expect(last().release).not.toHaveBeenCalled();
  });

  it("removes the blur and visibilitychange listeners on disconnect", () => {
    const win = vi.spyOn(window, "removeEventListener");
    const doc = vi.spyOn(document, "removeEventListener");
    mount(`<wordink-mic></wordink-mic>`).remove();
    expect(win.mock.calls.map((c) => c[0])).toContain("blur");
    expect(doc.mock.calls.map((c) => c[0])).toContain("visibilitychange");
    win.mockRestore();
    doc.mockRestore();
  });

  it("toggle: two clicks press twice (start, then stop); pointer events do nothing", () => {
    const mic = mount(`<wordink-mic mode="toggle"></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    pointer(button(mic), "pointerup");
    expect(createDictation).not.toHaveBeenCalled();
    button(mic).click();
    expect(last().press).toHaveBeenCalledTimes(1);
    last().emit("state", "listening");
    button(mic).click();
    expect(last().press).toHaveBeenCalledTimes(2);
    expect(last().release).not.toHaveBeenCalled();
  });

  it("shortcut: holding the key combination anywhere presses, releasing it releases", () => {
    const mic = mount(`<wordink-mic shortcut="Alt+D"></wordink-mic>`);
    key(document.body, "keydown", { key: "x", code: "KeyX", altKey: true });
    key(document.body, "keydown", { key: "d", code: "KeyD" });
    expect(createDictation).not.toHaveBeenCalled();
    const down = key(document.body, "keydown", { key: "∂", code: "KeyD", altKey: true });
    expect(down.defaultPrevented).toBe(true);
    expect(last().press).toHaveBeenCalledTimes(1);
    key(document.body, "keydown", { key: "∂", code: "KeyD", altKey: true, repeat: true });
    expect(last().press).toHaveBeenCalledTimes(1);
    key(document.body, "keyup", { key: "∂", code: "KeyD", altKey: true });
    expect(last().release).toHaveBeenCalledTimes(1);
    mic.remove();
    key(document.body, "keydown", { key: "d", code: "KeyD", altKey: true });
    expect(last().press).toHaveBeenCalledTimes(1);
  });

  it("shortcut in toggle mode presses on each keydown", () => {
    mount(`<wordink-mic mode="toggle" shortcut="F2"></wordink-mic>`);
    key(document.body, "keydown", { key: "F2", code: "F2" });
    key(document.body, "keyup", { key: "F2", code: "F2" });
    key(document.body, "keydown", { key: "F2", code: "F2" });
    expect(last().press).toHaveBeenCalledTimes(2);
    expect(last().release).not.toHaveBeenCalled();
  });

  it("ignores pointer input on the status text", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(status(mic), "pointerdown");
    expect(createDictation).not.toHaveBeenCalled();
  });
});

describe("states and events (R6)", () => {
  it("data-state cycles idle, listening, transcribing, idle with matching aria-pressed and status", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    const starts = vi.fn();
    document.addEventListener("wordink-start", starts);
    pointer(button(mic), "pointerdown");
    const d = last();
    const seen: string[] = [mic.getAttribute("data-state")!];

    d.emit("state", "requesting-mic");
    d.emit("state", "listening");
    seen.push(mic.getAttribute("data-state")!);
    expect(button(mic).getAttribute("aria-pressed")).toBe("true");
    expect(status(mic).textContent).toBe("Listening…");
    expect(starts).toHaveBeenCalledTimes(1);
    expect(mic.state).toBe("listening");

    d.emit("state", "transcribing");
    seen.push(mic.getAttribute("data-state")!);
    expect(button(mic).getAttribute("aria-pressed")).toBe("false");
    expect(status(mic).textContent).toBe("Transcribing…");

    d.emit("state", "idle");
    seen.push(mic.getAttribute("data-state")!);
    expect(status(mic).textContent).toBe("");
    expect(seen).toEqual(["idle", "listening", "transcribing", "idle"]);
    document.removeEventListener("wordink-start", starts);
  });

  it("an error sets data-state=error, shows the message and hint, and fires wordink-error", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    const onError = vi.fn();
    document.addEventListener("wordink-error", onError);
    pointer(button(mic), "pointerdown");
    const err = wordinkError("MicDenied", "Microphone blocked.", "Allow it in site settings.");
    last().emit("state", "error");
    last().emit("error", err);
    expect(mic.getAttribute("data-state")).toBe("error");
    expect(status(mic).textContent).toContain("Microphone blocked.");
    expect(status(mic).textContent).toContain("Allow it in site settings.");
    expect(onError).toHaveBeenCalledTimes(1);
    const event = onError.mock.calls[0]![0] as CustomEvent;
    expect(event.detail.error).toBe(err);
    expect(event.bubbles && event.composed).toBe(true);
    document.removeEventListener("wordink-error", onError);
  });

  it("a configuration error from createDictation is shown in the status part", () => {
    createDictation.mockImplementationOnce(() => {
      throw wordinkError("Config", "The groq provider needs an `endpoint`.", "Deploy @wordink/server.");
    });
    const mic = mount(`<wordink-mic></wordink-mic>`);
    const onError = vi.fn();
    mic.addEventListener("wordink-error", onError);
    pointer(button(mic), "pointerdown");
    expect(mic.getAttribute("data-state")).toBe("error");
    expect(status(mic).textContent).toContain("needs an `endpoint`");
    expect((onError.mock.calls[0]![0] as CustomEvent).detail.error.code).toBe("Config");
  });

  it("level drives the meter while listening and resets afterwards", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    pointer(button(mic), "pointerdown");
    last().emit("state", "listening");
    last().emit("level", 0.42);
    expect(meter(mic).style.getPropertyValue("--wordink-level")).toBe("0.42");
    last().emit("state", "transcribing");
    expect(meter(mic).style.getPropertyValue("--wordink-level")).toBe("0");
  });

  it("interim text fires wordink-interim and shows in the status part", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    const onInterim = vi.fn();
    mic.addEventListener("wordink-interim", onInterim);
    pointer(button(mic), "pointerdown");
    last().emit("state", "listening");
    last().emit("interim", "hello wor");
    expect((onInterim.mock.calls[0]![0] as CustomEvent).detail.text).toBe("hello wor");
    expect(status(mic).textContent).toBe("hello wor");
  });
});

describe("transcript insertion (R2, KTD7)", () => {
  it("fires wordink-transcript and inserts at the caret saved on press", () => {
    const mic = mount(`<textarea id="msg"></textarea><wordink-mic for="msg"></wordink-mic>`);
    const ta = document.getElementById("msg") as HTMLTextAreaElement;
    ta.value = "Hello world";
    ta.setSelectionRange(6, 6);
    const onTranscript = vi.fn();
    document.addEventListener("wordink-transcript", onTranscript);

    pointer(button(mic), "pointerdown");
    ta.setSelectionRange(0, 0); // focus moved to the button; the caret was lost
    last().emit("state", "listening");
    pointer(button(mic), "pointerup");
    last().emit("state", "transcribing");
    last().emit("final", "there");

    const event = onTranscript.mock.calls[0]![0] as CustomEvent;
    expect(event.detail.text).toBe("there");
    expect(event.bubbles && event.composed && event.cancelable).toBe(true);
    expect(ta.value).toBe("Hello there world");
    expect(document.activeElement).toBe(ta);
    document.removeEventListener("wordink-transcript", onTranscript);
  });

  it("does not insert when wordink-transcript is canceled (rich editors insert themselves)", () => {
    const mic = mount(`<textarea id="msg">Hello</textarea><wordink-mic for="msg"></wordink-mic>`);
    mic.addEventListener("wordink-transcript", (e) => e.preventDefault());
    pointer(button(mic), "pointerdown");
    last().emit("final", "there");
    expect((document.getElementById("msg") as HTMLTextAreaElement).value).toBe("Hello");
  });

  it("still fires wordink-transcript with no bound field", () => {
    const mic = mount(`<wordink-mic></wordink-mic>`);
    const onTranscript = vi.fn();
    mic.addEventListener("wordink-transcript", onTranscript);
    pointer(button(mic), "pointerdown");
    last().emit("final", "hi");
    expect(onTranscript).toHaveBeenCalledTimes(1);
  });
});

describe("custom button (R4)", () => {
  it("a slotted button replaces the default and still drives start and stop", () => {
    const mic = mount(`<wordink-mic><button id="custom">Talk</button></wordink-mic>`);
    const custom = document.getElementById("custom")!;
    const slot = shadow(mic).querySelector("slot")!;
    expect(slot.assignedElements()).toEqual([custom]);

    pointer(custom, "pointerdown");
    expect(last().press).toHaveBeenCalledTimes(1);
    last().emit("state", "listening");
    expect(custom.getAttribute("aria-pressed")).toBe("true");
    pointer(custom, "pointerup");
    expect(last().release).toHaveBeenCalledTimes(1);
    last().emit("state", "idle");
    expect(custom.getAttribute("aria-pressed")).toBe("false");
  });

  it("a slotted button works in toggle mode", () => {
    const mic = mount(`<wordink-mic mode="toggle"><button id="custom">Talk</button></wordink-mic>`);
    const custom = document.getElementById("custom")!;
    custom.click();
    last().emit("state", "listening");
    custom.click();
    expect(last().press).toHaveBeenCalledTimes(2);
    expect(mic.getAttribute("data-state")).toBe("listening");
  });
});
