# WordInk desktop

WordInk is voice dictation for Linux, macOS and Windows. Hold a key, speak, and
it pastes into whatever app you're in. It is built on
[Hex](https://github.com/kitlangton/Hex) (MIT).

## How it works

- hold the shortcut (Alt+Space, or Option+Space on a Mac keyboard), speak, then let go
- WordInk transcribes with Groq, OpenRouter or a local model, and pastes the text
- with a local model, nothing leaves your machine
- experimental features live in Labs, at the bottom of the sidebar

## Install

WordInk runs on Linux (x86_64 or aarch64), macOS 15+ on Apple silicon, and 64-bit Windows.

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

**Release candidates** install the same way with `WORDINK_INSTALL_BASE=https://github.com/duketopceo/WordInk/releases/download/desktop-v1.0.0-rc.N` set before the command.

More docs are in [`docs/`](docs/README.md).

## Contributing

Issues and pull requests are welcome: see [CONTRIBUTING.md](../../CONTRIBUTING.md).
Report bugs at <https://github.com/duketopceo/WordInk/issues>; `wordink diagnostics`
prints a summary that is safe to paste.

MIT licensed.
