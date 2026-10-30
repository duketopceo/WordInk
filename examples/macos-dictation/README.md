# WordInk dictation on macOS

Hold a hotkey, speak, release: cleaned text appears in the focused app. Same Groq Whisper and LLM
cleanup pipeline as the Linux and Windows daemons. Plan: `docs/plans/2026-10-28-0400-feat-macos-dictation-plan.md`.

> Status: nothing here has run on a real Mac. The tests run `install.sh` on Linux against fake `uv`,
> `launchctl` and `uname`. Read the plan's M9 checks before the first real run, and treat "works on macOS"
> as unproven until they are done. Nothing is signed or notarized; there is no `.app` bundle.

## What `install.sh` does

Needs macOS and [uv](https://docs.astral.sh/uv/). It fails early with an install hint if `uv` is missing.

| Command | Effect |
|---|---|
| `./install.sh` | `uv tool install` the package with the `macos` extra, write the launcher `~/.local/bin/wordink-dictd-mac`, render `~/Library/LaunchAgents/io.wordink.dictd.plist` and load it with `launchctl bootstrap`. No other app is touched. |
| `./install.sh --dry-run` | Print every step, change nothing. Add it to any other flag. |
| `./install.sh --no-start` | Install everything but do not load the agent now. It still starts at your next login. |
| `./install.sh --switch "App Name"` | As above, then quit the dictation app you name and write `~/.local/bin/wordink-use-previous`. Without `--switch` nothing else is touched. |
| `./install.sh --uninstall` | Unload and remove the agent, launcher, rollback script and tool. **Keeps** your Keychain key, vocabulary and config. |

Running it twice changes nothing the second time: files are only rewritten when their content differs, and the
agent is only reloaded when the plist changed.

## Why a launcher

macOS ties the Microphone and Accessibility permissions to a program's identity. The LaunchAgent runs one fixed
path, `~/.local/bin/wordink-dictd-mac`, so the grant belongs to a file you can find. The launcher only `exec`s
the uv-installed entry point. After a Python or uv upgrade, `wordink-dictd doctor` prints which executable needs
the grants and whether each one is on.

## The LaunchAgent

Label `io.wordink.dictd`: runs at login, restarts only after a failure (not after a clean quit), waits at least
30 seconds between restarts, and runs as an interactive process. launchd keeps a single instance per label, and
the app also refuses to start if its socket already answers. The app writes a rotating log to
`~/Library/Logs/wordink/dictd.log`; launchd's own stdout and stderr go to `launchd.log` in the same folder.
The plist holds no key.

## First run

```sh
~/.local/bin/wordink-dictd set-key     # hidden prompt; stored in your login Keychain
~/.local/bin/wordink-dictd doctor      # read-only checks
```

When macOS asks, allow Microphone and Accessibility for the launcher. Default keys: hold `Ctrl+Option+D`,
toggle `Ctrl+Option+S` (`[hotkey]` in `~/Library/Application Support/wordink/dictate.toml`).

## Going back

```sh
~/.local/bin/wordink-use-previous
```

Written by `--switch`. It unloads the WordInk agent and reopens the app that was running when you switched.
Run it before `--uninstall` if you want that app back.

## Tests

```sh
cd sdks/python && uv run pytest tests/desktop/test_mac_installer.py
shellcheck examples/macos-dictation/install.sh
```
