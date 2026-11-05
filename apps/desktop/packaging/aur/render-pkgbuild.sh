#!/usr/bin/env bash
# render-pkgbuild.sh <version> <SHA256SUMS> [PKGBUILD]: prints the wordink-bin
# PKGBUILD for a desktop release, filling in the template beside this script
# (or the one given) from that release's SHA256SUMS. Verify SHA256SUMS.sig
# first (docs/releasing.md); this script trusts the list it is given. pacman
# allows no hyphen in pkgver, so 1.2.0-rc.1 becomes 1.2.0rc1, which pacman
# sorts before 1.2.0. Nothing publishes the result to the AUR yet.
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }
version=${1:?Usage: render-pkgbuild.sh <version> <SHA256SUMS> [PKGBUILD]}
sums=${2:?Usage: render-pkgbuild.sh <version> <SHA256SUMS> [PKGBUILD]}
template=${3:-$(cd "$(dirname "$0")" && pwd)/PKGBUILD}
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$' \
  || die "$version is not X.Y.Z or X.Y.Z-rc.N"
sha() {
  local name=$1 found
  found=$(awk -v name="$name" 'substr($0, 67) == name { print substr($0, 1, 64) }' "$sums")
  printf '%s\n' "$found" | grep -Eq '^[0-9a-f]{64}$' || die "$sums does not list $name"
  printf '%s' "$found"
}
x64=$(sha "wordink-$version-linux-x64.tar.gz")
arm64=$(sha "wordink-$version-linux-arm64.tar.gz")
pkgver=${version/-rc./rc}
sed -e "s/@PKGVER@/$pkgver/" -e "s/@VERSION@/$version/" \
  -e "s/@SHA256_X64@/$x64/" -e "s/@SHA256_ARM64@/$arm64/" "$template"
