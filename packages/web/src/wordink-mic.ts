/**
 * `<wordink-mic>`: a push-to-talk dictation button for an input, textarea or contenteditable (KTD6).
 * A vanilla custom element with shadow DOM; simple config is attributes, complex config properties.
 */
import {
  createDictation,
  type CloudProvider,
  type Dictation,
  type DictationOptions,
  type DictationState,
  type HostProvider,
  type WordInkError,
} from "@wordink/core";
import { insertText, type SavedSelection, saveSelection } from "./insert.js";
import { MIC_ICON, STYLES } from "./styles.js";

export interface WordInkTranscriptDetail {
  text: string;
}
export interface WordInkInterimDetail {
  text: string;
}
export interface WordInkErrorDetail {
  error: WordInkError;
}

/** Events the element dispatches. All bubble and are composed; `wordink-transcript` is cancelable. */
export interface WordInkMicEventMap {
  /** Listening began. */
  "wordink-start": CustomEvent<Record<string, never>>;
  /** Transcript so far (streaming providers); replaces the previous interim. */
  "wordink-interim": CustomEvent<WordInkInterimDetail>;
  /** The final text. Call `preventDefault()` to insert it yourself (rich editors). */
  "wordink-transcript": CustomEvent<WordInkTranscriptDetail>;
  "wordink-error": CustomEvent<WordInkErrorDetail>;
}

const STATUS: Record<DictationState, string> = {
  idle: "",
  "requesting-mic": "Starting microphone…",
  listening: "Listening…",
  transcribing: "Transcribing…",
  error: "",
};

/** Accessor properties that may have been set before the element was upgraded (re-applied on connect). */
const PROPS = ["provider", "endpoint", "mode", "shortcut", "htmlFor", "devKey", "hint", "model", "transform", "transformTimeoutMs"] as const;

/** Lets the module load where there is no DOM (server rendering); the element is defined only in browsers. */
const Base = (typeof HTMLElement === "undefined" ? class {} : HTMLElement) as typeof HTMLElement;

export class WordInkMic extends Base {
  static readonly observedAttributes = ["mode", "provider", "endpoint"];

  private config: Pick<DictationOptions, "devKey" | "hint" | "model" | "transform" | "transformTimeoutMs"> = {};
  private hostProvider: HostProvider | undefined;
  private dictation: Dictation | undefined;
  private unsubscribe: (() => void)[] = [];
  private saved: SavedSelection | undefined;
  private currentState: DictationState = "idle";
  private interim = "";
  private holding = false;
  private readonly root: ShadowRoot;
  private readonly button: HTMLButtonElement;
  private readonly slotEl: HTMLSlotElement;
  private readonly meter: HTMLElement;
  private readonly statusEl: HTMLElement;

  constructor() {
    super();
    this.root = this.attachShadow({ mode: "open" });
    this.root.innerHTML =
      `<style>${STYLES}</style>` +
      `<slot><button part="button" type="button" aria-pressed="false">${MIC_ICON}</button></slot>` +
      `<span part="meter" aria-hidden="true"></span>` +
      `<span part="status" role="status" aria-live="polite"></span>`;
    this.slotEl = this.root.querySelector("slot")!;
    this.button = this.root.querySelector("button")!;
    this.meter = this.root.querySelector('[part="meter"]')!;
    this.statusEl = this.root.querySelector('[part="status"]')!;
    this.addEventListener("pointerdown", this.onPointerDown);
    this.addEventListener("pointerup", this.onPointerUp);
    this.addEventListener("lostpointercapture", this.onFocusLost);
    this.addEventListener("pointercancel", this.onPointerUp);
    this.addEventListener("click", this.onClick);
    this.addEventListener("keydown", this.onKeyDown);
    this.addEventListener("keyup", this.onKeyUp);
    this.slotEl.addEventListener("slotchange", () => this.render());
  }

  // --- configuration ---------------------------------------------------------------------------

  /** `groq` (default), `openai`, `deepgram`, or a `HostProvider` object (property only). */
  get provider(): CloudProvider | HostProvider {
    return this.hostProvider ?? ((this.getAttribute("provider") ?? "groq") as CloudProvider);
  }
  set provider(value: CloudProvider | HostProvider) {
    if (typeof value === "string") {
      this.hostProvider = undefined;
      this.setAttribute("provider", value);
    } else {
      this.hostProvider = value;
      this.reset();
    }
  }

  /** Your `@wordink/server` relay base URL. */
  get endpoint(): string | null {
    return this.getAttribute("endpoint");
  }
  set endpoint(value: string | null) {
    this.setOrRemove("endpoint", value);
  }

  /** `hold` (push-to-talk, default) or `toggle`. */
  get mode(): "hold" | "toggle" {
    return this.getAttribute("mode") === "toggle" ? "toggle" : "hold";
  }
  set mode(value: "hold" | "toggle") {
    this.setAttribute("mode", value);
  }

  /** Keyboard shortcut held (or tapped, in toggle mode) anywhere on the page, e.g. `Alt+D` or `F2`. */
  get shortcut(): string | null {
    return this.getAttribute("shortcut");
  }
  set shortcut(value: string | null) {
    this.setOrRemove("shortcut", value);
  }

  /** The `for` attribute: id of the field to dictate into. */
  get htmlFor(): string | null {
    return this.getAttribute("for");
  }
  set htmlFor(value: string | null) {
    this.setOrRemove("for", value);
  }

  /** Provider key used directly from the page (localhost only; see @wordink/core). */
  get devKey(): string | undefined {
    return this.config.devKey;
  }
  set devKey(value: string | undefined) {
    this.configure("devKey", value);
  }

  /** Custom vocabulary or prompt hint. */
  get hint(): string | undefined {
    return this.config.hint;
  }
  set hint(value: string | undefined) {
    this.configure("hint", value);
  }

  /** Provider model override. */
  get model(): string | undefined {
    return this.config.model;
  }
  set model(value: string | undefined) {
    this.configure("model", value);
  }

  /** Post-processing run on the final text before insertion. Call your own backend. */
  get transform(): DictationOptions["transform"] {
    return this.config.transform;
  }
  set transform(value: DictationOptions["transform"]) {
    this.configure("transform", value);
  }

  /** Timeout for `transform`, in ms (default 3000). */
  get transformTimeoutMs(): number | undefined {
    return this.config.transformTimeoutMs;
  }
  set transformTimeoutMs(value: number | undefined) {
    this.configure("transformTimeoutMs", value);
  }

  /** Current dictation state (mirrored to the `data-state` attribute). */
  get state(): DictationState {
    return this.currentState;
  }

  connectedCallback(): void {
    for (const prop of PROPS) {
      if (Object.prototype.hasOwnProperty.call(this, prop)) {
        const value = (this as Record<string, unknown>)[prop];
        delete (this as Record<string, unknown>)[prop];
        (this as Record<string, unknown>)[prop] = value;
      }
    }
    document.addEventListener("keydown", this.onShortcutDown, true);
    document.addEventListener("keyup", this.onShortcutUp, true);
    window.addEventListener("blur", this.onFocusLost);
    document.addEventListener("visibilitychange", this.onVisibilityChange);
    this.render();
  }

  disconnectedCallback(): void {
    document.removeEventListener("keydown", this.onShortcutDown, true);
    document.removeEventListener("keyup", this.onShortcutUp, true);
    window.removeEventListener("blur", this.onFocusLost);
    document.removeEventListener("visibilitychange", this.onVisibilityChange);
    this.reset();
  }

  attributeChangedCallback(): void {
    this.reset();
    this.render();
  }

  private setOrRemove(name: string, value: string | null): void {
    if (value === null || value === undefined) this.removeAttribute(name);
    else this.setAttribute(name, value);
  }

  private configure<K extends keyof WordInkMic["config"]>(key: K, value: WordInkMic["config"][K] | undefined): void {
    if (value === undefined) delete this.config[key];
    else this.config[key] = value;
    this.reset();
  }

  private options(): DictationOptions {
    const o: DictationOptions = { provider: this.provider, mode: this.mode, ...this.config };
    const endpoint = this.endpoint;
    if (endpoint) o.endpoint = endpoint;
    return o;
  }

  /** Drops the current dictation; the next press creates one from the current config. */
  private reset(): void {
    for (const off of this.unsubscribe) off();
    this.unsubscribe = [];
    this.dictation?.destroy();
    this.dictation = undefined;
    this.holding = false;
    if (this.currentState !== "idle") this.setState("idle");
  }

  private ensureDictation(): Dictation | undefined {
    if (this.dictation) return this.dictation;
    let d: Dictation;
    try {
      d = createDictation(this.options());
    } catch (err) {
      this.fail(err as WordInkError);
      return undefined;
    }
    this.dictation = d;
    this.unsubscribe = [
      d.on("state", (s) => this.setState(s)),
      d.on("level", (level) => this.meter.style.setProperty("--wordink-level", String(level))),
      d.on("interim", (text) => {
        this.interim = text;
        this.render();
        this.fire("wordink-interim", { text });
      }),
      d.on("final", (text) => this.onFinal(text)),
      d.on("error", (error) => this.fail(error)),
    ];
    return d;
  }

  // --- input -----------------------------------------------------------------------------------

  /** The button the user operates: a slotted replacement, or the default. */
  private control(): HTMLElement {
    return (this.slotEl.assignedElements()[0] as HTMLElement | undefined) ?? this.button;
  }

  /** Whether an event came from the button (default or slotted), not the meter or status. */
  private fromControl(e: Event): boolean {
    return e.composedPath().includes(this.control());
  }

  private press(): void {
    const starting = this.currentState === "idle" || this.currentState === "error";
    if (starting) this.saved = saveSelection(this.target());
    void this.ensureDictation()?.press();
  }

  private release(): void {
    void this.dictation?.release();
  }

  private holdStart(): void {
    if (this.holding) return;
    this.holding = true;
    this.press();
  }

  private holdEnd(): void {
    if (!this.holding) return;
    this.holding = false;
    this.release();
  }

  /** A hold whose release lands elsewhere (window blur, lost pointer capture) must not leave the mic open. */
  private readonly onFocusLost = (): void => {
    if (this.mode === "hold") this.holdEnd();
  };

  private readonly onVisibilityChange = (): void => {
    if (document.hidden) this.onFocusLost();
  };

  private readonly onPointerDown = (e: PointerEvent): void => {
    if (this.mode !== "hold" || e.button !== 0 || !this.fromControl(e)) return;
    try {
      this.setPointerCapture(e.pointerId);
    } catch {
      // Not every environment supports capture; pointerup on the button still releases.
    }
    this.holdStart();
  };

  /** Pointer capture retargets pointerup to the host, so any pointerup ends a hold. */
  private readonly onPointerUp = (): void => {
    if (this.mode === "hold") this.holdEnd();
  };

  private readonly onClick = (e: MouseEvent): void => {
    if (this.mode === "toggle" && this.fromControl(e)) this.press();
  };

  private readonly onKeyDown = (e: KeyboardEvent): void => {
    if (this.mode !== "hold" || (e.key !== " " && e.key !== "Enter") || !this.fromControl(e)) return;
    e.preventDefault();
    if (!e.repeat) this.holdStart();
  };

  private readonly onKeyUp = (e: KeyboardEvent): void => {
    if (this.mode !== "hold" || (e.key !== " " && e.key !== "Enter") || !this.fromControl(e)) return;
    e.preventDefault();
    this.holdEnd();
  };

  /** Matches `Mod+Mod+Key`: modifiers exactly, the key by `event.key` or `event.code` (case-insensitive). */
  private matchesShortcut(e: KeyboardEvent, ignoreModifiers = false): boolean {
    const spec = this.shortcut;
    if (!spec) return false;
    const parts = spec.split("+").map((p) => p.trim().toLowerCase());
    const keyName = parts.pop();
    if (!keyName) return false;
    const mods = { alt: e.altKey, control: e.ctrlKey, ctrl: e.ctrlKey, shift: e.shiftKey, meta: e.metaKey };
    const keyMatches =
      e.key.toLowerCase() === keyName ||
      e.code.toLowerCase() === keyName ||
      e.code.toLowerCase() === `key${keyName}` ||
      e.code.toLowerCase() === `digit${keyName}`;
    if (!keyMatches || ignoreModifiers) return keyMatches;
    const wanted = new Set(parts.map((p) => (p === "ctrl" ? "control" : p)));
    return (["alt", "control", "shift", "meta"] as const).every((m) => mods[m] === wanted.has(m));
  }

  private readonly onShortcutDown = (e: KeyboardEvent): void => {
    if (!this.matchesShortcut(e)) return;
    e.preventDefault();
    if (e.repeat) return;
    if (this.mode === "toggle") this.press();
    else this.holdStart();
  };

  private readonly onShortcutUp = (e: KeyboardEvent): void => {
    if (this.mode !== "hold" || !this.holding || !this.matchesShortcut(e, true)) return;
    e.preventDefault();
    this.holdEnd();
  };

  // --- output ----------------------------------------------------------------------------------

  /** The bound field (`for`), looked up in the element's own tree (document or shadow root). */
  private target(): HTMLElement | null {
    const id = this.htmlFor;
    if (!id) return null;
    const root = this.getRootNode() as Document | ShadowRoot;
    return (root.getElementById?.(id) ?? document.getElementById(id)) as HTMLElement | null;
  }

  private onFinal(text: string): void {
    const proceed = this.fire("wordink-transcript", { text }, true);
    const target = this.target();
    if (proceed && target) insertText(target, text, this.saved);
    this.saved = undefined;
  }

  private fail(error: WordInkError): void {
    this.setState("error", error);
    this.fire("wordink-error", { error });
  }

  private setState(state: DictationState, error?: WordInkError): void {
    const was = this.currentState;
    this.currentState = state;
    if (state !== "listening" && state !== "transcribing") this.interim = "";
    if (state !== "listening") this.meter.style.setProperty("--wordink-level", "0");
    if (error) this.errorText = [error.message, error.hint].filter(Boolean).join(" ");
    else if (state !== "error") this.errorText = "";
    this.render();
    if (state === "listening" && was !== "listening") this.fire("wordink-start", {});
  }

  private errorText = "";

  private render(): void {
    this.setAttribute("data-state", this.currentState);
    const control = this.control();
    const active = this.currentState === "listening" || this.currentState === "requesting-mic";
    control.setAttribute("aria-pressed", String(active));
    if (control === this.button) this.button.setAttribute("aria-label", this.mode === "hold" ? "Hold to dictate" : "Dictate");
    const text = this.currentState === "error" ? this.errorText : this.interim || STATUS[this.currentState];
    if (this.statusEl.textContent !== text) this.statusEl.textContent = text;
  }

  private fire<K extends keyof WordInkMicEventMap>(type: K, detail: WordInkMicEventMap[K]["detail"], cancelable = false): boolean {
    return this.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true, cancelable }));
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "wordink-mic": WordInkMic;
  }
  interface HTMLElementEventMap extends WordInkMicEventMap {}
}
