/**
 * Default theme for `<wordink-mic>` (R4). Everything is overridable from the page through CSS custom
 * properties on the element, or through `::part(button|meter|status)`.
 */
export const STYLES = `
:host {
  --_accent: var(--wordink-accent, #4f46e5);
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
  border: var(--wordink-border, 1px solid color-mix(in srgb, var(--_accent) 40%, transparent));
  border-radius: var(--wordink-radius, 999px);
  background: var(--wordink-bg, #fff);
  color: var(--wordink-icon, var(--_accent));
  cursor: pointer;
  touch-action: none;
  user-select: none;
  -webkit-user-select: none;
  -webkit-touch-callout: none;
  transition: background 120ms, color 120ms, box-shadow 120ms;
}
[part="button"]:focus-visible { outline: 2px solid var(--_accent); outline-offset: 2px; }
[part="button"] svg { inline-size: 55%; block-size: 55%; }
:host([data-state="listening"]) [part="button"] {
  background: var(--wordink-active-bg, var(--_accent));
  color: var(--wordink-active-icon, #fff);
}
:host([data-state="transcribing"]) [part="button"] { opacity: 0.7; cursor: progress; }
:host([data-state="error"]) [part="button"] { border-color: var(--wordink-error, #dc2626); color: var(--wordink-error, #dc2626); }
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
:host([data-state="error"]) [part="status"] { color: var(--wordink-error, #dc2626); }
`;

/** A microphone icon (inherits `currentColor`). */
export const MIC_ICON = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="2" width="6" height="12" rx="3"/><path d="M5 10a7 7 0 0 0 14 0M12 17v5M8 22h8"/></svg>`;
