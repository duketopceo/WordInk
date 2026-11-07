/**
 * Default theme for `<wordink-mic>` (R4). Everything is overridable from the page through CSS custom
 * properties on the element, or through `::part(button|meter|status)`.
 * Token defaults follow DESIGN.md and assets/brand/tokens.css (`--wi-*` chains through `--wordink-*`).
 */
export const STYLES = `
:host {
  --_accent: var(--wordink-accent, var(--wi-accent, #0a6b73));
  --_rule: var(--wordink-rule, var(--wi-rule, #d9dee4));
  --_surface: var(--wordink-bg, var(--wi-surface, #ffffff));
  --_on-accent: var(--wordink-active-icon, var(--wi-on-accent, #ffffff));
  --_error: var(--wordink-error, var(--wi-error, #b42318));
  --_size: var(--wordink-size, 2.5rem);
  display: inline-flex;
  align-items: center;
  gap: 0.5em;
  font: var(--wordink-font, inherit);
  color: var(--wordink-text, inherit);
  vertical-align: middle;
}
:host([hidden]) { display: none; }
[part="button"] {
  display: inline-grid;
  place-items: center;
  box-sizing: border-box;
  inline-size: var(--_size);
  block-size: var(--_size);
  padding: 0;
  border: var(--wordink-border, 2px solid var(--_rule));
  border-radius: var(--wordink-radius, 999px);
  background: var(--_surface);
  color: var(--wordink-icon, var(--wi-ink, currentColor));
  cursor: pointer;
  touch-action: none;
  user-select: none;
  -webkit-user-select: none;
  -webkit-touch-callout: none;
  transition: background 120ms, color 120ms, box-shadow 120ms, border-color 120ms;
}
[part="button"]:focus-visible { outline: 2px solid var(--_accent); outline-offset: 2px; }
[part="button"] svg { inline-size: 55%; block-size: 55%; }
:host([data-state="listening"]) [part="button"],
:host([data-state="requesting-mic"]) [part="button"] {
  background: var(--wordink-active-bg, var(--_accent));
  color: var(--_on-accent);
  border-color: var(--wordink-active-bg, var(--_accent));
}
:host([data-state="transcribing"]) [part="button"] { cursor: progress; }
:host([data-state="error"]) [part="button"] {
  border-color: var(--_error);
  color: var(--_error);
  background: var(--_surface);
}
[part="meter"] {
  display: block;
  inline-size: var(--wordink-meter-width, 3em);
  block-size: var(--wordink-meter-height, 0.35em);
  border-radius: 999px;
  background: var(--wordink-meter-track, color-mix(in srgb, currentColor 15%, transparent));
  overflow: hidden;
  visibility: hidden;
}
[part="meter"]::after {
  content: "";
  display: block;
  block-size: 100%;
  background: var(--wordink-meter-fill, var(--_accent));
  transform: scaleX(min(1, calc(var(--wordink-level, 0) * 4)));
  transform-origin: left;
  transition: transform 60ms linear;
}
:host([data-state="listening"]) [part="meter"] { visibility: visible; }
[part="status"] { font-size: var(--wordink-status-size, 0.875em); }
[part="status"]:empty { display: none; }
:host([data-state="error"]) [part="status"] { color: var(--_error); }
@media (prefers-reduced-motion: reduce) {
  [part="button"], [part="meter"]::after { transition: none; }
}
`;

const svg = (body: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

/** State glyphs from assets/brand/glyphs.svg (24 grid, 2px round stroke). */
export const GLYPH_IDLE = svg(`<path d="M12 3v13l-2.5 5h5L12 16"/><path d="M5 9v5M19 9v5"/>`);
export const GLYPH_LISTENING = svg(`<path d="M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4"/>`);
export const GLYPH_PROCESSING = svg(`<path d="M4 12h2M9 12h2M14 12h2M19 12h1"/>`);
export const GLYPH_ERROR = svg(`<path d="M12 4v10M12 19h.01"/>`);

/** @deprecated Use GLYPH_IDLE; kept for custom slotted buttons that copied the old default. */
export const MIC_ICON = GLYPH_IDLE;

export type MicGlyphState = "idle" | "requesting-mic" | "listening" | "transcribing" | "error";

export function glyphForState(state: MicGlyphState): string {
  switch (state) {
    case "listening":
      return GLYPH_LISTENING;
    case "requesting-mic":
    case "transcribing":
      return GLYPH_PROCESSING;
    case "error":
      return GLYPH_ERROR;
    default:
      return GLYPH_IDLE;
  }
}
