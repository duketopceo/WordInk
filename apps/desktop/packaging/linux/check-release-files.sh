#!/usr/bin/env bash
# check-release-files.sh <dist>: fails unless <dist> holds exactly the files a
# desktop release is built from, before signing: no file missing, none extra.
# The release workflow runs it in sign-linux (before the key is used) and in
# the dry run. Signatures, manifests and SHA256SUMS come later and are not
# expected here. Holds no secrets.
#
# Environment: WORDINK_VERSION.
set -euo pipefail

dist=${1:?Usage: check-release-files.sh <dist>}
: "${WORDINK_VERSION:?Set WORDINK_VERSION}"
v=$WORDINK_VERSION
expected=$(printf '%s\n' \
  "wordink-$v-linux-x64" "wordink-$v-linux-x64.tar.gz" \
  "wordink-$v-linux-arm64" "wordink-$v-linux-arm64.tar.gz" \
  "wordink-$v-windows-x64.zip" "wordink-$v-macos-arm64.zip" \
  "wordink-$v.sbom.cdx.json" \
  install.sh install-macos.sh install.ps1 \
  WordInk-Setup-windows-x64.exe WordInk-macos-arm64.dmg \
  WordInk-linux-x86_64.AppImage WordInk-linux-aarch64.AppImage \
  wordink_amd64.deb wordink_arm64.deb wordink-x86_64.rpm wordink-aarch64.rpm | LC_ALL=C sort)
found=$(find "$dist" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)
if [ "$expected" != "$found" ]; then
  echo "::error::unexpected release files (- expected, + found)" >&2
  diff <(echo "$expected") <(echo "$found") >&2 || true
  exit 1
fi
echo "All $(echo "$expected" | wc -l | tr -d ' ') release files are present."
