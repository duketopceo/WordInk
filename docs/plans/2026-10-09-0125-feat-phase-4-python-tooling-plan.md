# Phase 4 — `wordink` Python tooling (CLI + MCP over the gateway)

> **Goal.** Agents get `transcribe` as an MCP tool and shells get `wordink
> transcribe|dictate` — both riding the gateway's OpenAI-compatible contract, so
> fallback/vocabulary/rate-limits stay server-side and nothing is reimplemented.
> Source: `docs/brainstorms/2026-10-09---phase-4-python-tooling-requirements.md`.

## Key technical decisions

- **KTD1 — `sdks/python/`, not `packages/`.** `pnpm-workspace.yaml` globs
  `packages/*`; a Python tree there pollutes the JS workspace. New top level
  keeps ecosystems separated (mirrors `crates/`, `legacy/`).
- **KTD2 — `httpx` only for HTTP, no `openai` dep.** The call is one multipart
  POST; the `openai` package adds a client abstraction to wrap a wrapper. `httpx`
  `files=` posts the exact OpenAI shape (R1).
- **KTD3 — `sounddevice` for mic (R2).** PortAudio bindings, works on
  Wayland/PulseAudio here and macOS/Windows. Lazy-import inside `dictate` so the
  CLI/MCP stay dependency-light for headless use.
- **KTD4 — official `mcp` package for the server (R3).** `mcp.server.stdio` +
  `@server.call_tool` — stdio transport is what every host launches.
- **KTD5 — no Python in pnpm CI.** `sdks/python` gets its own tests via `uv run
  pytest`; wiring a Python job into CI is a follow-up, not this change.

## Units

- **U1 — package skeleton + transcribe core.** `sdks/python/pyproject.toml`
  (uv, py>=3.11, deps: httpx, sounddevice, mcp, pytest dev-dep), `src/wordink/
  __init__.py`, `client.py` (`transcribe(file, *, gateway, token, language,
  prompt) -> str` — multipart POST, Bearer, parse `{text}`, raise
  `WordInkError` with API message on non-2xx), config resolution
  (flag > env > default `http://127.0.0.1:8941/api/wordink`), and the
  `wordink` console script: `transcribe <file>` (R1) + `dictate [--seconds N]`
  (R2, records to in-memory WAV via sounddevice + `wave`, Ctrl-C/Enter stops).
- **U2 — `wordink-mcp` stdio server (R3).** `transcribe(file_path)` and
  `list_models` tools; reads the same env config; never logs the token.
- **U3 — tests + docs (R7).** `tests/test_client.py` with `respx`-free httpx
  MockTransport cases (happy path, 401 error surfaces, flag/env/default config
  precedence); `sdks/python/README.md` covering install/env/CLI/MCP-host config;
  ROADMAP Phase 4 row → "in progress".

## Verification

- `cd sdks/python && uv sync && uv run pytest` green; `uv run wordink --help`,
  `uv run wordink-mcp` importable; mypy-free but strict-ish (annotations on the
  public surface). Live check against the running gateway at :8941 if a device
  token is available.
- Repo guards unchanged: `pnpm install` still ignores `sdks/` (glob check).

## Definition of done

Units committed on a branch off master; tests green; README complete; PR open,
CI green. Publishing (PyPI) is out of scope — deferred like npm.
