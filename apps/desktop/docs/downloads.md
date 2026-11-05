# Downloads

WordInk desktop releases are hosted on GitHub Releases only. The release flow is in [`releasing.md`](../../../docs/releasing.md) and the pre-tag checks in [`release-checklist.md`](../../../docs/release-checklist.md).

## Download

| OS | Installer |
|---|---|
| Windows 10/11 (x64) | [`WordInk-Setup-windows-x64.exe`](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-Setup-windows-x64.exe): per user, no admin, Start Menu shortcut, uninstall from Apps & Features |
| macOS 15+ (Apple silicon) | [`WordInk-macos-arm64.dmg`](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-macos-arm64.dmg): drag WordInk to Applications |
| Linux x86_64 | [`WordInk-linux-x86_64.AppImage`](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-linux-x86_64.AppImage), [`wordink_amd64.deb`](https://github.com/duketopceo/WordInk/releases/latest/download/wordink_amd64.deb), [`wordink-x86_64.rpm`](https://github.com/duketopceo/WordInk/releases/latest/download/wordink-x86_64.rpm) |
| Linux aarch64 | [`WordInk-linux-aarch64.AppImage`](https://github.com/duketopceo/WordInk/releases/latest/download/WordInk-linux-aarch64.AppImage), [`wordink_arm64.deb`](https://github.com/duketopceo/WordInk/releases/latest/download/wordink_arm64.deb), [`wordink-aarch64.rpm`](https://github.com/duketopceo/WordInk/releases/latest/download/wordink-aarch64.rpm) |

The names carry no version, so these `releases/latest/download/` links always fetch the newest final release (GitHub's `latest` skips prereleases, so release candidates are only on their own release pages). They start with the first release after 1.0.0.

First launch of the unsigned builds: on Windows, SmartScreen's **More info > Run anyway**; on macOS, open WordInk once, then **System Settings > Privacy & Security > Open Anyway** (right-click > **Open** no longer skips Gatekeeper on macOS 15 and later).

On Linux, the `.deb` and `.rpm` install `/usr/bin/wordink`, the launcher entry and the systemd user unit, and update through the package manager (the app never replaces a package-managed executable). The AppImage bundles no libraries and does not update itself; it needs FUSE to run in place (otherwise run it with `--appimage-extract-and-run`). The one-line installer is the signed, self-updating Linux install.

## Install from the terminal

| OS | Command |
|---|---|
| Linux (x86_64, aarch64; glibc 2.39+, OpenSSL 3) | `curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh \| sh` |
| macOS (Apple silicon) | `curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install-macos.sh \| sh` |
| Windows (x64) | `irm https://github.com/duketopceo/WordInk/releases/latest/download/install.ps1 \| iex` |

## Release assets

Each `desktop-vX.Y.Z` release carries: the per-architecture Linux executables and tarballs, AppImages, `.deb` and `.rpm` packages, `wordink-V-macos-arm64.zip` and `WordInk-macos-arm64.dmg`, `wordink-V-windows-x64.zip` and `WordInk-Setup-windows-x64.exe`, the three one-line installers, a CycloneDX SBOM, `stable.json` (final releases) and `dev.json`, `SHA256SUMS` and its ed25519 signature `SHA256SUMS.sig`. The exact table is in [`releasing.md`](../../../docs/releasing.md).

## Channels

- **Stable:** the latest final `desktop-vX.Y.Z` release (`stable.json`).
- **Dev:** the rolling `desktop-dev` prerelease, whose `dev.json` the Dev-channel updater reads. Release candidates are prereleases.

## Signing and updates

- **Linux:** ed25519-verified by the installer and the app; the app updates itself.
- **macOS and Windows:** unsigned (Gatekeeper and SmartScreen warn on first launch); the app reports a newer release and links to it, and never installs it.

## Network

Allow `github.com` and its release asset host for downloads and update checks, and `huggingface.co` with its download CDN for local speech models. Model downloads go in ranged pieces and resume after a failure. Proxy variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`) are honored on every OS.
