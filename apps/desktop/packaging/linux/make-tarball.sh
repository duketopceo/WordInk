#!/usr/bin/env bash
# make-tarball.sh <dist> <arch>: packs wordink-<version>-linux-<arch>.tar.gz
# in <dist> from the executable there, plus install-release.sh (the unpacked
# installer the bootstrap, install.sh, runs after verifying the tarball) and the desktop files
# from this checkout. It runs BEFORE signing, in a job with no secrets, so the
# tarball carries no wordink.sig: SHA256SUMS lists the tarball and
# SHA256SUMS.sig signs that list (sign-linux.sh --sums), which is how a
# download of the tarball is verified. The bare executable's own signature is
# published beside it as wordink-<version>-linux-<arch>.sig.
#
# Environment: WORDINK_VERSION.
set -euo pipefail

dist=${1:?Usage: make-tarball.sh <dist> <x64|arm64>}
arch=${2:?Usage: make-tarball.sh <dist> <x64|arm64>}
: "${WORDINK_VERSION:?Set WORDINK_VERSION}"
here=$(cd "$(dirname "$0")" && pwd)
desktop=$(cd "$here/../.." && pwd)
name="wordink-$WORDINK_VERSION-linux-$arch"
[ -f "$dist/$name" ] || { echo "::error::$name is missing in $dist" >&2; exit 1; }
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
mkdir -p "$root"
install -m 755 "$dist/$name" "$root/wordink"
install -m 755 "$here/install-release.sh" "$root/install-release.sh"
install -m 644 "$here/wordink-signing.pub" "$root/wordink-signing.pub"
install -m 644 "$desktop/packaging/wordink.service" "$desktop/packaging/io.github.duketopceo.wordink.desktop" "$root/"
cp -R "$here/icons" "$root/icons"
install -m 644 "$desktop/LICENSE" "$desktop/THIRD_PARTY_NOTICES.md" "$root/"
# A reproducible archive: fixed order, owner and times.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='2026-01-01 00:00Z' \
  -C "$stage" -czf "$dist/$name.tar.gz" "$name"
echo "Packed $name.tar.gz."
