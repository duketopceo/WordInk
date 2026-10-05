# @wordink/react

React bindings for WordInk push-to-talk dictation: a `useDictation` hook for your own UI, and
`<WordInkMic>`, a typed wrapper for the [`<wordink-mic>`](../web) element. Requires React 19.

```sh
npm install @wordink/react
```

## useDictation

```tsx
import { useDictation } from "@wordink/react";
import { useState } from "react";

function Message() {
  const [text, setText] = useState("");
  const { state, level, interim, error, press, release } = useDictation({
    provider: "groq",
    endpoint: "/api/wordink", // your @wordink/server relay
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

### Options

| Option | Default | |
|---|---|---|
| `provider` | (required) | `"groq"`, `"openai"`, `"deepgram"`, or a `HostProvider` (such as `@wordink/local`) |
| `endpoint` | | Relay base URL |
| `devKey` | | Provider key sent from the page. Localhost only |
| `mode` | `"hold"` | `"hold"`: `press` starts, `release` stops. `"toggle"`: each `press` starts or stops |
| `hint` | | Vocabulary / prompt hint |
| `model` | provider default | Model override |
| `transform` | | `async (text) => text`, run before `onTranscript` |
| `transformTimeoutMs` | `3000` | On timeout the raw text is used |
| `onTranscript` | | `(text) => void`, once per dictation with the final text |
| `onError` | | `(error: WordInkError) => void` |

### Result

| Field | |
|---|---|
| `state` | `"idle"`, `"requesting-mic"`, `"listening"`, `"transcribing"` or `"error"` |
| `level` | Input level, 0 to 1, while listening |
| `interim` | Transcript so far (streaming providers: OpenAI, Deepgram) |
| `error` | The last `WordInkError` (`code`, `message`, `hint`), or `null` |
| `start()` / `stop()` | Start listening; stop and transcribe |
| `press()` / `release()` | Raw button down / up; `mode` decides what they do |

Call `start()` / `press()` from a user gesture. The dictation is created on the first one, recreated
after any option except the callbacks changes, and destroyed on unmount, which releases the
microphone. `onTranscript`, `onError` and `transform` can be new functions on every render.

## WordInkMic

The `<wordink-mic>` element as a React component: the default button, keyboard shortcut, level meter
and status, with text inserted at the cursor of the bound field.

```tsx
import { WordInkMic } from "@wordink/react";

<textarea id="message" />
<WordInkMic htmlFor="message" endpoint="/api/wordink" shortcut="Alt+D" onError={(e) => console.warn(e.hint)} />
```

Props are the element's attributes and properties (`htmlFor`, `mode`, `provider`, `endpoint`,
`shortcut`, `devKey`, `hint`, `model`, `transform`, `transformTimeoutMs`) plus its events:

| Prop | Event | |
|---|---|---|
| `onStart(event)` | `wordink-start` | Listening began |
| `onInterim(text, event)` | `wordink-interim` | Transcript so far |
| `onTranscript(text, event)` | `wordink-transcript` | Final text. `event.preventDefault()` to insert it yourself |
| `onError(error, event)` | `wordink-error` | A `WordInkError` |

`ref` is the element. Children replace the default button. Plain `<wordink-mic>` is also typed in
JSX once `@wordink/react` is imported.

For a React-controlled field the element's insertion fires an `input` event, so `onChange` sees it.

Both are safe to import during server rendering. No telemetry is sent anywhere.
