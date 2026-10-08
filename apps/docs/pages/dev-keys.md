# Dev keys

For a quick experiment you can give WordInk a provider key directly, with no relay:

```js
document.querySelector("wordink-mic").devKey = "gsk_…";
// or: createDictation({ provider: "groq", devKey: "gsk_…" })
// or: useDictation({ provider: "groq", devKey: "gsk_…" })
```

## A dev key is visible to anyone with access to the page

The key sits in the page's JavaScript and travels in its network requests. Anyone who can open the
page can read it from the developer tools and spend your provider quota with it. Treat any key you
have used this way as exposed.

## Localhost only

To stop a dev key from shipping by accident, WordInk refuses it unless the page is served from
`localhost`, `127.0.0.1` or `[::1]`. On any other hostname, creating the dictation fails with a
`Config` error, before any network request, that says why. On localhost it works and logs a console
warning that the key is visible.

Pass either `endpoint` or `devKey`, not both. When you deploy, remove the key and point `endpoint` at
your [relay](relay.html).

## Good practice

- Use a separate key for local experiments, with a low spending limit if the provider offers one.
- Don't commit it. Paste it at runtime (the [live demo](./#try-it) does this when you run the docs on
  localhost) or read it from a local, git-ignored file.
- Rotate it if it was ever on a page other people could open.
