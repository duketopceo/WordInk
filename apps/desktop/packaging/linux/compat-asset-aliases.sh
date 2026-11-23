#!/usr/bin/env bash
# compat-asset-aliases.sh <dist>: hard-links the four human-facing installers
# under the legacy names shipped in WordInk 1.0.x–1.0.2 so their in-app updater
# download button still resolves on GitHub. These aliases are uploaded with the
# release but are not part of SHA256SUMS (signing finished earlier).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck disable=SC1091
. "$here/release-asset-names.sh"

dist=${1:?Usage: compat-asset-aliases.sh <dist>}
pair() {
  local canonical=$1 legacy=$2
  [ -f "$dist/$canonical" ] || { echo "::error::missing $canonical for legacy alias $legacy" >&2; exit 1; }
  ln -f "$dist/$canonical" "$dist/$legacy"
  echo "Alias $legacy -> $canonical"
}
pair "$WORDINK_ASSET_DMG" "$WORDINK_LEGACY_DMG"
pair "$WORDINK_ASSET_WINDOWS_SETUP" "$WORDINK_LEGACY_WINDOWS_SETUP"
pair "$WORDINK_ASSET_APPIMAGE_X86_64" "$WORDINK_LEGACY_APPIMAGE_X86_64"
pair "$WORDINK_ASSET_APPIMAGE_AARCH64" "$WORDINK_LEGACY_APPIMAGE_AARCH64"