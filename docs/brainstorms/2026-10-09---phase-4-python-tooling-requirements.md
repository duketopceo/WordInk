# Phase 4 — Python tooling for agents and CLIs: requirements

Date: 2026-10-09 · Status: settled · Tier: Lightweight

## Origin

ROADMAP Phase 4: "Python SDK for agents and CLIs." Brainstorm found the premise's
attachment gap: the gateway's OpenAI-compatible contract already makes every Python
HTTP client a working dictation client (`openai` + `base_url` + `wdk_` token gets
transcription, fallback, shared vocabulary, and rate limiting for free). A
`wordink` library wrapping `openai-python` duplicates what the protocol already
provides.

The uncovered surfaces are the ones an HTTP client can't reach: **live mic
dictation in a shell** and **transcription as an agent tool call**.

## Decision

Ship one Python package, `wordink`, uv-installable, that wraps the gateway only:

- `wordink transcribe <file>` — file → stdout text
- `wordink dictate` — mic → stdout text (push-to-… — records until Ctrl+C/Enter,
  then transcribes the clip)
- `wordink-mcp` — MCP stdio server exposing a `transcribe` tool so agents
  (wisp, kurultai, Devin, Claude Code) transcribe audio files through the gateway
  without writing client code

No provider adapters, no key handling beyond the device token: provider fallback,
vocabulary, and rate limits stay server-side. Works against any OpenAI-compatible
transcriptions endpoint, not just wordink-gateway.

## Actors

- **A1** — agents on this machine (and any MCP host) needing audio → text as a tool
- **A2** — humans/scripts composing `wordink transcribe` / `wordink dictate` in
  pipelines
- **A3** — external Python users pointing the same tools at any
  OpenAI-compatible endpoint

## Requirements

- **R1** — `wordink transcribe <file> [--language X] [--prompt S]` POSTs the file
  to `{base}/v1/audio/transcriptions` (multipart `file`, `model`, `response_format=json`,
  optional `language`/`prompt`), prints `text` to stdout, exits nonzero on error
  with the API message on stderr.
- **R2** — `wordink dictate` captures mic audio (sounddevice/PortAudio), records
  until SIGINT or Enter, encodes WAV in memory, transcribes via R1, prints text.
  `--seconds N` for fixed-length capture.
- **R3** — `wordink-mcp` speaks MCP over stdio (official `mcp` Python SDK):
  tool `transcribe(file_path)` → transcript text; tool `list_models` → the
  gateway's model list. Same auth/config as the CLI.
- **R4** — config: `--gateway`/`--token` flags override env `WORDINK_GATEWAY_URL`
  (default `http://127.0.0.1:8941/api/wordink`) and `WORDINK_DEVICE_TOKEN`
  (required, fail fast if unset). Token is never logged.
- **R5** — `pyproject.toml` (uv): console scripts `wordink` and `wordink-mcp`;
  `uv tool install` gives both. Python >= 3.11.
- **R6** — live at `sdks/python/` (new top level; `packages/*` is the pnpm glob).
- **R7** — docs: a section in the docs site or `sdks/python/README.md` covering
  install, env config, CLI examples, and MCP host config.

## Key flows

- **F1** — agent: MCP host configured with `wordink-mcp` → `transcribe` tool call
  with a local audio path → transcript text back.
- **F2** — human: `wordink dictate | wl-copy` — mic → text → clipboard.
- **F3** — script: `for f in *.m4a; do wordink transcribe "$f" > "$f.txt"; done`.

## Acceptance examples

- **AE1** — `WORDINK_DEVICE_TOKEN=wdk_… wordink transcribe hello.m4a` prints the
  transcript; wrong token → exit 1 + error text.
- **AE2** — `wordink dictate --seconds 3` records 3s, prints the transcript.
- **AE3** — MCP `transcribe` call from a host returns the same transcript as AE1.

## Out of scope

- No provider adapters or direct provider calls (fallback/vocab stay server-side).
- No streaming partials to stdout in v1 (batch per clip).
- No translation endpoint (gateway doesn't expose one).
- No PyPI publish machinery yet — local `uv tool install` path first.
