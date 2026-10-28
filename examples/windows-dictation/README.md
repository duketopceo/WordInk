# WordInk dictation on Windows (cyberpowa)

Hold a hotkey, speak, release: cleaned text appears in the focused window. Same Groq Whisper + LLM
cleanup pipeline as the Linux daemon. Plan: `docs/plans/2026-10-28-0200-feat-windows-dictation-plan.md`.

> Status: nothing here runs by itself and it has not been run on a real machine. The installer registers
> `wordink-dictd-tray` (the app wiring from plan W7b); read the plan's W10 checks before the first real run.

## What `install.ps1` does

| Command | Effect |
|---|---|
| `.\install.ps1` | `uv tool install` the package (with the `windows` extra) and register a logon task for the current user (limited rights, restarts 3x at 1 min, one instance). The legacy app is **not** touched. |
| `.\install.ps1 -DryRun` | Print every step, change nothing. Add it to any other flag. |
| `.\install.ps1 -Switch` | As above, then stop the legacy Groq Flow app, move its `Groq Flow.lnk` startup shortcut to `%LOCALAPPDATA%\wordink\legacy-backup`, and write `wordink-use-legacy.ps1`. |
| `.\install.ps1 -Uninstall` | Remove the task, the rollback script and the tool. **Keeps** your Groq key, vocabulary, config and the moved legacy shortcut. |

Options: `-Source <path-or-requirement>` (default: this checkout's `sdks/python`, else the GitHub repo) and `-NoStart`.

## Going back to the legacy app

```powershell
powershell -File "$env:LOCALAPPDATA\wordink\wordink-use-legacy.ps1"
```

It disables the WordInk task, stops its process, restores `Groq Flow.lnk` and starts the legacy app.
`.\install.ps1 -Switch` returns to WordInk: it re-enables the task, starts it, and moves the legacy app aside again.

## First run

```powershell
wordink-dictd set-key     # hidden prompt; stored in Windows Credential Manager
wordink-dictd doctor      # read-only checks
```

Default keys: hold `Ctrl+Alt+D`, toggle `Ctrl+Alt+S` (`[hotkey]` in `%APPDATA%\wordink\dictate.toml`).

## Tests

```powershell
Install-Module Pester -MinimumVersion 5.5 -Scope CurrentUser -Force -SkipPublisherCheck
Invoke-Pester .\install.Tests.ps1
```

Pester mocks every system change; CI runs it on `windows-latest` with PSScriptAnalyzer.
