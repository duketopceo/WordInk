# WordInk dictation on Omarchy (Hyprland)

`wordink-dictd` is a small push-to-talk daemon: hold a key, speak, release, and cleaned-up text is typed into the focused window. Groq Whisper transcribes (with your vocabulary as a hint), a Groq LLM pass tidies it, and the offline `flow.py` rules take over if that pass is slow or down. It needs only a Groq key; a WordInk gateway is optional.

Plan: [`docs/plans/2026-10-28-0005-feat-standalone-linux-dictation-app-plan.md`](../../docs/plans/2026-10-28-0005-feat-standalone-linux-dictation-app-plan.md).

## Install

```sh
examples/omarchy-dictation/install.sh --dry-run          # see every step, change nothing
examples/omarchy-dictation/install.sh                    # install; Voxtype is not touched
$EDITOR ~/.config/wordink/dictd.env                      # GROQ_API_KEY=...  (file is mode 0600)
wordink-dictd doctor                                     # read-only checks, never prints the key
systemctl --user start wordink-dictd                     # try it alongside Voxtype (stop it with `stop`)
```

Add `--install-packages` to let the script run `sudo pacman -S --needed` for missing tools (`wtype`, `wl-clipboard`, `libnotify`, `uv`); without it the script only prints the command.

The script adds a `source =` line to `~/.config/hypr/hyprland.conf` between marker comments, and writes the binds to `~/.config/hypr/wordink-dictd.conf`. Before adding them it runs `wordink-dictd doctor` and stops if another tool already owns the key.

## Keys

| Keys | Action |
|---|---|
| hold `SUPER ALT, D` | record while held, transcribe on release |
| tap `SUPER ALT, S` | start, tap again to stop |

wisp keeps `SUPER, D`; `wordink-dictd` refuses that key. Change the keys in `~/.config/wordink/dictate.toml` (`[hotkey] hold = "..."`), then re-run the installer. `wordink-dictd binds` prints the lines.

## Switching from Voxtype, and back

```sh
examples/omarchy-dictation/install.sh --switch   # stops and disables Voxtype, starts WordInk
wordink-use-voxtype                              # one command back: stops WordInk, re-enables Voxtype
```

`--switch` records Voxtype's previous state in `~/.local/state/wordink/voxtype-before` and writes `~/.local/bin/wordink-use-voxtype` before it changes anything. It looks for a `voxtype*.service` user unit; set `VOXTYPE_UNIT=name.service` if yours differs. It never edits Voxtype's own config or binds.

Remove everything the installer added with `install.sh --uninstall` (your key file, vocabulary and `dictate.toml` stay).

## waybar

Merge [`waybar-module.jsonc`](waybar-module.jsonc) into your waybar config, add `custom/wordink` to a modules list, and append [`waybar-style.css`](waybar-style.css). The icon shows idle, recording, transcribing, error, or off when the daemon is not running. Clicking it toggles recording.

## Everyday use

```sh
wordink-dictd vocab add WordInk Hyprland   # terms Whisper should spell right
wordink-dictd last                         # the last cleaned transcript (also kept if insertion fails)
wordink-dictd status                       # daemon state as JSON
```

How text is inserted: `wtype`, then `ydotool` (ASCII only), then the clipboard with a notification. Terminals get a paste (clipboard plus `Ctrl+Shift+V`) and your previous clipboard is restored. Password-manager windows are never typed into. See `[insert]` in `dictate.toml` to change any of this.

## Troubleshooting

`wordink-dictd doctor` names what is missing and the fix. Logs: `journalctl --user -u wordink-dictd`. If `hyprctl` cannot be reached from the service, make sure Hyprland's session environment is imported into `systemd --user` (UWSM does this).

## Meeting mode

Long-form transcription of the mic and the system audio, ported from Voxtype's `meeting` command. Speakers are labelled by source: the mic is `You`, the system loopback (the other side of a call) is `Remote`. That is the same as Voxtype's default `simple` diarization, needs no extra provider, and uses the same `GROQ_API_KEY` as dictation. Telling several remote people apart (Voxtype's experimental `ml` backend) is not included.

```sh
wordink-dictd meeting start --title "Weekly sync"   # --source mic|loopback|both (default both)
wordink-dictd meeting pause; wordink-dictd meeting resume
wordink-dictd meeting stop                           # waits for the last chunk to transcribe
wordink-dictd meeting label latest Remote Alice      # rename a speaker
wordink-dictd meeting export latest --format md      # md, txt or json
wordink-dictd meeting list | show | delete
```

Meetings are stored in `~/.local/share/wordink/meetings/<id>/meeting.json` (0600). Audio is not kept. Audio is cut into 30 s chunks; silent chunks are skipped, the rest go to Groq Whisper. With speakers (not headphones) the mic hears the call too, so wear headphones for clean `You` / `Remote` separation.

## Hyprland Lua configs

Hyprland 0.55 and later can run a Lua config (Omarchy's `~/.config/hypr/hyprland.lua`). When that file exists, `hyprland.conf` is not read, so `install.sh` writes `~/.config/hypr/wordink-dictd.lua` (Omarchy `o.bind` lines with an absolute path to `wordink-dictd`, since the compositor's PATH usually lacks `~/.local/bin`) and appends a guarded `dofile` block to `bindings.lua` (or `hyprland.lua` if there is no `bindings.lua`). `--uninstall` removes both. Print the lines yourself with `wordink-dictd binds --format lua --exe ~/.local/bin/wordink-dictd`.
