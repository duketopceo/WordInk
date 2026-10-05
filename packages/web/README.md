# @wordink/web

`<wordink-mic>`: a drop-in push-to-talk dictation button for any `<input>`, `<textarea>` or
contenteditable. Hold the button (or a keyboard shortcut), speak, release: the text is inserted at
the cursor, and the field's native undo still works.

## Quickstart: plain HTML, no build step

```html
<textarea id="message"></textarea>
<wordink-mic for="message" endpoint="/api/wordink"></wordink-mic>

<script
  type="module"
  src="https://cdn.jsdelivr.net/npm/@wordink/web@VERSION/dist/wordink-web.cdn.js"
  integrity="sha384-…"
  crossorigin="anonymous"
></script>
```

Pin an exact version and its Subresource Integrity hash. The [HTML quickstart](https://duketopceo.github.io/WordInk/quickstart-html.html)
shows the current release with its hash filled in and how to compute it yourself.

The CDN build is one ES module (about 11 KB gzipped, budget 40 KB) with `@wordink/core` bundled in.
It fetches the core wasm (`wordink_core_bg.wasm`, about 35 KB gzipped) from beside itself, so if you
self-host it, serve both files from the same directory. See [`examples/html`](../../examples/html).

`endpoint` is your [`@wordink/server`](../server) relay, which keeps the provider key off the page.
For a quick local experiment you can set a provider key instead (localhost only, visible to anyone
with the page): `document.querySelector("wordink-mic").devKey = "gsk_…"`.

## Quickstart: bundler

```sh
npm install @wordink/web
```

```js
import "@wordink/web"; // registers <wordink-mic>
```

## Attributes

| Attribute | Default | |
|---|---|---|
| `for` | | id of the field to dictate into |
| `mode` | `hold` | `hold` (push-to-talk) or `toggle` (tap to start, tap to stop) |
| `provider` | `groq` | `groq`, `openai` or `deepgram` |
| `endpoint` | | Your relay's base URL |
| `shortcut` | | A key combination held anywhere on the page, e.g. `Alt+D`, `Control+Shift+Space`, `F2` |

## Properties

The attributes above are also properties (`for` is `htmlFor`). Complex config is property-only:

| Property | |
|---|---|
| `provider` | Also accepts a `HostProvider` object, such as `@wordink/local` |
| `devKey` | Provider key used directly from the page. Localhost only |
| `hint` | Custom vocabulary or prompt hint |
| `model` | Provider model override |
| `transform` | `async (text) => text`, run before insertion. Call your own backend |
| `transformTimeoutMs` | Default 3000. On timeout the raw text is inserted |
| `state` | Read-only: `idle`, `requesting-mic`, `listening`, `transcribing` or `error` |

Changing any of them takes effect on the next press.

## Events

All are `CustomEvent`s that bubble and cross shadow roots.

| Event | `detail` | |
|---|---|---|
| `wordink-start` | `{}` | Listening began |
| `wordink-interim` | `{ text }` | Transcript so far (OpenAI, Deepgram); replaces the previous one |
| `wordink-transcript` | `{ text }` | Final text. Cancelable: call `preventDefault()` to insert it yourself |
| `wordink-error` | `{ error }` | A `WordInkError` with `code`, `message` and `hint` |

Rich editors (ProseMirror, Lexical, Slate, …) should handle `wordink-transcript`, call
`preventDefault()` and insert through their own API.

## Insertion

On press the element remembers the field's caret or selection. On the final transcript it re-focuses
the field, restores the caret and inserts with `document.execCommand("insertText")`, which keeps native
undo. If that isn't available it falls back to `setRangeText` plus an `input` event, so frameworks such
as React still see the change. A space is added where the text would otherwise run into a word.

## Styling

The state is reflected as `data-state` on the element (`idle`, `requesting-mic`, `listening`,
`transcribing`, `error`). Style the parts with `::part(button)`, `::part(meter)` and
`::part(status)`, or set these custom properties:

| Property | Default |
|---|---|
| `--wordink-accent` | `#4f46e5` |
| `--wordink-size` | `2.5rem` (button) |
| `--wordink-bg`, `--wordink-icon` | `#fff`, accent |
| `--wordink-active-bg`, `--wordink-active-icon` | accent, `#fff` (while listening) |
| `--wordink-border`, `--wordink-radius` | accent-tinted 1px, `999px` |
| `--wordink-error` | `#dc2626` |
| `--wordink-meter-width`, `--wordink-meter-height` | `3em`, `0.35em` |
| `--wordink-meter-track`, `--wordink-meter-fill` | tinted text color, accent |
| `--wordink-font`, `--wordink-text`, `--wordink-status-size` | inherit, inherit, `0.875em` |

`--wordink-level` (0 to 1) is set on the meter part while listening.

### Your own button

Put a button inside the element and it replaces the default one. It still drives start and stop, and
gets `aria-pressed`:

```html
<wordink-mic for="message" endpoint="/api/wordink">
  <button type="button">Hold to talk</button>
</wordink-mic>
```

## Accessibility

The default control is a real `<button>` with an `aria-label` and `aria-pressed`. In hold mode, Space
or Enter held on the focused button works like holding the pointer. State changes and errors are
announced through a `role="status"` live region (the status part).

No telemetry is sent anywhere. Full docs and a live demo: <https://duketopceo.github.io/WordInk/>.
