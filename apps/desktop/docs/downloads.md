# Downloads

WordInk desktop releases are hosted on GitHub Releases only. The release flow is in [`releasing.md`](../../../docs/releasing.md) and the pre-tag checks in [`release-checklist.md`](../../../docs/release-checklist.md).

## Install

| OS | Command |
|---|---|
| Linux (x86_64, aarch64; glibc 2.39+, OpenSSL 3) | `curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh \| sh` |
| macOS (Apple silicon) | `curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install-macos.sh \| sh` |
| Windows (x64) | `irm https://github.com/duketopceo/WordInk/releases/latest/download/install.ps1 \| iex` |

## Release assets

Each `desktop-vX.Y.Z` release carries: the per-architecture Linux executables and tarballs, `wordink-V-macos-arm64.zip`, `wordink-V-windows-x64.zip`, the three installers, a CycloneDX SBOM, `stable.json` (final releases) and `dev.json`, `SHA256SUMS` and its ed25519 signature `SHA256SUMS.sig`. The exact table is in [`releasing.md`](../../../docs/releasing.md).

## Channels

- **Stable:** the latest final `desktop-vX.Y.Z` release (`stable.json`).
- **Dev:** the rolling `desktop-dev` prerelease, whose `dev.json` the Dev-channel updater reads. Release candidates are prereleases.

## Signing and updates

- **Linux:** ed25519-verified by the installer and the app; the app updates itself.
- **macOS and Windows:** unsigned (Gatekeeper and SmartScreen warn on first launch); the app reports a newer release and links to it, and never installs it.

## Network

Allow `github.com` and its release asset host for downloads and update checks, and `huggingface.co` with its download CDN for local speech models. Model downloads go in ranged pieces and resume after a failure. Proxy variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY`) are honored on every OS.
