## What changed
<!-- One or two sentences. -->

## Why
<!-- The problem, not the solution. -->

## Checks
- [ ] `uv sync` passes
- [ ] `python -m compileall wordink` passes
- [ ] `.env.example` updated if any config var was added/renamed

## If this touches the dictation hot path (audio, engine, groq_service, text_inserter)
- [ ] Latency impact noted (target stays ~300ms on whisper-large-v3-turbo)
- [ ] Noise-reduction / AI-cleanup defaults still off unless the PR is explicitly about them
- [ ] Tested in both a GUI app (clipboard paste) and a terminal (fallback typing)

## If this touches onboarding (`wordink/onboarding/`)
- [ ] Wizard still opens at `http://localhost:18981`
- [ ] Mic VU meter, key visualizer, and API-key verification each still work

## If this touches Windows boot/tray (`tray.py`, `start_silent.vbs`, `setup*`)
- [ ] Tray and silent-boot behavior verified on Windows
