# WordInk React example

A controlled `<input>` that `useDictation` fills: hold the button, speak, release.

```sh
pnpm install
pnpm --filter @wordink/react... build          # builds @wordink/core, @wordink/web, @wordink/react
VITE_GROQ_KEY=gsk_… pnpm --filter @wordink/example-react dev
```

Open the printed `http://localhost:5173` URL. `VITE_GROQ_KEY` is used as a `devKey`: the key is sent
from the page, which `@wordink/core` allows only on `localhost`, `127.0.0.1` and `[::1]`. Without it
the app still builds and runs, and pressing the button shows a `Config` error. In production, drop
`devKey` and point `endpoint` at your [`@wordink/server`](../../packages/server) relay.

`pnpm --filter @wordink/example-react build` type-checks and builds to `dist/`.
