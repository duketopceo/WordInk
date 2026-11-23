#!/usr/bin/env bash
# release-asset-names.sh: shared GitHub Release asset basenames for the desktop
# workflow, signing checks, and packaging scripts. Human-facing installers use
# WordInk-for-* so they sort near the top; versioned wordink-<version>-* files
# stay for the updater and one-line installers; wordink-package-* marks .deb
# and .rpm as package-manager paths.
set -euo pipefail

# Human-facing installers (releases/latest/download/<name>).
export WORDINK_ASSET_DMG=WordInk-for-Mac-Apple-Silicon.dmg
export WORDINK_ASSET_WINDOWS_SETUP=WordInk-for-Windows-Setup.exe
export WORDINK_ASSET_APPIMAGE_X86_64=WordInk-for-Linux-x86_64.AppImage
export WORDINK_ASSET_APPIMAGE_AARCH64=WordInk-for-Linux-ARM64.AppImage

# Linux distribution packages (no version in the name; package metadata holds it).
export WORDINK_ASSET_DEB_AMD64=wordink-package-debian-amd64.deb
export WORDINK_ASSET_DEB_ARM64=wordink-package-debian-arm64.deb
export WORDINK_ASSET_RPM_X86_64=wordink-package-fedora-x86_64.rpm
export WORDINK_ASSET_RPM_AARCH64=wordink-package-fedora-aarch64.rpm

# One-line installers (exact names; consumed by curl | sh and SHA256SUMS).
export WORDINK_ASSET_INSTALL_SH=install.sh
export WORDINK_ASSET_INSTALL_MACOS_SH=install-macos.sh
export WORDINK_ASSET_INSTALL_PS1=install.ps1

# Legacy installer names: WordInk 1.0.0–1.0.2 hard-code these in the in-app
# updater's download link. New releases also upload byte-identical copies under
# these names (not listed in SHA256SUMS; see compat-asset-aliases.sh).
export WORDINK_LEGACY_DMG=WordInk-macos-arm64.dmg
export WORDINK_LEGACY_WINDOWS_SETUP=WordInk-Setup-windows-x64.exe
export WORDINK_LEGACY_APPIMAGE_X86_64=WordInk-linux-x86_64.AppImage
export WORDINK_LEGACY_APPIMAGE_AARCH64=WordInk-linux-aarch64.AppImage