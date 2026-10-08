# Quickstart: React

`@wordink/react` needs React 19. It brings `@wordink/web` and `@wordink/core` with it.

```sh
npm install @wordink/react
```

## The ready-made button: `<WordInkMic>`

```tsx
import { WordInkMic } from "@wordink/react";

export function Message() {
  return (
    <>
      <textarea id="message" />
      <WordInkMic htmlFor="message" endpoint="/api/wordink" shortcut="Alt+D" onError={(e) => console.warn(e.hint)} />
    </>
  );
}
```

`<WordInkMic>` is the [`<wordink-mic>`](quickstart-html.html) element as a typed component: the default
button, keyboard shortcut, level meter and status, with text inserted at the cursor of the bound field.
For a controlled field the insertion fires an `input` event, so your `onChange` sees it. Props are the
element's attributes and properties (`htmlFor`, `mode`, `provider`, `endpoint`, `shortcut`, `devKey`,
`hint`, `model`, `transform`, `transformTimeoutMs`) plus `onStart`, `onInterim`, `onTranscript` and
`onError`.

`endpoint` is your [`@wordink/server`](relay.html) relay.

## Your own UI: `useDictation`

```tsx
import { useDictation } from "@wordink/react";
import { useState } from "react";

export function Message() {
  const [text, setText] = useState("");
  const { state, level, interim, error, press, release } = useDictation({
    provider: "groq",
    endpoint: "/api/wordink",
    onTranscript: (t) => setText((prev) => (prev ? `${prev} ${t}` : t)),
  });

  return (
    <>
      <input value={text} onChange={(e) => setText(e.target.value)} />
      <button onPointerDown={() => press()} onPointerUp={() => release()}>
        {state === "listening" ? "Listening…" : "Hold to talk"}
      </button>
      <meter min={0} max={1} value={level} />
      <p role="status">{error ? `${error.message} ${error.hint}` : interim}</p>
    </>
  );
}
```

The dictation is created on the first press, recreated when an option other than the callbacks
changes, and destroyed on unmount, which releases the microphone. Call `press()` / `start()` from a
user gesture. Both exports are safe to import during server rendering.

## With the local engine

```sh
npm install @wordink/local
```

```tsx
import { useDictation } from "@wordink/react";
import { createLocalProvider } from "@wordink/local";

const local = createLocalProvider(); // create it once, outside render

export function Message() {
  const dictation = useDictation({ provider: local, onTranscript: (t) => console.log(t) });
  // …
}
```

Create the provider once (module scope or `useMemo`): a new provider object counts as a changed
option and recreates the dictation. With Vite, keep the package out of dependency pre-bundling so its
worker loads: `optimizeDeps: { exclude: ["@wordink/local"] }`.

The full option and result tables are in the
[`@wordink/react` README](https://github.com/duketopceo/WordInk/tree/master/packages/react#readme).
