# Quickstart: plain HTML

No build step. One script tag, one element.

## 1. Add the element

@include snippets/quickstart.html

`for` is the `id` of the field to dictate into: an `<input>`, a `<textarea>` or a contenteditable.
Hold the button, speak, release, and the text is inserted at the cursor.

### The pinned version and the integrity hash

The script URL pins an exact version (`@wordink/web@__WEB_VERSION__`), and `integrity` is the
[Subresource Integrity](https://developer.mozilla.org/en-US/docs/Web/Security/Subresource_Integrity)
hash of that exact file, so the browser refuses to run it if the CDN ever serves different bytes.
SRI on a cross-origin script needs `crossorigin="anonymous"`. The hash on this page is generated
from the release build when the docs are published. To compute or check it yourself:

```sh
curl -sL https://cdn.jsdelivr.net/npm/@wordink/web@__WEB_VERSION__/dist/wordink-web.cdn.js \
  | openssl dgst -sha384 -binary | openssl base64 -A
```

Prefix the output with `sha384-`. When you upgrade, change the version and the hash together.

The CDN build is one ES module (about 11 KB gzipped) with `@wordink/core` bundled in. It fetches the
core WebAssembly file (`wordink_core_bg.wasm`, about 35 KB gzipped) from beside itself. The
`integrity` attribute covers the script, not that `.wasm` fetch. If you want both under your control,
self-host them: copy `dist/wordink-web.cdn.js` and `dist/wordink_core_bg.wasm` from the
`@wordink/web` package into one directory on your site and point `src` at it.

## 2. Point it at your relay

`endpoint="/api/wordink"` is the URL of your [`@wordink/server`](relay.html) relay. The relay holds
your provider key, so the key never reaches the page. It takes a few lines on Cloudflare Workers or
Node; the [relay guide](relay.html) has the setup, including the `authorize` check it requires.

To switch providers, change one attribute: `provider="groq"` (the default), `"openai"` or
`"deepgram"`. See the [provider comparison](providers.html).

## Trying it before you have a relay

On `localhost` you can skip the relay and give the element a provider key directly:

```html
<script type="module">
  document.querySelector("wordink-mic").devKey = "gsk_…"; // localhost only
</script>
```

Remove the `endpoint` attribute when you do: the element takes one or the other. A dev key only works
when the page is served from `localhost`, `127.0.0.1` or `[::1]`, and anyone who can open the page can
read it. Read the [dev key warning](dev-keys.html) before using one.

## No key at all: the local engine

[`@wordink/local`](local.html) runs speech recognition in the browser, so it needs no key and no relay.
Its model runs in a Web Worker, and browsers only start workers from your own origin, so serve its two
files from your site. Make them once:

@include snippets/vendor-local.sh

That writes `wordink-local/index.js` and `wordink-local/worker.js`. Next to them, create `index.html`:

@include snippets/quickstart-local.html

Serve the directory (for example `npx serve .` or `python3 -m http.server 8000`) and open it at
`http://localhost:…`. The first press downloads the model (about 32 MB); later visits load it from the
browser cache, and dictation then works offline. The page has to be a secure context (HTTPS or
localhost) because the model files are checksum-verified with WebCrypto.

## Next

- Style it: CSS custom properties, `::part(button)`, `::part(meter)`, `::part(status)`, or your own
  button inside the element. All attributes, properties and events are in the
  [`@wordink/web` README](https://github.com/duketopceo/WordInk/tree/master/packages/web#readme).
- Rich editors (ProseMirror, Lexical, Slate): handle the cancelable `wordink-transcript` event and
  insert the text yourself.
- Clean up text with a [`transform`](transform.html) that calls your own backend.
