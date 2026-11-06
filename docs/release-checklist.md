# Release checklist

Run this on real machines for a release candidate before tagging the final
release. The automated gate and smoke jobs prove a draft installs and reports
the right version on clean runners; they cannot prove the microphone, the
shortcut, pasting, a real user session or a real update. A person does that here.

Machines (one of each; generic roles):

- Linux laptop (a Wayland or X11 desktop with a user systemd session, Bluetooth
  earbuds, and a built-in microphone)
- Linux desktop (the other CPU architecture from the laptop, if you have one)
- Mac (Apple silicon)
- Windows PC (x64)

Use a clean user account or a machine without WordInk where you can. Record the
tag, the date and the result for each box. Any failure means a new `-rc.N`.

## Per machine

Do these in order. Substitute the candidate's tag where a step names a release;
a release candidate is a prerelease, so `releases/latest/download` does not
serve it. Install it with the candidate's own base URL:
`https://github.com/duketopceo/WordInk/releases/download/desktop-vX.Y.Z-rc.N`
Export it once per shell as `WORDINK_INSTALL_BASE` (`$env:WORDINK_INSTALL_BASE` in
PowerShell) and every command below uses it; in these commands `<base>` is that URL.

1. **Install with the one-liner.**
   - Linux: `curl -fsSL <base>/install.sh | sh`
   - macOS: `curl -fsSL <base>/install-macos.sh | sh`
   - Windows (PowerShell): `irm <base>/install.ps1 | iex`

   Expect: no sudo or admin prompt, no error, a final message that says what to
   do next. `wordink --version` prints the candidate's version (macOS: run
   `~/Applications/WordInk.app/Contents/MacOS/wordink --version`; Windows:
   `%LOCALAPPDATA%\Programs\WordInk\wordink.exe --version`).
2. **Re-run the installer.** It says WordInk is already installed and up to date
   and changes nothing. `--force` (Linux and macOS) or `$env:WORDINK_FORCE='1'`
   (Windows) reinstalls.
3. **First run.** The app opens; first run shows welcome, demo, permissions,
   engine, shortcut, a test dictation and done. No text says "Hex". Quit midway
   once and relaunch: it resumes at the same step. "Finish later" on the
   permissions and engine steps lands in the app with a visible "Set up
   dictation" state.
4. **Unsigned notice.** macOS and Windows: the installer said once that the app
   is unsigned. macOS: grant any Gatekeeper or Login Items approval it asks for.
   Windows: SmartScreen may warn on first open from Explorer.
5. **Microphone.**
   - The built-in microphone dictates.
   - Linux laptop: connect Bluetooth earbuds, pick them in Settings, dictate;
     then disconnect them mid-session and confirm the app falls back or says
     clearly that the device is gone (no hang, no silent empty text).
   - A USB or headset microphone, if present.
   - macOS and Windows: the permission prompt appears once and a denied
     permission produces a readable message, not silence.
6. **Shortcut.** The shortcut chosen in first run starts and stops dictation
   from another app. Change it in Settings and confirm the new one works and the
   old one does not. On Linux, check the shortcut on the session type in use
   (Wayland or X11).
7. **Paste.** Dictate into a text editor: the text lands at the cursor. Dictate
   into a terminal emulator: it pastes there too (terminals paste with a
   different key than editors). Check the clipboard is restored afterwards.
8. **Engines.**
   - Cloud: enter a Groq key, dictate, confirm the text arrives. Remove the key
     and confirm a clear message.
   - Local: download a local model; the download shows progress and counts as
     finished only after its checksum verifies. Dictate with the network off.
9. **Launch at login.** Log out and in (or reboot): WordInk is running without
   being opened. macOS: it appears in Login Items. Windows: it is in the Start
   menu and starts at login. Linux: `systemctl --user is-active wordink` says
   `active`.
10. **Settings survive.** Quit, reinstall with `--force`, and confirm settings,
    history and downloaded models are still there.
11. **Update (Linux only).** Settings shows the update state for the channel
    (dev for a candidate). With a newer release available, the app downloads it,
    verifies the signature and restarts into it. Check `wordink --version`
    afterwards and that the previous version is kept. macOS and Windows only
    show that an update exists and link to the release page; confirm the link
    opens the right release.
12. **Copy diagnostics.** The privacy pane says nothing is sent automatically;
    "Copy diagnostics" copies a plain-text summary with no audio, transcripts,
    paths or keys.
13. **Uninstall.**
    - Linux: `curl -fsSL <base>/install.sh | sh -s -- uninstall`
    - macOS: `curl -fsSL <base>/install-macos.sh | sh -s -- uninstall`
    - Windows: `$env:WORDINK_UNINSTALL = '1'; irm <base>/install.ps1 | iex`

    Expect: the app stops and is removed, settings and history stay. Then
    install again, and uninstall with `--purge` (`$env:WORDINK_PURGE = '1'` on
    Windows): after one confirmation it also removes settings, history and
    models.

### Update test per OS

Install the previous final release (for the first run of this test, 1.0.0), then
publish or serve the candidate as the newer version and open Settings. The
update state must say a newer version exists and the button must open the right
file:

- **Windows (installed from Setup):** the link opens
  `WordInk-Setup-windows-x64.exe`. Run it over the old install: it keeps
  settings and history and `wordink.exe --version` reports the new version.
- **macOS (dmg install):** the link opens `WordInk-macos-arm64.dmg`. Mounting it
  shows `WordInk.app`, the Applications link and the WordInk volume icon. Drag
  the app over the old one, confirm the replace, and open it: the new version
  runs and settings survive.
- **Linux one-line install:** the app downloads, verifies the signature and
  restarts into the new version (step 11).
- **Linux AppImage:** the link opens the AppImage download; replace the file and
  run it.
- **Linux `.deb`, `.rpm`, Arch package:** the app shows the package-manager
  command and installs nothing itself.

Extra for the Linux laptop and desktop:

- Run the installer over a machine that has a local source build of WordInk: the
  release build replaces it and settings, history, keys and models are kept.
- Check an unsupported system gets a one-line message (for example, run the
  installer in a container with an older glibc) and that nothing is installed.

## Release candidate, then final

1. Merge the release notes (`## X.Y.Z` in `apps/desktop/CHANGELOG.md`) to
   `master`.
2. The owner tags the commit `desktop-vX.Y.Z-rc.N` and pushes the tag
   ([docs/releasing.md](releasing.md)).
3. The owner approves the `release` environment when the run pauses at
   `sign-linux`.
4. Confirm the run is green through `gate`, `sign-linux`, `draft` and all four
   `smoke` jobs, and that the candidate is a published prerelease that is not
   marked latest.
5. Run the per-machine list above on each machine. Fix anything on `master` and
   cut the next `-rc.N`; repeat until one candidate passes everywhere.
6. The owner tags the same commit `desktop-vX.Y.Z`, pushes it, and approves the
   `release` environment again.
7. Confirm the final release is marked latest and that
   `https://github.com/duketopceo/WordInk/releases/latest/download/install.sh`,
   `install-macos.sh` and `install.ps1` resolve. Run the one-liners once from
   the real `latest` URL on one machine per OS.

## Owner actions

- Approve the `release` environment for every candidate and the final tag.
- Make the four `Desktop` jobs required status checks on `master`, turn off
  "Allow force pushes" on `master`, and add a tag ruleset so only the owner can
  create `desktop-v*` tags (limit the `release` environment to those tags).
- Run this checklist on each machine and tag the final release.
- macOS and Windows builds are unsigned until paid developer accounts exist;
  say so wherever those downloads are offered.
