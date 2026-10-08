# WordInk legacy app (Windows dictation daemon)

The original WordInk dictation app: a Windows tray daemon that records while you hold a hotkey, transcribes with Groq Whisper, and pastes the text at your cursor. It descends from [groq_flow](https://github.com/ParthJain18/groq_flow).

It stays runnable until WordInk Desktop (see the repo's `ROADMAP.md`) replaces it.

```bash
uv sync
uv run wordink --onboard   # setup wizard at http://localhost:18981
uv run wordink             # tray daemon
```

On Windows, `run.bat` runs it from a console, and `start_silent.vbs` runs it at boot. Settings and data live in `~/.groq_flow/`. Configuration comes from `.env` (copy `.env.example`). See `QUICKSTART.md` for the full walkthrough.
