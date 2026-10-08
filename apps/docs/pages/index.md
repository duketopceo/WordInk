# Dictation as a component

WordInk adds push-to-talk dictation to any text field in your web app. Hold a button, speak, let go:
the text lands at the cursor, and the field's native undo still works. Use your own Groq, OpenAI or
Deepgram key through a small relay you host, or a local model that runs in the browser. No WordInk
account, no WordInk servers, no telemetry. MIT licensed.

```html
<textarea id="message"></textarea>
<wordink-mic for="message" endpoint="/api/wordink"></wordink-mic>
```

## Try it

<section class="demo" id="demo" aria-label="Live demo">
  <p class="demo-note">This demo runs <a href="local.html">Moonshine tiny</a> in your browser, so it needs no key and your audio never leaves this tab. The first press downloads the model (about 32 MB on WASM, about 80 MB on WebGPU); after that it is cached.</p>
  <label for="demo-text">Dictate here</label>
  <textarea id="demo-text" rows="4" placeholder="Hold the mic and speak…"></textarea>
  <div class="demo-row">
    <wordink-mic for="demo-text" shortcut="Alt+D"></wordink-mic>
    <span id="demo-status" role="status">Hold the button (or <kbd>Alt</kbd>+<kbd>D</kbd>) and speak.</span>
  </div>
  <progress id="demo-progress" max="100" value="0" hidden></progress>
  <div id="demo-devkey"></div>
</section>

## What you get

- **One element or one hook.** [`<wordink-mic>`](quickstart-html.html) for any page, including plain
  HTML with no build step, or [`useDictation`](quickstart-react.html) for React.
- **Your provider, your key.** Groq (default), OpenAI or Deepgram, switched with one attribute. See
  the [comparison](providers.html).
- **Keys stay on your server.** [`@wordink/server`](relay.html) forwards Groq audio and mints
  short-lived OpenAI and Deepgram tokens. It refuses every request until you give it an `authorize`
  check.
- **Offline option.** [`@wordink/local`](local.html) runs speech recognition in the browser.
- **Nothing phones home.** Every data flow is listed on the [privacy page](privacy.html).

## Packages

| Package | What it is |
|---|---|
| `@wordink/web` | The `<wordink-mic>` element, as an ES module and a single-file CDN build |
| `@wordink/react` | `useDictation` and a typed `<WordInkMic>` wrapper (React 19) |
| `@wordink/core` | The engine: a Rust core compiled to WebAssembly plus a small TypeScript host |
| `@wordink/local` | Offline speech recognition (Moonshine via transformers.js) |
| `@wordink/server` | The credential relay for Cloudflare Workers, Node or any Fetch API runtime |
