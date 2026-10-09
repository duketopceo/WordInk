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
| `--token` | `WORDINK_DEVICE_TOKEN` | required — mint with `wordink-gateway token new` |

The token is read from env/flag only and never logged.

## CLI

```bash
wordink transcribe meeting.m4a            # transcript to stdout
wordink transcribe a.wav --language en --prompt "Kurultai, Omarchy"
wordink dictate                           # mic → Enter/Ctrl-C → transcript
wordink dictate --seconds 10 | wl-copy    # fixed-length capture to clipboard
wordink models                            # what the gateway serves
```

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
`list_models()` → the gateway's model ids.

## Develop

```bash
cd sdks/python
uv sync --extra mic
uv run pytest
uv run wordink --help
```
