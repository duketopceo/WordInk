# Verify your gateway

A five-minute smoke test that proves the whole chain on any machine the gateway runs on — Linux, macOS or Windows: Node serves the endpoints, a provider key resolves, a device token authenticates, and audio comes back as text. The gateway itself needs only Node 20+; the recipe below also assumes a repo checkout (for the build and the bundled test clip), `pnpm`, and `curl` — `curl.exe` on Windows.

The gateway is a plain HTTP server. It needs no microphone, audio-device or accessibility permission anywhere — the dictation app that points at it owns those permissions on its own machine.

## The smoke test

Bash / zsh:

```sh
# 1. Build the CLI (until @wordink/server is on npm)
pnpm --filter @wordink/server build
GW="node packages/server/dist/bin/wordink-gateway.js"

# 2. Minimal config at the resolved path ($XDG_CONFIG_HOME or ~/.config)
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/wordink"
cat > "${XDG_CONFIG_HOME:-$HOME/.config}/wordink/gateway.json" <<'JSON'
{ "providers": [{ "provider": "groq", "keyEnv": "GROQ_API_KEY" }] }
JSON

# 3. Provider key in the environment
export GROQ_API_KEY=gsk_…
$GW check   # reports which providers have keys — never prints them

# 4. `serve` runs in the foreground — leave it in this terminal:
$GW serve                # listens on http://127.0.0.1:8941

# 5. In a SECOND terminal, repeat the setup then mint a device token:
export GROQ_API_KEY=gsk_…
GW="node packages/server/dist/bin/wordink-gateway.js"
$GW tokens create test   # prints wdk_… once — save it
```

PowerShell 5.1+:

```powershell
pnpm --filter @wordink/server build
$GW = "packages\server\dist\bin\wordink-gateway.js"

# Config at the resolved path ($env:XDG_CONFIG_HOME or ~\.config)
$cfg = if ($env:XDG_CONFIG_HOME) { Join-Path $env:XDG_CONFIG_HOME "wordink" } else { "$HOME\.config\wordink" }
New-Item -ItemType Directory -Force $cfg | Out-Null
'{ "providers": [{ "provider": "groq", "keyEnv": "GROQ_API_KEY" }] }' |
  Set-Content -Encoding utf8 (Join-Path $cfg "gateway.json")

$env:GROQ_API_KEY = "gsk_…"
node $GW check
node $GW serve                 # foreground — leave it running in this window

# In a SECOND PowerShell window, repeat $env:GROQ_API_KEY and $GW, then:
node $GW tokens create test    # prints wdk_… once — save it
```

Two calls prove the chain (POSIX `curl`; in PowerShell use `curl.exe` — plain `curl` aliases `Invoke-WebRequest`):

```sh
# Providers the gateway will try, in fallback order
curl -H "Authorization: Bearer wdk_…" http://127.0.0.1:8941/v1/models
# PowerShell:  curl.exe -H "Authorization: Bearer wdk_…" http://127.0.0.1:8941/v1/models

# Any short WAV or m4a works — the repo ships one
curl -H "Authorization: Bearer wdk_…" \
     -F file=@packages/local/test/fixtures/hello-world.wav \
     http://127.0.0.1:8941/v1/audio/transcriptions
# PowerShell:  curl.exe -H "Authorization: Bearer wdk_…" -F "file=@packages\local\test\fixtures\hello-world.wav" http://127.0.0.1:8941/v1/audio/transcriptions
```

`{"text": "Hello, world"}` back means everything works end to end: token auth → provider key → upstream transcription → OpenAI-shaped response.

To also verify the runtime vocabulary loop, mint an admin token (`tokens create --admin agent`), `POST /v1/vocabulary` a term like `{"terms": ["Kurultai"]}`, then `GET /v1/vocabulary` and check it reports `effective`. Both routes are documented on the [Desktop apps](desktop.html) page.

## Reading failures

| Symptom | Cause |
|---|---|
| `check` lists no providers | The `keyEnv` variable isn't set in the gateway's environment — see the env-var note below |
| Empty `/v1/models` | No provider entry had a usable key at serve time |
| `401` | Missing, mistyped or revoked token (`tokens list` shows ids and revoked state, never values) |
| `403` | Device token on an admin route (`/v1/vocabulary`) — mint with `--admin` |
| `502` | Every configured provider failed — `serve` logs which one and why (dead key, rate limit, outage) |
| `504` | The 30 s chain deadline passed — providers too slow or unreachable |
| `cannot listen on …` | Port already bound — another gateway is running, or pick a different `port` |
| Config not found | Wrong path for your OS — see the table |

## Per-OS notes

| | Linux | macOS | Windows |
|---|---|---|---|
| Config | `~/.config/wordink/gateway.json` | same | `%USERPROFILE%\.config\wordink\gateway.json` |
| State (tokens, vocabulary) | `~/.local/state/wordink/` | same | `%USERPROFILE%\.local\state\wordink\` |
| Run as a service | systemd user service — `examples/gateway-systemd` | launchd, or a terminal | a terminal, or Task Scheduler |
| Notes | `XDG_CONFIG_HOME` / `XDG_STATE_HOME` respected | paths are the Unix defaults — non-idiomatic but stable | no `install -Dm755`: run via `node`; use `curl.exe` in PowerShell (`curl` aliases `Invoke-WebRequest`) |

**Environment variables are per-process.** A systemd user service or scheduled task does not see your shell's exports — the gateway's providers silently skip when their `keyEnv` resolves to nothing. On Linux, `examples/gateway-systemd` includes a wrapper that fills them from a secret store at start; on other OSes, set user-level environment variables or export them in the same session that runs `serve`.

**Serving other machines** needs `"host"` plus `"allowInsecureRemote": true` and a TLS terminator — without TLS, tokens and audio cross the network in cleartext. The details and the client-side base-URL quirks per app are on the [Desktop apps](desktop.html) page.

## What CI already covers

The upstream repo runs Rust tests, the TypeScript unit suites, browser end-to-end dictation tests (chromium, firefox and webkit through a fake microphone), the gateway's request-path tests, and Swift compile/test gates on every merge to `master`. What this smoke test adds is the part CI cannot do for you: your OS, your keys, your audio file, your eventual dictation app.
