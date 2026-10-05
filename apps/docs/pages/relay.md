# The credential relay

`@wordink/server` keeps your long-lived provider keys on your server. Each provider is handled the way
its API allows:

- **Groq** has no temporary tokens, so the relay forwards the browser's audio upload with your key.
- **OpenAI**: the relay mints a short-lived `ek_` realtime client secret, and the browser opens the
  WebSocket to OpenAI with it.
- **Deepgram**: the relay mints a short-lived JWT, and the browser opens the WebSocket to Deepgram
  with it.

The relay is a Web-standard `(Request) => Promise<Response>` handler with no framework dependencies,
plus thin adapters for Cloudflare Workers and `node:http`.

```sh
npm install @wordink/server
```

## Cloudflare Worker

```ts
// src/index.ts
import { createWorker } from "@wordink/server/cloudflare";

export default createWorker((env) => ({
  basePath: "/wordink",
  allowedOrigins: ["https://app.example.com"],
  authorize: (request) => isSignedIn(request, env), // required, see below
}));
```

```sh
npx wrangler secret put GROQ_API_KEY      # and/or OPENAI_API_KEY, DEEPGRAM_API_KEY
npx wrangler deploy
```

## Node

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

The Node adapter takes a second argument, `{ trustProxy }`. By default it replaces `X-Forwarded-For`
and `CF-Connecting-IP` with the socket address, so callers can't pick their own rate-limit bucket. Set
`trustProxy: true` only behind a proxy that overwrites those headers.

## Any Fetch API runtime

Deno, Bun, Hono, Next.js route handlers:

```ts
import { createRelay } from "@wordink/server";

const relay = createRelay({
  keys: { groq: process.env.GROQ_API_KEY, openai: process.env.OPENAI_API_KEY },
  authorize: (request) => isSignedIn(request),
});
// relay(request) returns a Response.
```

## Connect the browser

Set the element's (or hook's) `endpoint` to the relay's base URL including `basePath`, for example
`endpoint="https://relay.example.com/wordink"` or `endpoint="/wordink"` on the same origin. Relay
requests are sent with `credentials: "include"`, so your session cookie reaches `authorize`; for a
relay on another origin, list your app's origin in `allowedOrigins`.

## `authorize` is required

The relay **fails closed**. Until you pass `authorize`, every route returns `403` and the relay logs
one setup error at startup. Anyone who can reach an open relay can spend your provider quota, and
Origin and CORS checks are not authentication: any script outside a browser can send any `Origin`.

`authorize(request)` gets the incoming `Request` and returns (or resolves to) `true` to allow it.
Anything else, including a thrown error, is a `403`. Bind it to your app's own user session.

A session cookie checked by your app:

```ts
authorize: async (request) => {
  const cookie = request.headers.get("cookie");
  if (!cookie) return false;
  const res = await fetch("https://app.example.com/api/session", { headers: { cookie } });
  return res.ok;
},
```

A signed JWT (here with [`jose`](https://github.com/panva/jose)), rate-limited per user:

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

## Limits

| Option | Default | |
|---|---|---|
| `rateLimit` | `{ windowMs: 60000, max: 20, dailyMax: 1000 }` | `max` upstream calls per client per window; `dailyMax` upstream calls per UTC day across all clients. Over the limit: `429` with `Retry-After` |
| `clientId` | `CF-Connecting-IP`, then the first `X-Forwarded-For`, then `"anon"` | The rate-limit key. Prefer your user id |
| `maxBodyBytes` | 2 MB (about 60 s of 16 kHz PCM16) | Larger Groq uploads get `413` before any upstream call |
| `tokenTtlSeconds` | `120` | Lifetime of minted OpenAI and Deepgram credentials. Fixed on the server; any TTL the client sends is ignored |

The built-in limiter is in memory: per process on Node, per isolate on Workers. For a hard global
limit across instances, pass `rateLimit: (clientId, request) => boolean | Promise<boolean>` backed by
KV, Redis or a Durable Object.

## Routes and status codes

All routes are `POST`, under `basePath`.

| Route | Upstream | Returns |
|---|---|---|
| `/groq/transcriptions` | Groq `audio/transcriptions` (multipart body forwarded as is) | Groq's transcription |
| `/openai/token` | OpenAI `realtime/client_secrets` | `{ "value": "ek_…", "expires_at": … }` |
| `/deepgram/token` | Deepgram `auth/grant` | `{ "access_token": "…", "expires_in": 120 }` |

`403`: not authorized, or no `authorize` hook. `413`: upload too large. `429`: rate limited, by the
relay or upstream. `502`: upstream failed (its error body is never passed through, because provider
errors can quote part of the key). `503`: no key configured for that route.

## What the relay logs

Nothing from request or response bodies (audio, transcripts, tokens), and never keys: only a setup
error, upstream HTTP status codes and error names. If you wrap it in your own middleware, keep it that
way; see [privacy](privacy.html).
