# AGENTS.md — WordInk

WordInk is a sub-second Windows voice-dictation daemon + embeddable Python SDK
powered by Groq Whisper Turbo (`whisper-large-v3-turbo`). It also serves as a
voice frontend for Computer-Using Agents (CUAs) via temperature-scaled
probabilistic centering. Repo name is `WordInk` (capital W/I); the Python
package / pip name is lowercase `wordink`.

## Commands

```bash
uv sync
uv pip install -e .
uv run wordink --onboard   # browser wizard at http://localhost:18981
uv run wordink             # system-tray daemon
```

There is no test suite or linter configured. Verify a change with
`uv sync` plus a compile check: `python -m compileall wordink`.
On Windows, silent boot run is `wscript.exe start_silent.vbs`.

## Architecture invariants

- Hot path is raw audio → Groq Whisper Turbo → paste. No LLM hop in the
  default path; `ENABLE_NOISE_REDUCTION` and `ENABLE_AI_CLEANUP` default off
  in README for latency (`.env.example` still shows `true` — treat README as
  intended default, do not "fix" by enabling).
- `wordink/engine.py` owns the live listen/transcribe loop;
  `wordink/groq_service.py` owns all Groq API calls;
  `wordink/audio.py` owns capture; `wordink/text_inserter.py` owns paste
  (clipboard injection with bracketed-typing fallback in terminals);
  `wordink/tray.py` owns the Windows tray; `wordink/telemetry/` is local-only
  SQLite (no phoning home); `wordink/onboarding/server.py` is the `:18981`
  setup wizard.
- Config flows `.env` → `wordink/config.py`. Every var the app reads must also
  exist in `.env.example` with a safe default. Never commit a real `.env`
  (`.gitignore` covers `.env`, `config.json`, `*.db`, `recordings/`).
- `groq-flow` console entry in `pyproject.toml` is a legacy alias for the
  pre-rebrand name. Keep it until the old docs/installs are gone.

## Traps

- Requires Python `>=3.12` (`uv` manages it via `.python-version`). `pyaudio`
  needs system audio libraries; on a fresh machine that is the first install
  failure, not the code.
- Hotkey uses `keyboard`-module syntax (`HOTKEY=alt+d` recommended). Some
  hotkeys need elevated privileges on Windows — prefer the onboarding key
  visualizer over hand-editing `.env`.
- Paste behavior differs by target: GUI apps get clipboard injection, terminals
  (Ghostty, Cascadia, Windows Terminal) get the fallback path. Test the app
  you actually dictate into.
- Do not hand-edit `uv.lock`; regenerate via `uv sync` / `uv lock`.

## What not to touch without asking

- `start_silent.vbs`, `setup*.bat` / `setup*.ps1`, boot/startup behavior.
- Telemetry schema in `wordink/telemetry/` (local user data).
- Default branch `master` protections and any workflow scope — owned repo, but
  CI/Dependabot/protection changes go through a PR, not direct push.

## Domain vocabulary

- CUA = Computer-Using Agent; probabilistic centering = power-scaling a VLM
  spatial heatmap (`P^alpha`) then taking the center of mass for sub-icon
  click accuracy. Details live in README § "Architecture: WordInk & CUAs".
