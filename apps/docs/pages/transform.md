# Post-processing with `transform`

`transform` is an optional `async (text) => text` that runs after the final transcript and before the
text is inserted. Use it for punctuation fixes, filler-word removal or an LLM cleanup pass. It is off by
default because it adds latency to every dictation.

```js
const mic = document.querySelector("wordink-mic");
mic.transform = async (text) => {
  const res = await fetch("/api/cleanup", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ text }),
  });
  if (!res.ok) throw new Error(`cleanup failed: ${res.status}`);
  return (await res.json()).text;
};
mic.transformTimeoutMs = 3000; // the default
```

The same options exist on `createDictation` and `useDictation`. If `transform` takes longer than
`transformTimeoutMs` or throws, the raw transcript is used, so the user never loses what they said
(with `createDictation`, a `warning` event reports `TransformTimeout` or `TransformFailed`).

## Call your own backend, never an LLM API with a key in the page

`transform` runs in the browser. Anything it holds, everyone with access to the page holds. Point it
at an endpoint on your server that checks the user's session, applies your rate limits and calls the
model with a key that stays there. Do not put an OpenAI, Anthropic, Groq or any other LLM key in the
page for this, not even "just for the demo".

## Dictated text is untrusted input

Whatever the user says ends up in your prompt. Treat it like any other user input that reaches an
LLM:

- Keep your instructions in the system prompt and pass the transcript as clearly delimited data.
- Expect it to contain instructions ("ignore the above and…"); don't give the cleanup model tools,
  secrets or access to other users' data.
- Constrain the output (for example, reject a result much longer than the input) and fall back to
  the raw transcript when the result looks wrong.
- Escape it like any user input wherever you render it.
