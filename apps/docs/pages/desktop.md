# Desktop apps

WordInk doesn't ship its own desktop dictation app. It gives the ones you already use a better backend instead. `@wordink/server` includes a **gateway**: a standard OpenAI-compatible `POST /v1/audio/transcriptions` endpoint (plus `GET /v1/models` for model pickers) that any desktop dictation app can point at.

What the gateway adds over calling a provider directly:

- **Fallback.** Providers are tried in order (for example Groq, a second Groq key, then OpenAI, then Deepgram). A provider that is rate-limited, down, slow, or rejecting its key is skipped and briefly cooled down, so dictation keeps working.
- **Keys in one place.** Provider keys live only on the gateway. Each device gets its own WordInk token, and revoking it locks out that one device.
- **One vocabulary.** Your word list (product names, people, jargon) is sent as a spelling hint with every request, from every app.

## Run it

The gateway is a local service. It listens on `127.0.0.1:8941` by default.

```sh
# @wordink/server isn't on npm yet; build the CLI from the repo:
pnpm --filter @wordink/server build
install -Dm755 packages/server/dist/bin/wordink-gateway.js ~/.local/bin/wordink-gateway
# (once published: npm install -g @wordink/server)

wordink-gateway check                 # which providers have keys (never prints them)
wordink-gateway serve                 # or install the systemd user service, see below
wordink-gateway tokens create laptop  # prints the device token once
```

The config file is `~/.config/wordink/gateway.json`:

```json
{
  "providers": [
    { "provider": "groq", "keyEnv": "GROQ_API_KEY" },
    { "provider": "openai", "keyEnv": "OPENAI_API_KEY" }
  ],
  "vocabulary": ["WordInk", "Omarchy", "Hyprland"]
}
```

The config names environment variables, never key values. On Linux, `examples/gateway-systemd` in the repo has a user service and a wrapper that fills those variables from omaseal at start, so keys never touch disk.

The gateway only listens on loopback. To serve other machines, put it behind TLS and set `"allowInsecureRemote": true`. Without TLS, tokens and audio would cross the network in cleartext.

The full `gateway.json` schema:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `host` | string | `"127.0.0.1"` | Bind address. Only loopback values are accepted unless `allowInsecureRemote` is set. |
| `port` | number | `8941` | Port to listen on. |
| `allowInsecureRemote` | boolean | `false` | Permit a non-loopback `host`. Use only behind TLS. |
| `providers` | array | `[]` | Ordered fallback chain. Each entry: `{"provider": "groq" \| "openai" \| "deepgram", "keyEnv": "ENV_NAME", "model"?: string, "url"?: string}`. `url` overrides are https-only, or http on loopback. |
| `vocabulary` | string[] | `[]` | Terms hinted to every request (provider prompt or keyterms). |
| `rateLimit` | object | — | `{"max"?: number, "windowMs"?: number, "dailyMax"?: number}` per device token. |
| `maxBodyBytes` | number | `26214400` | Request body cap (25 MB). |
| `upstreamTimeoutMs` | number | `15000` | Per-provider attempt timeout. |
| `deadlineMs` | number | `30000` | Whole-chain deadline across all providers. |

Provider keys resolve at serve time: a `keyEnv` whose variable is unset (or resolves to a non-string) skips that entry with a warning; an entry with no usable key never serves requests.

## Voxtype (Linux)

In `~/.config/voxtype/config.toml`, under `[whisper]`:

```toml
mode = "remote"
remote_endpoint = "http://127.0.0.1:8941"
remote_api_key = "wdk_…"   # your device token, or set VOXTYPE_WHISPER_API_KEY
```

Move your `initial_prompt` words into the gateway's `vocabulary` so every app shares them. The gateway caps the combined prompt at 800 characters, with gateway vocabulary first.

## TypeWhisper (macOS, Windows, iOS)

TypeWhisper's bundled **OpenAI Compatible** engine talks to the gateway directly — no WordInk plugin is needed or planned.

In Settings, add an **OpenAI Compatible** transcription engine:

- **Base URL:** `http://127.0.0.1:8941` — with **no `/v1`** at the end. TypeWhisper appends `/v1/audio/transcriptions` itself; a `/v1` in the base URL produces 404s. For a gateway on another machine, use its LAN or Tailscale address — which needs `host` and `allowInsecureRemote` set on the gateway, behind TLS only.
- **API key:** your device token.
- **Model:** pick from the discovered list — the gateway answers `GET /v1/models` with each provider entry's configured model — or type a name manually. The gateway picks the provider per request; the model name is just a label for the profile. Leave the transport on Auto (these model names all resolve to batch); do not enter `gpt-live-transcribe` or `gpt-realtime-whisper`, which would force a realtime WebSocket transport the gateway does not serve.
- **Translate mode:** unsupported. The gateway only implements `/v1/audio/transcriptions` — there is no `/v1/audio/translations`, and Deepgram has no translations endpoint to fall back to.

Live-verified on Windows: TypeWhisper 1.0.9 (Windows 11 ARM, `openai-compatible` plugin 1.0.6) served a transcription end to end — its own local API `POST 127.0.0.1:8978/v1/transcribe` returned `{"text":"Hello, world","engine":"openai-compatible","model":"whisper-large-v3-turbo"}`, which the engine produced by calling the gateway's `POST /v1/audio/transcriptions` and relaying `{"text":"Hello, world"}` upstream. Compatibility for macOS and iOS rests on the shared plugin source (iOS gained the same custom-endpoint profile shape in 0.3.0); dictation on Apple hardware is still on the operator checklist.

## Other apps

Any app with an "OpenAI-compatible" or "custom endpoint" transcription setting works:

- **Base URL:** `http://127.0.0.1:8941/v1`, or `http://127.0.0.1:8941` if the app adds `/v1` itself (TypeWhisper does — see above).
- **API key:** your device token.
- **Model:** any value, or pick from `GET /v1/models`. The gateway picks the model per provider.

## Tokens

```sh
wordink-gateway tokens list           # id, scope, label, created, revoked (never the token)
wordink-gateway tokens revoke <id>    # takes effect on the device's next request
```

Tokens are stored hashed in `~/.local/state/wordink/gateway-tokens.json` (mode 0600).

**Admin tokens.** `wordink-gateway tokens create --admin agent` mints an admin token — a superset credential that transcribes like any device token and additionally reaches the vocabulary routes below. Keep it for agents and operators; a plain `tokens create` device token earns a clean `403` there. `check` and `serve` warn when no live admin token exists.

## Runtime vocabulary

The `vocabulary` array in `gateway.json` is operator-owned and needs a restart to change. The gateway also keeps a **runtime vocabulary** — terms agents add on the fly, persisted to `~/.local/state/wordink/vocabulary.json` (mode 0600) with who-added-it provenance. The two merge per request: config terms first, then runtime terms in insertion order, so a new term reaches the provider on the very next transcription — no restart.

That is the agent correction loop: dictate → the transcript misspells a name → the agent adds the right spelling → the next dictation gets it right, from every app.

```
GET    /v1/vocabulary   # the effective list: config + runtime, with source/pinned/effective
POST   /v1/vocabulary   # {"terms": ["Kurultai"]} — add or respell terms
DELETE /v1/vocabulary   # {"term": "Kurultai"} — remove a runtime term
```

All three require an **admin token** (`Authorization: Bearer wdk_…`). Every response carries capacity metadata — `prompt_chars` (the packed prompt head against the 800-char budget) and `keyterm_count` (of the 50-keyterm Deepgram slice) — and each term reports `effective: {prompt, keyterm}` so a stored term that fits neither budget is never silently inert.

POST outcomes, per term (matching is always case-folded):

| Outcome | Meaning |
|---|---|
| `added` | New runtime term stored. |
| `already_present` | Exact duplicate of a runtime term — nothing written. |
| `updated` | Case-variant of a runtime term — the stored spelling was respelled. |
| `pinned` | Already a config term — nothing stored. |
| `config_shadowed` | Case-variant of a config term — a runtime *spelling override* was stored. The new casing wins, but the term stays config-owned (undeletable). |

POST also accepts `"remove": ["term", …]` (deletions in the same call — the proxy-safe path for clients that can't send DELETE bodies) and `"dryRun": true` (reports outcomes and post-change capacity without persisting). A batch that would exceed the 200-term runtime cap fails wholly with `409 vocabulary_full`.

DELETE removes runtime terms only: a config term answers `409 term_pinned` (edit `gateway.json`); deleting a runtime spelling override reverts to the config spelling (`reverted_to` in the response).

| Error | When |
|---|---|
| `400` | Malformed body: bad JSON, non-array `terms`, empty terms, a term over 200 chars, control characters, more than 200 items |
| `401 invalid_api_key` | Missing, unknown or revoked token |
| `403 insufficient_permissions` | A valid device token — vocabulary routes need admin |
| `404 term_not_found` | DELETE on a term that isn't there |
| `405` | Wrong method (`Allow: GET, POST, DELETE`) |
| `409 term_pinned` | DELETE on a config-owned term |
| `409 vocabulary_full` | The batch would exceed 200 runtime terms — nothing persisted |
| `413` | Body over 64 KB |
| `429 rate_limit_exceeded` | Per-token rate limit, shared with transcriptions |
| `500 server_error` | `vocabulary.json` unreadable — transcriptions keep working on config terms |

Python helpers ship the same surface: `wordink vocab list|add|remove` (resolving `WORDINK_ADMIN_TOKEN`, falling back to `WORDINK_DEVICE_TOKEN`) and the `vocab_*` tools in `wordink-mcp`.

Never add secrets, passwords or sensitive dictated content as terms — every term is sent verbatim to the transcription providers on every call.

## Responses

Two endpoints share the device-token gate and per-token rate limit: `POST /v1/audio/transcriptions` and `GET /v1/models` (the OpenAI list shape — each configured provider's effective model, de-duplicated, so apps with model discovery can populate their picker).

Transcription answers use OpenAI's shapes:

- `json` returns `{"text": "…"}`. This is the default.
- `text` returns plain text.
- `verbose_json` returns the text plus language, duration and segments when available.

Errors use OpenAI's error shape:

| Status | Meaning |
|---|---|
| 400 | Malformed request: missing `file`, unsupported `response_format`, or a non-multipart body |
| 401 | Unknown or revoked token |
| 405 | Wrong method on a route (`Allow: POST` for transcriptions, `Allow: GET` for `/v1/models`) |
| 413 | Audio too large (25 MB by default) |
| 429 | Rate limit for this device |
| 502 | Every provider failed |
| 504 | The overall 30 s deadline passed |

Responses never say which provider served a request.

## Watching it

Under the systemd service the gateway logs to the journal:

```sh
journalctl --user -u wordink-gateway -f
```

Provider failures appear as `gateway: groq#0 failed (HTTP 429)` — the provider is named in the log, never to the client — followed by which entry served or a 502. A provider in cooldown is skipped silently until it expires.

If dictation stops working:

- `wordink-gateway check` shows which providers have resolvable keys.
- `journalctl --user -u wordink-gateway` shows why each entry failed. Repeated `HTTP 401` from one entry means its key is wrong or revoked — fix the omaseal entry and restart the service.
- `502` answers mean every entry failed; `504` means the chain ran out of time.
