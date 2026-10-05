import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { insertText, saveSelection } from "../src/insert.js";

let execCommand: ReturnType<typeof vi.fn>;

beforeEach(() => {
  document.body.innerHTML = "";
  // happy-dom has no editing commands; each test decides what execCommand does.
  execCommand = vi.fn(() => false);
  Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true, writable: true });
});

afterEach(() => {
  vi.restoreAllMocks();
});

function textarea(value: string, caret: number, end = caret): HTMLTextAreaElement {
  const el = document.createElement("textarea");
  el.value = value;
  document.body.append(el);
  el.setSelectionRange(caret, end);
  return el;
}

describe("insertText into input and textarea (KTD7)", () => {
  it("uses execCommand('insertText') first, at the saved caret, with word spacing", () => {
    const el = textarea("Hello world", 6);
    const saved = saveSelection(el);
    el.setSelectionRange(0, 0); // the caret moved (or was lost) while dictating
    execCommand.mockImplementation(() => true);

    insertText(el, "there", saved);

    expect(document.activeElement).toBe(el);
    expect(execCommand).toHaveBeenCalledWith("insertText", false, "there ");
    expect([el.selectionStart, el.selectionEnd]).toEqual([6, 6]);
  });

  it("falls back to setRangeText plus a bubbling insertText input event when execCommand returns false", () => {
    const el = textarea("Hello world", 6);
    const saved = saveSelection(el);
    const inputs: InputEvent[] = [];
    document.body.addEventListener("input", (e) => inputs.push(e as InputEvent));

    insertText(el, "there", saved);

    expect(execCommand).toHaveBeenCalled();
    expect(el.value).toBe("Hello there world");
    expect(el.selectionStart).toBe("Hello there ".length);
    expect(inputs).toHaveLength(1);
    expect(inputs[0]).toBeInstanceOf(InputEvent);
    expect(inputs[0]!.inputType).toBe("insertText");
    expect(inputs[0]!.data).toBe("there ");
    expect(inputs[0]!.bubbles).toBe(true);
  });

  it("replaces a selection and adds a leading space after a word", () => {
    const el = textarea("Hello cruel world", 5, 11); // "Hello| cruel| world"
    insertText(el, "brave", saveSelection(el));
    expect(el.value).toBe("Hello brave world");
  });

  it("appends at the end of an input, trimming the transcript", () => {
    const el = document.createElement("input");
    el.value = "Hi";
    document.body.append(el);
    el.setSelectionRange(2, 2);
    insertText(el, "  there\n", saveSelection(el));
    expect(el.value).toBe("Hi there");
  });

  it("inserts nothing for an empty transcript", () => {
    const el = textarea("Hello", 5);
    const onInput = vi.fn();
    el.addEventListener("input", onInput);
    insertText(el, "   ", saveSelection(el));
    expect(el.value).toBe("Hello");
    expect(execCommand).not.toHaveBeenCalled();
    expect(onInput).not.toHaveBeenCalled();
  });
});

describe("insertText into contenteditable", () => {
  it("restores the saved range, re-focuses and inserts with execCommand", () => {
    const div = document.createElement("div");
    div.contentEditable = "true";
    div.textContent = "Hello world";
    document.body.append(div);
    const range = document.createRange();
    range.setStart(div.firstChild!, 6);
    range.collapse(true);
    getSelection()!.removeAllRanges();
    getSelection()!.addRange(range);
    const saved = saveSelection(div);
    getSelection()!.removeAllRanges();
    execCommand.mockImplementation(() => true);

    insertText(div, "there", saved);

    expect(document.activeElement).toBe(div);
    expect(execCommand).toHaveBeenCalledWith("insertText", false, "there ");
    const sel = getSelection()!;
    expect(sel.rangeCount).toBe(1);
    expect(sel.getRangeAt(0).startOffset).toBe(6);
  });

  it("does not save a selection that lies outside the target", () => {
    const div = document.createElement("div");
    div.contentEditable = "true";
    const other = document.createElement("p");
    other.textContent = "elsewhere";
    document.body.append(div, other);
    const range = document.createRange();
    range.selectNodeContents(other);
    getSelection()!.removeAllRanges();
    getSelection()!.addRange(range);
    expect(saveSelection(div)).toBeUndefined();
  });
});
