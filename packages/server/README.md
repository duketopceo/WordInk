# @wordink/server

A small credential relay for WordInk, so your long-lived speech provider keys never reach the browser.

- **Groq**: has no temporary tokens, so the relay forwards the browser's audio upload with your key.
- **OpenAI**: mints a short-lived `ek_` realtime client secret for a transcription session.
- **Deepgram**: mints a short-lived JWT via the auth grant API.

The core is a Web-standard `(Request) => Promise<Response>` handler with no framework dependencies. Thin adapters cover Cloudflare Workers and `node:http`, and the core drops into anything that speaks `Request`/`Response` (Deno, Bun, Hono, Next.js route handlers).

## Quickstart

```bash
npm install @wordink/server
```

### Cloudflare Worker

```ts
// src/index.ts
import { createWorker } from "@wordink/server/cloudflare";

export default createWorker((env) => ({
  basePath: "/wordink",
  allowedOrigins: ["https://app.example.com"],
  authorize: (request) => isSignedIn(request, env), // required, see below
}));
```

```bash
npx wrangler secret put GROQ_API_KEY      # and/or OPENAI_API_KEY, DEEPGRAM_API_KEY
npx wrangler deploy
```

### Node

```ts
import { createServer } from "node:http";
import { createNodeHandler } from "@wordink/server/node";

// Keys are read from GROQ_API_KEY / OPENAI_API_KEY / DEEPGRAM_API_KEY.
createServer(
  createNodeHandler({
    basePath: "/wordink",
    allowedOrigins: ["https://app.example.com"],
    authorize: (request) => isSignedIn(request),
  }),
).listen(8787);
```

### Any Fetch-API runtime

```ts
import { createRelay } from "@wordink/server";

const relay = createRelay({
  keys: { groq: process.env.GROQ_API_KEY, openai: process.env.OPENAI_API_KEY },
  authorize: (request) => isSignedIn(request),
});
// relay(request) returns a Response.
```

Working examples live in `examples/server-cloudflare` and `examples/server-node` in the repo.

## Routes

All routes are `POST`, under the optional `basePath`.

| Route | Upstream | Returns |
|---|---|---|
| `/groq/transcriptions` | `POST https://api.groq.com/openai/v1/audio/transcriptions` (multipart body forwarded as-is) | Groq's transcription body |
| `/openai/token` | `POST https://api.openai.com/v1/realtime/client_secrets` | `{ "value": "ek_...", "expires_at": 1730000000 }` |
| `/deepgram/token` | `POST https://api.deepgram.com/v1/auth/grant` | `{ "access_token": "eyJ...", "expires_in": 120 }` |

A route whose key isn't configured answers `503`. Status codes the browser can see: `403` (not authorized, or no `authorize` hook), `413` (upload too large), `429` (rate limited, by the relay or upstream, with `Retry-After`), `502` (upstream failed; its error body is never passed through, because provider errors can quote part of the key).

## `authorize` is required

The relay **fails closed**. Until you pass `authorize`, every route returns `403` and the relay logs one setup error at startup. There is no permissive default, because anyone who can reach the relay can otherwise spend your provider quota.

`authorize(request)` gets the incoming `Request` and returns (or resolves to) `true` to allow it. Anything else, including a thrown error, is a `403`. Bind it to your app's own user session.

With a signed JWT (for example via [`jose`](https://github.com/panva/jose)):

```ts
import { jwtVerify } from "jose";

const secret = new TextEncoder().encode(process.env.SESSION_JWT_SECRET);

const relay = createRelay({
  keys: { deepgram: process.env.DEEPGRAM_API_KEY },
  authorize: async (request) => {
    const token = request.headers.get("authorization")?.replace(/^Bearer /, "");
    if (!token) return false;
    try {
      const { payload } = await jwtVerify(token, secret, { audience: "wordink" });
      return typeof payload.sub === "string";
    } catch {
      return false;
    }
  },
  clientId: (request) => request.headers.get("authorization") ?? "anon", // limit per user, not per IP
});
```

With a session cookie checked by your app:

```ts
authorize: async (request) => {
  const cookie = request.headers.get("cookie");
  if (!cookie) return false;
  const res = await fetch("https://app.example.com/api/session", { headers: { cookie } });
  return res.ok;
},
```

Cross-origin cookie auth needs the browser to send credentials; the relay sends `Access-Control-Allow-Credentials: true` for allowed origins.

## Options

| Option | Default | Notes |
|---|---|---|
| `authorize` | none (all `403`) | Required. See above. |
| `keys` | env in the adapters | `{ groq?, openai?, deepgram? }`. |
| `basePath` | `""` | e.g. `"/api/wordink"`. |
| `allowedOrigins` | `[]` | Origins that get CORS allow headers. |
| `rateLimit` | `{ windowMs: 60000, max: 20, dailyMax: 1000 }` | `max` upstream calls per client per window, `dailyMax` upstream calls per UTC day across all clients. Or pass `(clientId, request) => boolean \| Promise<boolean>` to use your own store. |
| `clientId` | CF-Connecting-IP, then first X-Forwarded-For, then `"anon"` | Rate-limit key. Prefer your user id. |
| `maxBodyBytes` | `2 MB` (about 60 s of 16 kHz PCM16) | Larger Groq uploads get `413` before any upstream call. |
| `tokenTtlSeconds` | `120` | Lifetime of minted credentials. Any TTL the client sends is ignored. |

The Node adapter takes a second argument, `{ trustProxy }`. By default it replaces `X-Forwarded-For` and `CF-Connecting-IP` with the socket address so callers can't choose their own rate-limit bucket. Set `trustProxy: true` only behind a proxy that overwrites those headers.

The built-in limiter is in memory: per process on Node, per isolate on Workers. For a hard global limit across instances, pass a `rateLimit` function backed by KV, Redis or a Durable Object.

## Security notes

- **Origin and CORS are not authentication.** `allowedOrigins` only controls which web pages a browser lets read the responses. Any script outside a browser can send any `Origin` header. `authorize` is what protects your keys and quota.
- **Dev keys are public keys.** WordInk's browser `devKey` mode (localhost only) puts your provider key in the page, where anyone with page access can read it. Use it for local experiments only, and use this relay for anything deployed.
- **No body logging.** The relay never logs request or response bodies (audio, transcripts, tokens) or keys. It logs only a setup error, upstream HTTP status codes and error names.
- **Keys never echo.** Successful mint responses are rebuilt from only the fields above; upstream error bodies are dropped.
- **Short-lived credentials.** Minted tokens live for `tokenTtlSeconds` (default 120 s), fixed on the server.
- The relay sends no telemetry anywhere.

## License

MIT
