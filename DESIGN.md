# WordInk design direction (proposal)

Status: proposal for owner sign-off. Nothing in the packages or docs site uses these tokens yet; applying them is a later code change. Today the docs site and `<wordink-mic>` use indigo `#4f46e5` (`apps/docs/src/style.css`, `packages/web/src/styles.ts`).

## Direction: "Pen and Signal"

WordInk turns a voice signal into written text. The identity is a waveform that ends in a pen nib: speech on the left and right, ink in the middle. The look is a developer component, not a consumer app: near-black ink on cool paper, one teal accent used only for "live" things (the nib, the recording state, links, focus), thin rules, a monospace for anything you copy. No gradients, no glass, no illustrations of people.

## Research basis and reference lock

The Refero MCP was not available in this session, so there are no Refero style records. The research is the product pages and repositories fetched for `docs/research/2026-10-10-landscape.md` (Superwhisper, Wispr Flow, MacWhisper, Voxtype, Talon, nerd-dictation, whisper.cpp, Groq and OpenAI docs) read as text, plus the repo's own existing UI. Treat the visual judgments below as a proposal grounded in what those products emphasise, not a pixel survey.

- **Primary direction:** a developer-tool component identity, in the spirit of the terminal-native Linux tools (Voxtype, nerd-dictation) rather than the consumer-app look of Wispr Flow or Superwhisper.
- **Preserve:** cool near-black and near-white neutrals; a single teal accent with a fixed role; monospace for code and tokens; 2 px round-cap geometric glyphs; visible state changes (idle, listening, processing, error) that do not rely on colour alone.
- **Borrow only:** from Superwhisper, the idea of named modes shown as plain labels (for future cleanup presets); from Voxtype, the plain, status-first terminal output.
- **Role rules:** teal means live or interactive, never decoration; red is error only; amber is warning only.
- **Media strategy:** code-native SVG only (mark, glyphs, social card). No photography, stock icons or generated imagery.
- **Reject:** indigo/violet default (the current accent), gradient buttons, centred hero over a feature grid, decorative serif word swaps, microphone-with-sparkles imagery.

## Decision ledger

| Decision | Source | Why |
|---|---|---|
| Mark = three bars with a nib | Product: speech in, text out | Reads as audio and as writing; works at 16 px |
| Teal accent replaces indigo | Reject-list; contrast checks below | Indigo is the generic default; teal passes AA on both themes |
| Monospace for tokens and commands | Gateway device tokens (`wdk_...`), CLI | Users copy these |
| State shown by glyph plus text plus colour | Accessibility rule (WCAG 1.4.1) | Colour must not carry state alone |
| Reduced-motion respected | WCAG 2.3.3 | Listening animation is optional |

## Colour tokens

Contrast ratios computed with the WCAG 2.x relative-luminance formula. AA needs 4.5:1 for text and 3:1 for large text and UI components.

| Token | Light | Dark | Role |
|---|---|---|---|
| `--wi-bg` | `#F8F9FA` | `#0B0E12` | Page |
| `--wi-surface` | `#FFFFFF` | `#141920` | Cards, the mic control |
| `--wi-surface-sunk` | `#EDF0F3` | `#0B0E12` | Code blocks (light only differs) |
| `--wi-ink` | `#0E1116` | `#E8ECEF` | Body text, mark bars |
| `--wi-muted` | `#4A5260` | `#9AA3AF` | Secondary text |
| `--wi-accent` | `#0A6B73` | `#4FD1C5` | Nib, links, focus ring, listening state |
| `--wi-on-accent` | `#FFFFFF` | `#0B0E12` | Text on accent fill |
| `--wi-error` | `#B42318` | `#F97066` | Error only |
| `--wi-warn` | `#8A5A00` | `#F0B84A` | Warning text only |
| `--wi-rule` | `#D9DEE4` | `#2A313B` | Borders (decorative, not relied on for meaning) |

Checked pairs (ratio):

| Pair | Light | Dark |
|---|---|---|
| ink on bg | 17.94 | 16.28 |
| muted on bg | 7.47 | 7.58 |
| muted on surface | 7.87 | 6.92 |
| accent on bg | 5.92 | 10.37 |
| accent on surface | 6.24 | 9.46 |
| on-accent on accent | 6.24 | 10.37 |
| error on bg | 6.24 | 6.94 |
| warn on bg | 5.62 | 10.74 |

All text pairs pass AA. `--wi-rule` is not checked because borders are never the only cue.

## Type tokens

System stacks only, so the 34.7 KB core budget and the zero-dependency CDN build are not affected by font files.

- **Sans:** `system-ui, -apple-system, "Segoe UI", sans-serif` (as today). Body 16/1.6; headings weight 650-700, tight tracking; `text-wrap: balance` on headings.
- **Mono:** `ui-monospace, "SF Mono", "JetBrains Mono", Menlo, Consolas, monospace`. 0.9em inside prose; code blocks 14/1.5. Used for tokens, commands, endpoints.
- **Scale:** 12 (status text), 14 (code, labels), 16 (body), 20, 28, 40 (docs headings). The wordmark is drawn, not typeset.

## Icon and glyph style

- 24 px grid, 2 px stroke, round caps and joins, `currentColor`, no fills except the mark's nib. This matches the existing mic icon in `packages/web/src/styles.ts`.
- State glyphs in `assets/brand/glyphs.svg` (proposal): `wi-idle` (nib with side bars), `wi-listening` (five bars), `wi-processing` (dashes), `wi-error` (exclamation). Each state also has a text label.
- Mark colours: bars `--wi-ink`, nib `--wi-accent`. Single-colour use is allowed (all `currentColor`).

## Motion rules

- Listening: bars scale on the Y axis from live input level when available, otherwise a 1.2 s ease-in-out loop. Processing: dashes step through opacity, 0.9 s, linear.
- State change cross-fades in 120 ms. Nothing moves on page load.
- `prefers-reduced-motion: reduce`: no loops; show the glyph and the text label only.
- Never animate layout; the control keeps a fixed size so inserted text does not jump.

## Surfaces

| Surface | Direction |
|---|---|
| `<wordink-mic>` | Round 40 px button, `--wi-surface` fill, 2 px `--wi-rule` border; listening state fills with `--wi-accent` and shows the listening glyph; error state uses `--wi-error` text plus glyph. Keep the existing `--wordink-*` custom properties as the public theming API. |
| React `WordInkMic` | Same element styling and tokens |
| Docs site (`apps/docs`) | Tokens above replace indigo; left nav and code blocks stay; the live demo is the hero, not an illustration |
| README | Lockup at top (`assets/brand/lockup.svg` with a dark variant via `<picture>`) |
| CLI (`wordink`, `wordink-gateway`) | No colour required; status-first plain lines; mono output; optional teal for the success line only when stdout is a TTY |
| Gateway token screen | Shown once, mono, with a copy-hint line |
| Social card | `assets/brand/social-card.svg`, dark, mark plus wordmark |
| Bar widget / plugin panel / TUI | None exist in this repo |

## Logo (proposal, owner signs off)

See `assets/brand/README.md`. Files: `logo.svg`, `logo-dark.svg`, `wordmark.svg`, `wordmark-dark.svg`, `lockup.svg`, `lockup-dark.svg`, `glyphs.svg`, `social-card.svg`; PNG previews in `assets/brand/preview/`.
