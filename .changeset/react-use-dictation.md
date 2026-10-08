---
"@wordink/react": minor
---

First release of `@wordink/react` (React 19). `useDictation({ provider, endpoint, onTranscript, … })` returns `{ state, level, interim, error, start, stop, press, release }`: the dictation is created on the first gesture, recreated when an option changes, destroyed on unmount (releasing the microphone), and callbacks can change on every render without restarting it. `<WordInkMic>` is a typed wrapper for `<wordink-mic>` that maps `onTranscript`, `onInterim`, `onError` and `onStart` to the element's `wordink-*` events, sets complex config as properties and forwards `ref` to the element. Both are safe to import during server rendering.
