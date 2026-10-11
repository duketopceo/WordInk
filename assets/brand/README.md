# WordInk brand assets (proposal)

These are a proposal. The owner signs off logos before they are used as the official mark. All files are hand-written SVG; no raster sources, stock icons or generated images. PNG previews in `preview/` are rendered with `resvg` and optimised with `oxipng`.

## Mark (`logo.svg`, `logo-dark.svg`)

A 32 x 32 grid, three vertical rounded bars:

- Left and right bars: 4 wide, 10 tall (`y` 11 to 21), `rx` 2, at `x` 5 and 23. They read as a waveform.
- Centre bar: 4 wide, from `y` 4 (rounded top, radius 2) down to `y` 20, then a straight-edged taper to a point at `(16, 28)`. That tapered end is a pen nib, so the centre bar is both the loudest audio bar and the pen.
- Colours: side bars `#0E1116`, nib `#0A6B73` on light; `#E8ECEF` and `#4FD1C5` on dark. See `DESIGN.md` for contrast values.
- Legible at 16 px: the bars stay at least 2 px wide (checked by rendering at 16 px).

## Wordmark (`wordmark.svg`, `wordmark-dark.svg`)

"WordInk" drawn from strokes (3.2 wide, round caps and joins) on a 32-tall grid, so it does not depend on an installed font: straight diagonals for `W` and `k`, circles/ellipse for `o` and the bowl of `d`, a quarter-curve arm for `r` and `n`. The capital `I` has slab serifs to tell it from a lowercase `l`.

## Lockup (`lockup.svg`, `lockup-dark.svg`)

Mark at left, wordmark offset 44 units to the right, shared baseline.

## Glyphs (`glyphs.svg`)

An SVG `<symbol>` sprite for the four dictation states (idle, listening, processing, error): 24 px grid, 2 px round strokes, `currentColor`.

## Social card (`social-card.svg`)

1280 x 640, dark background, mark and wordmark, one teal rule. It contains no text elements so it renders the same without fonts.
