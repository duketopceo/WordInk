# Local API

The running WordInk desktop app serves a small HTTP API on your own computer.
Scripts, editor plugins and other apps use it to transcribe audio, start a
dictation, and run the same commands as the `hex` control CLI. The code is
`src/local_api.rs`; the `@wordink/desktop` package (`packages/desktop`) is a
typed Node client for it.

The app has to be running. Nothing here starts it (`hex start` does).

## Security

- The server listens on `127.0.0.1` only. Another machine cannot reach it.
- Every request needs the bearer token from the discovery file. The file is
  readable only by your user (mode `0600` on macOS and Linux, your user's
  folder ACL on Windows).
- Any process running as the same user can read that file, so any such
  process can use the API. Other users on the machine cannot. Treat the token
  like a password and never log it or send it anywhere.
- A new token is generated each time the app starts.

## Discovery

The app writes `local-api.json` to its data folder while it runs and removes
it on exit:

| OS | Folder |
| --- | --- |
| Linux | `$XDG_DATA_HOME/wordink/` (default `~/.local/share/wordink/`) |
| macOS | `~/Library/Application Support/wordink/` |
| Windows | `%APPDATA%\wordink\` |

`HEX_APPLICATION_SUPPORT_DIR`, when set in the app's environment, replaces
that folder. A file left behind by a crash is stale: check that `pid` is
alive and the port answers before trusting it.

```json
{ "port": 41823, "token": "<64 hex characters>", "apiVersion": "2", "pid": 4242 }
```

| Field | Meaning |
| --- | --- |
| `port` | TCP port on `127.0.0.1`. The base URL is `http://127.0.0.1:<port>`. |
| `token` | Bearer token for this run of the app. |
| `apiVersion` | Version of this API (see Versioning). |
| `pid` | Process id of the app. |

Hosts that embed the engine in-process get the same fields as a JSON object
(`type`, `url`, `token`, `apiVersion`, `pid`) instead of a file. Clients that
accept a ready-made `url` can use it as is.

## Requests

Send `Authorization: Bearer <token>` on every request. A missing or wrong
token answers `401` with an empty body. Bodies need a `Content-Length`;
chunked bodies are refused. Errors are JSON, `{"code": "<kebab-case-code>"}`.

```sh
DIR="${XDG_DATA_HOME:-$HOME/.local/share}/wordink"
PORT=$(jq -r .port "$DIR/local-api.json")
TOKEN=$(jq -r .token "$DIR/local-api.json")
BASE="http://127.0.0.1:$PORT"
AUTH="Authorization: Bearer $TOKEN"
```

## Public endpoints

These are stable within an `apiVersion`.

### `GET /health`

```sh
curl -s -H "$AUTH" "$BASE/health"
# {"version":"1.0.0","apiVersion":"2"}
```

### `GET /capabilities`

What this app, on this OS, supports.

```sh
curl -s -H "$AUTH" "$BASE/capabilities"
# {"audioFormats":["audio/wav"],"partialTranscripts":false,"serviceCapture":false,
#  "streams":true,"developerControl":false,"control":true}
```

`serviceCapture` is true only where `/dictations` works (macOS today).

### `GET /models`

Lists the transcription models: `id`, `name`, `installed`, `verified`,
`managed`, `downloadBytes`, `languages`, `supportsLanguageDetection`,
`source`. Optional `?language=<code>` decides what `installed` means.

```sh
curl -s -H "$AUTH" "$BASE/models?language=en"
```

`POST /models/<id>/prepare[?language=<code>]` installs and loads a model and
streams progress as server-sent events. It answers `409 prepare-busy` while
another preparation runs.

### `POST /transcriptions`

Transcribe one WAV file with an installed model. The body is the raw WAV
(`Content-Type: audio/wav`, up to 64 MiB). The query picks the model:
`?model=<id>` (required, an `id` from `/models`) and `&language=<code>`.

```sh
curl -s -H "$AUTH" -H "Content-Type: audio/wav" \
  --data-binary @clip.wav \
  "$BASE/transcriptions?model=parakeet_v2&language=en"
# {"transcript":"hello world","durationMs":1520}
```

Errors: `415 unsupported-audio` (not a supported WAV), `400 invalid-audio`,
`409 model-not-ready` (install it first), `429 queue-full`,
`413 resource-exhausted`.

### `POST /dictations` and `/dictations/<id>/...`

Start a dictation the app records from its own microphone, then finish or
cancel it. Available only when `/capabilities` reports `serviceCapture: true`;
otherwise every route answers `404 service-capture-unavailable`.

`POST /dictations` with `{"source": "my-tool"}` (`Content-Type:
application/json`) answers `201`:

```json
{ "id": 7, "ownerToken": "<token>", "sampleRate": 16000 }
```

Each `/dictations/<id>/...` request needs the bearer token and the dictation's
owner token in `X-Hex-Dictation-Token`. A missing or wrong owner token
answers `404 dictation-not-found`.

| Route | Does |
| --- | --- |
| `POST /dictations/<id>/finish` | Stop and transcribe. `200 {"transcript","durationMs"}` |
| `POST /dictations/<id>/cancel` | Discard it. `204` |
| `GET /dictations/<id>/levels` | Server-sent events of `{"rmsDb","peakDb"}`; once per dictation |
| `GET /dictations/<id>/audio` | Raw little-endian `f32` mono samples at `sampleRate` |
| `POST /dictations/<id>/heartbeat` | Keep it alive while you work. `204` |

```sh
START=$(curl -s -H "$AUTH" -H "Content-Type: application/json" \
  -d '{"source":"my-tool"}' "$BASE/dictations")
ID=$(echo "$START" | jq -r .id)
OWNER=$(echo "$START" | jq -r .ownerToken)
curl -s -X POST -H "$AUTH" -H "X-Hex-Dictation-Token: $OWNER" \
  "$BASE/dictations/$ID/finish"
```

### `POST /control`

Run a `hex` control command (`status`, `dictation`, `settings`, `model`, ...).
It is the same protocol the CLI uses; see [`cli.md`](cli.md) for every
command. The body is JSON: the control `version` (currently `4`), a
`command`, and that command's arguments. The reply is JSON too. A command the
app refuses comes back as a `200` with a `status` of `error`; an HTTP error
means the request never reached the app.

```sh
curl -s -H "$AUTH" -H "Content-Type: application/json" \
  -d '{"version":4,"command":"status"}' "$BASE/control"

curl -s -H "$AUTH" -H "Content-Type: application/json" \
  -d '{"version":4,"command":"dictation","action":"toggle"}' "$BASE/control"
```

A request whose `version` the app does not speak gets a `version-mismatch`
error reply, not a crash.

## Internal endpoints

`/dev/*` (developer control, debug builds), `/profile/*` (process vitals) and
`/streams` (live transcription, described in [`streams.md`](streams.md)) are
not part of this contract and may change or disappear in any release.

## Versioning

`apiVersion` (from `/health` and the discovery file) names the version of the
public endpoints above. A change that breaks them bumps it: removing or
renaming a route, field or error code, or changing what one means. Adding a
route or a response field does not. A client should refuse an `apiVersion` it
does not know instead of guessing. The control protocol has its own `version`
inside `/control`, which the app checks separately.

A test in `src/local_api.rs` (`documented_public_routes_are_routed`) fails if a
route listed here stops being handled, so keep that list in step with this
page.
