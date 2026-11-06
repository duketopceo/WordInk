# Hardware verification: macOS and Windows

CI builds and tests the desktop app on Windows x64 and macOS arm64, but a CI runner has no microphone, no signed-in user session and no real keyboard. A person runs this list once on a real Mac (Apple silicon) and a real Windows x64 PC for each release candidate. It extends the per-machine steps in [release-checklist.md](release-checklist.md) with the checks those two systems need.

Record for each machine: the tag, the date, the OS version, and pass, fail or skipped for every step. Any failure means a new `-rc.N` (or a bug against `master` when no release is in progress).

## Before you start

- Use a user account that has never run WordInk, or uninstall with purge first (release checklist step 13).
- Have a text editor, a terminal emulator and a web page with a password field open.
- Have a Groq API key ready for the cloud steps. Have the network available for the model download, then a way to turn it off.
- Install from the candidate's installer: `WordInk-Setup-windows-x64.exe` or `WordInk-macos-arm64.dmg` from the release page, not a local build.

## 1. Download and open

| Step | macOS | Windows | Expected |
|---|---|---|---|
| 1.1 | Open the `.dmg` | Run `WordInk-Setup-windows-x64.exe` | The dmg window shows WordInk and an Applications shortcut. The Windows installer asks for no administrator rights. |
| 1.2 | Drag WordInk to Applications, open it | Finish the installer | macOS blocks the unsigned app once; System Settings › Privacy & Security › Open Anyway opens it. Windows SmartScreen may warn; More info › Run anyway opens it. |
| 1.3 | Check the Dock and Finder icon | Check the Start menu, desktop shortcut, taskbar and Apps & Features icons | The WordInk logo, sharp at every size, nowhere the Hex logo. |
| 1.4 | `/Applications/WordInk.app/Contents/MacOS/wordink --version` | `%LOCALAPPDATA%\Programs\WordInk\wordink.exe --version` | The candidate's version. Windows: Apps & Features shows the same version. |

## 2. First run (auto-detect onboarding)

| Step | Check | Expected |
|---|---|---|
| 2.1 | First launch | The first run opens on its own. No text says "Hex". |
| 2.2 | Keyboard drawing and shortcut | macOS shows Option and Command keys and an Option-based default shortcut. Windows shows Ctrl, Win and Alt, and an Alt-based default. |
| 2.3 | Microphone capsule | Names this computer's real input, not a demo name. The menu lists every input. |
| 2.4 | Engine step | The line names this machine's GPU (or says CPU only) and its memory. The recommendation matches: local Parakeet with a usable GPU, cloud otherwise. The other choice is one click away. |
| 2.5 | Language | Matches the system language when WordInk supports it, else English. |
| 2.6 | Appearance | Follows the system light or dark setting; switch it and the window follows. |
| 2.7 | Other tools | With another dictation tool installed, the shortcut step names it. |
| 2.8 | Quit midway and relaunch | The first run resumes at the same step. |
| 2.9 | Finish later | Lands in the app with a visible "Set up dictation" row. |

## 3. Permissions

| Step | macOS | Windows | Expected |
|---|---|---|---|
| 3.1 | Microphone prompt | Microphone privacy switch | Asked once. Denying it shows a readable message with the way to fix it, never silence. |
| 3.2 | Accessibility (paste) and Input Monitoring | n/a | Each is asked for with an explanation; the app works once granted without a reinstall. |

## 4. Dictation

Run each with the cloud engine (a Groq key) and again with the local model (after its download finishes, with the network off).

| Step | Target | Expected |
|---|---|---|
| 4.1 | Text editor | Hold the shortcut, speak, release: the text lands at the cursor. |
| 4.2 | Terminal emulator (Terminal, iTerm2, Windows Terminal) | The text lands at the prompt. |
| 4.3 | A browser text field | The text lands in the field. |
| 4.4 | A password field | Nothing is typed or pasted, and the clipboard holds nothing new. |
| 4.5 | Clipboard | After each dictation the previous clipboard contents are back. |

## 5. Indicator styles

In Settings › General › Recording indicator, pick each style in turn and dictate once with it. Expected: each style appears while recording, follows the screen with the focused window (try a second display if there is one), and disappears afterwards.

## 6. Cancel

| Step | Check | Expected |
|---|---|---|
| 6.1 | Bind a Cancel shortcut in Settings › General, start a dictation, press it | The recording stops, nothing is typed, the cancel sound plays. |
| 6.2 | Start a dictation, press Escape twice quickly | Same as 6.1. |
| 6.3 | Press Escape once while recording, then keep talking | The recording continues. |
| 6.4 | Press the Cancel shortcut while idle | Nothing happens. |

## 7. Usage and History

| Step | Check | Expected |
|---|---|---|
| 7.1 | Usage pane after the dictations above | Counts and the per-engine latency rows match what was run. |
| 7.2 | History: turn the network off, dictate with the cloud engine, turn it back on, press Retry | The failed entry transcribes and shows the text. |
| 7.3 | History: Retry with the local engine | Works offline. |

## 8. Update

| Step | macOS | Windows | Expected |
|---|---|---|---|
| 8.1 | Settings shows the update state for the channel | Same | A newer release is reported with its notes. |
| 8.2 | Press Update | Press Update | Unsigned builds open the new `.dmg` or `WordInk-Setup-windows-x64.exe` download, not a zip. Settings, history and models survive installing it over the old copy. |

## 9. Uninstall

| Step | macOS | Windows | Expected |
|---|---|---|---|
| 9.1 | Quit, drag WordInk to the Bin | Apps & Features › WordInk › Uninstall | The app is gone. Settings and history stay unless you purge. |

## What CI covers instead

The `Desktop` workflow builds and runs the unit and golden tests on `windows-latest` and `macos-26`. The release workflow's dry-run (`desktop-release.yml`, `dry_run: true`) builds every installer, installs it on a clean runner and checks the reported version. Neither can prove the steps above that need a microphone, a real keyboard, permissions or a signed-in desktop.
