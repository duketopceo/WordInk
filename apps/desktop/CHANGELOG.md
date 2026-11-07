# Changelog

Release notes for the WordInk desktop app. Each release is published from the
section whose heading is its version (`## X.Y.Z`); a release candidate
(`-rc.N`) uses its own `## X.Y.Z-rc.N` section if there is one, else the
`## X.Y.Z` section it is a candidate for. The release workflow refuses to start
when the section is missing. See [`docs/releasing.md`](../../docs/releasing.md).

## Unreleased

## 1.0.2

- **Meeting button in the window.** The sidebar always shows "Start meeting";
  while recording it reads "In a meeting" with "Stop & transcribe" and the
  elapsed time. Pressing it turns meetings on if they were off.
- **Connect OpenRouter.** A button in Settings and onboarding (and
  `wordink keys connect openrouter`) opens OpenRouter in the browser, mints a
  key named WordInk and saves it to the system keyring.
- **Better meeting accuracy, optional.** With OpenRouter connected, onboarding
  offers Nemotron 3.5 Transcribe as the meeting engine. If a cloud meeting
  request fails or the key is missing, that meeting is transcribed locally
  instead, so a recording is never lost.
- **Defaults that fit the machine (new installs).** WordInk picks Parakeet v3
  on the Apple Neural Engine when available, then a usable GPU, then the CPU,
  and recommends a cloud engine only on low-powered machines. The cloud
  fallback uses whichever key is set. Existing settings are kept.
- `wordink transcribe-file` help now says it uses the cloud engine.

## 1.0.1

- **Installers on every release.** Alongside the one-line installers, each
  release now carries a per-user Windows installer
  (`WordInk-Setup-windows-x64.exe`, no admin, Start Menu shortcut and
  uninstaller), a macOS disk image (`WordInk-macos-arm64.dmg`, drag to
  Applications), and for Linux x86_64 and aarch64 an AppImage, a `.deb` and an
  `.rpm`. Their names carry no version, so
  `https://github.com/duketopceo/WordInk/releases/latest/download/<name>`
  always fetches the newest. All are listed in the signed `SHA256SUMS`. The
  Windows and macOS builds are still unsigned: on Windows choose **More info >
  Run anyway**, on macOS **System Settings > Privacy & Security > Open
  Anyway**.
- **The right icon everywhere.** Every icon size is now generated from one
  WordInk vector source: the Windows `.ico` carries all nine sizes (16 to 256,
  including 20, 24 and 40 for high-DPI taskbars), Linux gets a scalable SVG
  beside the PNGs, and the macOS app icon no longer uses Hex's hexagon.
- **A disk image that looks like WordInk.** The macOS `.dmg` volume carries
  WordInk's icon.
- **Updates open the right download.** On Windows, an installed WordInk now
  opens the Setup installer when a new version exists, and on macOS the disk
  image; an AppImage opens the AppImage download. `.deb`, `.rpm` and Arch
  installs keep telling you to update through your package manager. Leftover
  Hex wording in updater messages is gone.
- **Download page.** The docs site now has the download links and the
  first-launch steps for the unsigned Windows and macOS builds.

## 1.0.0

WordInk is local voice dictation for your desktop. Hold the shortcut, speak,
let go, and the text is pasted into whatever app you are in. Speech is
transcribed on your device; after the first-run model download, nothing leaves
your machine.

- **Linux (x86_64, aarch64):** signed and self-updating. Releases carry an
  ed25519 signature that the installer and the app's updater verify before
  anything is installed, and the app keeps itself up to date from the release
  feed.
- **macOS (Apple silicon) and Windows (x64):** unsigned for 1.0. Gatekeeper and
  SmartScreen will warn on first launch. Updates are download-only: the app
  tells you when a newer release exists and links to it, and never installs one
  itself.
- **Requirements:** Linux with glibc 2.39 or newer (Ubuntu 24.04 or equivalent)
  and a desktop session; macOS 15 or newer on Apple silicon; 64-bit Windows.
  A microphone, and free disk space for the speech model.
- **Verifying a download:** every release lists all of its files in
  `SHA256SUMS`, and `SHA256SUMS.sig` is an ed25519 signature over that list.
  Each file also has a build-provenance attestation
  (`gh attestation verify <file> -R duketopceo/WordInk`).

### Features

- Terminal auto-paste: text goes into terminal apps with the right shortcut.
- Usage stats: words, speaking speed, streak and top apps.
- Vocabulary suggestions from your own corrections.
- Bluetooth earbuds work for dictation, and microphones have clean, renameable names.
- Shorter first run, with a Finish later option.
- Bigger bottom indicator pill (1.25x).
- `wordink compare` runs one recording through several engines, and uploads can be FLAC.
- Copy diagnostics in Settings (the only opt-in data sharing) and release download counts from GitHub.
- `[record] device` setting for `wordink-dictd`.
- GPU transcription through Vulkan on Linux.
- OpenRouter as a first-party engine (transcription endpoint, 29 models) with p50/p95 latency in Usage, and a separate meeting engine setting.
- Developer mode: full debug logs with a live view, and clear coded errors for microphone, earbud, engine and paste failures.

### Fixes

- A GPU run that was slower than CPU is now detected and reported instead of silently kept.

### Release and install

- One-line installers: `install.sh` (Linux), `install-macos.sh` (macOS) and `install.ps1` (Windows), published as release assets.
- Tag-driven release workflow with an install smoke test on Linux x86_64 and aarch64, macOS arm64 and Windows x64. Prereleases follow the rolling `desktop-dev` channel.
- See [`docs/releasing.md`](../../docs/releasing.md) and [`docs/release-checklist.md`](../../docs/release-checklist.md).

### Removed

- The iOS app is paused and removed from the active tree; it is preserved on branch `archive/ios-2026-11-03`.
