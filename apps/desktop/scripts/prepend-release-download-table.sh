#!/usr/bin/env bash
# prepend-release-download-table.sh <changelog-body>: prints a Download table
# for GitHub release notes, then the body from release-notes.sh / the changelog.
# Environment: WORDINK_TAG, WORDINK_REPO (owner/name).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../packaging/linux/release-asset-names.sh
. "$here/../packaging/linux/release-asset-names.sh"

body=${1:?Usage: prepend-release-download-table.sh <changelog-body>}
: "${WORDINK_TAG:?Set WORDINK_TAG}"
: "${WORDINK_REPO:?Set WORDINK_REPO}"
base="https://github.com/$WORDINK_REPO/releases/download/$WORDINK_TAG"
latest="https://github.com/$WORDINK_REPO/releases/latest/download"

cat <<MD
## Download

| OS | Installer |
| --- | --- |
| macOS (Apple silicon) | [$WORDINK_ASSET_DMG]($base/$WORDINK_ASSET_DMG) |
| Windows (x64) | [$WORDINK_ASSET_WINDOWS_SETUP]($base/$WORDINK_ASSET_WINDOWS_SETUP) |
| Linux (x86_64) | [$WORDINK_ASSET_APPIMAGE_X86_64]($base/$WORDINK_ASSET_APPIMAGE_X86_64) |
| Linux (ARM64) | [$WORDINK_ASSET_APPIMAGE_AARCH64]($base/$WORDINK_ASSET_APPIMAGE_AARCH64) |

Linux one-line install: \`curl -fsSL $latest/$WORDINK_ASSET_INSTALL_SH | sh\` ·
macOS: \`$latest/$WORDINK_ASSET_INSTALL_MACOS_SH\` ·
Windows: \`$latest/$WORDINK_ASSET_INSTALL_PS1\`

Packages: [$WORDINK_ASSET_DEB_AMD64]($base/$WORDINK_ASSET_DEB_AMD64),
[$WORDINK_ASSET_DEB_ARM64]($base/$WORDINK_ASSET_DEB_ARM64),
[$WORDINK_ASSET_RPM_X86_64]($base/$WORDINK_ASSET_RPM_X86_64),
[$WORDINK_ASSET_RPM_AARCH64]($base/$WORDINK_ASSET_RPM_AARCH64).

The other assets on this release (\`wordink-*\` executables and archives, \`install.*\`, \`stable.json\`, \`dev.json\`, \`SHA256SUMS\`) are for the in-app updater and verified installs.

MD
printf '%s\n' "$body"