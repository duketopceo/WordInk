# wordink — Python tooling for agents and CLIs

Dictation through a `wordink-gateway` — or any OpenAI-compatible transcriptions
endpoint. Provider fallback, shared vocabulary and rate limiting live
server-side; this package only speaks the protocol.

Two entry points ship from one package:

| Command | What it does |
|---|---|
| `wordink` | CLI: `transcribe` a file, `dictate` from the mic, `models` to list |
| `wordink-mcp` | MCP stdio server: `transcribe` + `list_models` tools for agents |

## Install

```bash
uv tool install 'wordink[mic] @ ./sdks/python'   # mic extra for `dictate`
# or from the checkout:
cd sdks/python && uv sync
```

## Config

| Flag | Env | Default |
|---|---|---|
| `--gateway` | `WORDINK_GATEWAY_URL` | `http://127.0.0.1:8941` |
| `--token` | `WORDINK_DEVICE_TOKEN` (dictation) / `WORDINK_ADMIN_TOKEN` (vocab) | required — mint with `wordink-gateway tokens create [--admin] <label>` |

The token is read from env/flag only and never logged. Dictation calls resolve
`--token` > `WORDINK_DEVICE_TOKEN` > `WORDINK_ADMIN_TOKEN`; vocabulary calls
resolve `--token` > `WORDINK_ADMIN_TOKEN` > `WORDINK_DEVICE_TOKEN` (a device
token earns a clean `403` — the vocabulary routes need admin scope).

## CLI

```bash
wordink transcribe meeting.m4a            # transcript to stdout
wordink transcribe a.wav --language en --prompt "Kurultai, Omarchy"
wordink dictate                           # mic → Enter/Ctrl-C → transcript
wordink dictate --seconds 10 | wl-copy    # fixed-length capture to clipboard
wordink models                            # what the gateway serves

wordink vocab list                        # effective vocabulary (config + runtime), JSON
wordink vocab add Kurultai Omarchy        # add terms; outcomes per term
wordink vocab add X --dry-run             # report outcomes without persisting
wordink vocab remove Kurultai             # remove a runtime term
wordink vocab remove Kurultai --via-post  # same, through POST (proxies that strip DELETE bodies)
```

`WordInkError` keeps the gateway's machine-readable error code on `.code`
(`term_pinned`, `vocabulary_full`, `insufficient_permissions`, `term_not_found`,
`too_many_terms`) and the HTTP status on `.status`, so callers can branch on the
failure mode instead of parsing the message.

## MCP

`wordink-mcp` is a stdio server. Point any MCP host at it; the env vars above
must be set in the host's environment:

```json
{
  "mcpServers": {
    "wordink": {
      "command": "wordink-mcp",
      "env": {
        "WORDINK_DEVICE_TOKEN": "wdk_…",
        "WORDINK_GATEWAY_URL": "http://127.0.0.1:8941"
      }
    }
  }
}
```

Tools: `transcribe(file_path, language?, prompt?)` → transcript text;
`list_models()` → the gateway's model ids;
`vocab_list()` / `vocab_add(terms, dry_run?)` / `vocab_remove(term, via_post?)` → the
vocabulary API's JSON responses. The vocab tools need `WORDINK_ADMIN_TOKEN`
(either env var satisfies the server gate; a device token 403s on vocab calls).
Use `transcribe(prompt=…)` for one-off corrections and `vocab_add` only for
terms that recur across sessions — never add secrets or sensitive dictated
content; terms are sent verbatim to providers on every call.

## Develop

```bash
cd sdks/python
uv sync --extra mic
uv run pytest
uv run wordink --help
```
