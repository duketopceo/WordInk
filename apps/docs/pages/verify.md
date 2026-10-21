# Verify your gateway

One command proves the whole chain on any machine the gateway runs on — Linux, macOS or Windows: config resolves, `serve` binds, a device token authenticates, `/v1/models` lists, a multipart transcription round-trips, the vocabulary routes write and read, and a revoked token gets refused. `wordink-gateway verify` runs it all against a loopback fake provider inside a temp sandbox — no provider keys, no config file, and it never touches your real token store.

The gateway is a plain HTTP server. It needs no microphone, audio-device or accessibility permission anywhere — the dictation app that points at it owns those permissions on its own machine.

## The smoke test

You need Node 20+ and a repo checkout (until `@wordink/server` is on npm).

```sh
# Build the CLI
pnpm install --frozen-lockfile
pnpm --filter @wordink/server build

# Offline — proves every mechanic with no keys:
node packages/server/dist/bin/wordink-gateway.js verify

# Live — additionally transcribes the bundled hello-world.wav through each
# provider entry that resolves a real key in your gateway.json:
export GROQ_API_KEY=gsk_…        # PowerShell: $env:GROQ_API_KEY = "gsk_…"
node packages/server/dist/bin/wordink-gateway.js verify --live
```

Each check prints `PASS`, `FAIL` with the cause, or `SKIP`; exit code is 0 only when nothing failed. `--live` reads your real `gateway.json` for provider entries (read-only — use `--config <path>` to point at a different file) and runs one isolated `serve` per entry, so a dead provider later in your fallback order cannot hide behind a healthy first entry. Tokens mint only into a temp store.

## Reading failures

| Symptom | Cause |
|---|---|
| `FAIL check` | The sandbox's own fake key failed to resolve — report it; this is never caused by your real environment (offline `verify` injects its own). With `--live`, `FAIL live <provider>` instead means that provider's real key/model/endpoint failed |
| Empty `/v1/models` | No provider entry had a usable key at serve time |
| `401` | Missing, mistyped or revoked token (`tokens list` shows ids and revoked state, never values) |
| `403` | Device token on an admin route (`/v1/vocabulary`) — mint with `--admin` |
| `502` | Every configured provider failed — `serve` logs which one and why (dead key, rate limit, outage) |
| `504` | The 30 s chain deadline passed — providers too slow or unreachable |
| `cannot listen on …` | Port already bound — another gateway is running, or pick a different `port` |
| Config not found | Wrong path for your OS — see the table; `verify` itself needs no config offline |
| `FAIL live <provider>` | That provider's key, model or endpoint failed on a real transcription — check the entry in `gateway.json` |

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

The upstream repo runs this same `verify` command on Linux, macOS and Windows runners on every merge — plus Rust tests, the TypeScript unit suites, browser end-to-end dictation tests, the gateway's request-path tests, and Swift compile/test gates. What a run on your machine adds is the part CI cannot do for you: your OS flavor, your keys, your dictation app.
