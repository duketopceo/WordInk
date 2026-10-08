/**
 * Text insertion at the caret (KTD7, R2). `execCommand('insertText')` is the only path that keeps the
 * field's native undo, so it goes first; inputs and textareas fall back to `setRangeText` plus a
 * synthetic `input` event, so frameworks that listen for `input` (React's onChange) still update.
 */

/** A caret or selection captured when dictation starts, restored before inserting. */
export type SavedSelection = { start: number; end: number } | Range;

export type TextField = HTMLInputElement | HTMLTextAreaElement;

export function isTextField(el: Element | null | undefined): el is TextField {
  return el instanceof HTMLTextAreaElement || (el instanceof HTMLInputElement && el.selectionStart !== null);
}

/** Captures `target`'s caret or selection, if it has one. */
export function saveSelection(target: Element | null | undefined): SavedSelection | undefined {
  if (isTextField(target)) {
    const start = target.selectionStart ?? target.value.length;
    return { start, end: target.selectionEnd ?? start };
  }
  const sel = target ? target.ownerDocument.getSelection() : null;
  if (!target || !sel || sel.rangeCount === 0) return undefined;
  const range = sel.getRangeAt(0);
  return target.contains(range.commonAncestorContainer) ? range.cloneRange() : undefined;
}

/** Pads `text` with a space where it would otherwise run into the neighboring words. */
function spaced(text: string, before: string | undefined, after: string | undefined): string {
  const lead = before && !/\s/.test(before) ? " " : "";
  const trail = after && !/\s/.test(after) ? " " : "";
  return lead + text + trail;
}

/**
 * Inserts a transcript into `target` (an input, textarea or contenteditable) at `saved`, re-focusing
 * it first. The transcript is trimmed and spaced from adjacent words; an empty one inserts nothing.
 */
export function insertText(target: HTMLElement, transcript: string, saved?: SavedSelection): void {
  const text = transcript.trim();
  if (!text) return;
  const doc = target.ownerDocument;
  target.focus({ preventScroll: true });

  if (isTextField(target)) {
    if (saved && !(saved instanceof Range)) target.setSelectionRange(saved.start, saved.end);
    const start = target.selectionStart ?? target.value.length;
    const end = target.selectionEnd ?? start;
    const data = spaced(text, target.value[start - 1], target.value[end]);
    if (doc.execCommand("insertText", false, data)) return;
    target.setRangeText(data, start, end, "end");
    target.setSelectionRange(start + data.length, start + data.length);
    target.dispatchEvent(new InputEvent("input", { inputType: "insertText", data, bubbles: true }));
    return;
  }

  if (!target.isContentEditable) return;
  const sel = doc.getSelection();
  if (sel && saved instanceof Range) {
    sel.removeAllRanges();
    sel.addRange(saved);
  }
  const range = sel && sel.rangeCount > 0 ? sel.getRangeAt(0) : undefined;
  const before = range?.startContainer instanceof Text ? range.startContainer.data[range.startOffset - 1] : undefined;
  const after = range?.endContainer instanceof Text ? range.endContainer.data[range.endOffset] : undefined;
  doc.execCommand("insertText", false, spaced(text, before, after));
}
