# Desktop apps

WordInk doesn't ship its own desktop dictation app. It gives the ones you already use a better backend instead. `@wordink/server` includes a **gateway**: a standard OpenAI-compatible `POST /v1/audio/transcriptions` endpoint that any desktop dictation app can point at.

What the gateway adds over calling a provider directly:

- **Fallback.** Providers are tried in order (for example Groq, a second Groq key, then OpenAI, then Deepgram). A provider that is rate-limited, down, slow, or rejecting its key is skipped and briefly cooled down, so dictation keeps working.
- **Keys in one place.** Provider keys live only on the gateway. Each device gets its own WordInk token, and revoking it locks out that one device.
- **One vocabulary.** Your word list (product names, people, jargon) is sent as a spelling hint with every request, from every app.

## Run it

The gateway is a local service. It listens on `127.0.0.1:8941` by default.

```sh
# @wordink/server isn't on npm yet; build the CLI from the repo:
pnpm --filter @wordink/server build
ln -sf "$(pwd)/packages/server/dist/bin/wordink-gateway.js" ~/.local/bin/wordink-gateway
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

## Voxtype (Linux)

In `~/.config/voxtype/config.toml`, under `[whisper]`:

```toml
mode = "remote"
remote_endpoint = "http://127.0.0.1:8941"
remote_api_key = "wdk_…"   # your device token, or set VOXTYPE_WHISPER_API_KEY
```

Move your `initial_prompt` words into the gateway's `vocabulary` so every app shares them. The gateway caps the combined prompt at 800 characters, with gateway vocabulary first.

## Other apps

Any app with an "OpenAI-compatible" or "custom endpoint" transcription setting works:

- **Base URL:** `http://127.0.0.1:8941/v1`, or `http://127.0.0.1:8941` if the app adds `/v1` itself.
- **API key:** your device token.
- **Model:** any value. The gateway picks the model per provider.

A dedicated TypeWhisper plugin is planned.

## Tokens

```sh
wordink-gateway tokens list           # id, label, created, revoked (never the token)
wordink-gateway tokens revoke <id>    # takes effect on the device's next request
```

Tokens are stored hashed in `~/.local/state/wordink/gateway-tokens.json` (mode 0600).

## Responses

The gateway answers in OpenAI's shapes:

- `json` returns `{"text": "…"}`. This is the default.
- `text` returns plain text.
- `verbose_json` returns the text plus language, duration and segments when available.

Errors use OpenAI's error shape:

| Status | Meaning |
|---|---|
| 401 | Unknown or revoked token |
| 413 | Audio too large (25 MB by default) |
| 429 | Rate limit for this device |
| 502 | Every provider failed |
| 504 | The overall 30 s deadline passed |

Responses never say which provider served a request.
