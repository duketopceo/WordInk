# @wordink/web

## 0.1.0

### Minor Changes

- 10a10af: First release of `@wordink/web`: the `<wordink-mic>` element. Add it next to an input, textarea or contenteditable (`for="id"`) for push-to-talk or toggle dictation with an optional keyboard shortcut. Text is inserted at the saved caret with native undo (`execCommand('insertText')`, falling back to `setRangeText` plus an `input` event). It reflects `data-state`, exposes `button`, `meter` and `status` parts and CSS custom properties, accepts a slotted replacement button, and dispatches `wordink-start`, `wordink-interim`, `wordink-transcript` (cancelable, for rich editors) and `wordink-error`. Ships as an ES module and as a single-file CDN module (`dist/wordink-web.cdn.js`) that loads the core wasm from beside itself.

### Patch Changes

- 10a10af: `<wordink-mic>` in hold mode now releases when the window loses focus, the tab becomes hidden or pointer capture is lost, so a keyup or pointerup delivered elsewhere no longer leaves the microphone open or blocks the next hold.
- Updated dependencies [10a10af]
- Updated dependencies [10a10af]
- Updated dependencies [10a10af]
- Updated dependencies [10a10af]
  - @wordink/core@0.1.0
