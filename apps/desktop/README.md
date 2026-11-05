# WordInk desktop

WordInk is voice dictation for Linux, macOS and Windows. Hold a key, speak, and
it pastes into whatever app you're in. It is built on
[Hex](https://github.com/anomalyco/hex) (MIT).

## How it works

- hold the shortcut (Alt+Space, or Option+Space on a Mac keyboard), speak, then let go
- WordInk transcribes with Groq, OpenRouter or a local model, and pastes the text
- with a local model, nothing leaves your machine
- experimental features live in Labs, at the bottom of the sidebar

## Download

- **Windows 10/11 (x64):** [WordInk-Setup-windows-x64.exe](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-Setup-windows-x64.exe), a per-user installer (no admin)
- **macOS 15+ (Apple silicon):** [WordInk-macos-arm64.dmg](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-macos-arm64.dmg), drag WordInk to Applications
- **Linux x86_64:** [AppImage](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-linux-x86_64.AppImage) · [.deb](https://github.com/duketopceo/WordInk/releases/latest/download/wordink_amd64.deb) · [.rpm](https://github.com/duketopceo/WordInk/releases/latest/download/wordink-x86_64.rpm)
- **Linux aarch64:** [AppImage](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-linux-aarch64.AppImage) · [.deb](https://github.com/duketopceo/WordInk/releases/latest/download/wordink_arm64.deb) · [.rpm](https://github.com/duketopceo/WordInk/releases/latest/download/wordink-aarch64.rpm)

These start with the first release after 1.0.0. The Windows and macOS builds are unsigned, so the first launch warns once:

- Windows SmartScreen: **More info**, then **Run anyway**.
- macOS: open WordInk once, then **System Settings > Privacy & Security > Open Anyway**. (Right-click > **Open** no longer skips the warning on macOS 15 and later.)

## Install

WordInk runs on Linux (x86_64 or aarch64), macOS 15+ on Apple silicon, and 64-bit Windows. The one-line installers below also set up autostart, verify the download and, on Linux, keep WordInk updated from signed releases.

**Linux**

```sh
curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh | sh
```

Needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40, Arch and newer) and OpenSSL 3. The installer checks the release's ed25519 signature, starts WordInk as a user service, and keeps it updated from signed releases.

**macOS**

```sh
curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install-macos.sh | sh
```

Installs `WordInk.app` into `~/Applications` and starts it at login. WordInk for macOS is not notarized yet: macOS asks you to allow microphone and accessibility access, and updates are shown in the app as a download link.

**Windows (PowerShell)**

```powershell
irm https://github.com/duketopceo/WordInk/releases/latest/download/install.ps1 | iex
```

Installs per user (no admin) and starts at login. WordInk for Windows is not code-signed yet, so SmartScreen may warn the first time you open it from Explorer (More info › Run anyway).

**Uninstall**

- Linux: `curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh | sh -s -- uninstall` (add `--purge` to delete settings, history and models)
- macOS: `curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install-macos.sh | sh -s -- uninstall`
- Windows: `& ([scriptblock]::Create((irm https://github.com/duketopceo/WordInk/releases/latest/download/install.ps1))) -Uninstall`

To install a specific release instead of the latest, set `WORDINK_INSTALL_BASE=https://github.com/duketopceo/WordInk/releases/download/desktop-vX.Y.Z` before the command.

More docs are in [`docs/`](docs/README.md). To build on the running app, see the [local API](docs/local-api.md).

## Contributing

Issues and pull requests are welcome: see [CONTRIBUTING.md](../../CONTRIBUTING.md).
Report bugs at <https://github.com/duketopceo/WordInk/issues>; `wordink diagnostics`
prints a summary that is safe to paste.

MIT licensed.
