#!/usr/bin/env bash
# verify-release.sh <dist>: checks a finished release directory before it is
# uploaded, with no secrets: SHA256SUMS verifies against every file, lists
# exactly the files present (nothing published is unlisted, nothing listed is
# missing), SHA256SUMS.sig verifies against the public key, and each Linux
# executable's signature verifies too. Needs OpenSSL 3.
#
# WORDINK_SIGNING_PUBLIC_KEY_FILE overrides the public key file (tests only).
set -euo pipefail

die() { echo "::error::$*" >&2; exit 1; }

dist=${1:?Usage: verify-release.sh <dist>}
here=$(cd "$(dirname "$0")" && pwd)
public_key=${WORDINK_SIGNING_PUBLIC_KEY_FILE:-$here/wordink-signing.pub}
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
cd "$dist"

[ -f SHA256SUMS ] || die "SHA256SUMS is missing."
[ -f SHA256SUMS.sig ] || die "SHA256SUMS.sig is missing."
[ "$(wc -c < SHA256SUMS.sig | tr -d ' ')" = 64 ] || die "SHA256SUMS.sig is not 64 bytes."
sha256sum --check --strict --quiet SHA256SUMS || die "A file does not match SHA256SUMS."

find . -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort | awk '$0 != "SHA256SUMS" && $0 != "SHA256SUMS.sig"' > "$temporary/present"
awk '{ print substr($0, 67) }' SHA256SUMS | LC_ALL=C sort > "$temporary/listed"
diff -u "$temporary/listed" "$temporary/present" > /dev/null \
  || die "SHA256SUMS does not list exactly the release files."

sha=$(sha256sum SHA256SUMS | cut -c1-64)
printf 'wordink.sha256sums.v1\n%s\n' "$sha" > "$temporary/message"
openssl pkeyutl -verify -rawin -pubin -inkey "$public_key" \
  -in "$temporary/message" -sigfile SHA256SUMS.sig > /dev/null \
  || die "SHA256SUMS.sig does not verify against wordink-signing.pub."

checked=0
for sig in wordink-*-linux-x64.sig wordink-*-linux-arm64.sig; do
  [ -f "$sig" ] || continue
  file=${sig%.sig}
  [ -f "$file" ] || die "$sig has no executable beside it."
  sha=$(sha256sum "$file" | cut -c1-64)
  printf 'wordink.linux-artifact.v1\n%s\n' "$sha" > "$temporary/message"
  openssl pkeyutl -verify -rawin -pubin -inkey "$public_key" \
    -in "$temporary/message" -sigfile "$sig" > /dev/null \
    || die "$sig does not verify against wordink-signing.pub."
  checked=$((checked + 1))
done
[ "$checked" -gt 0 ] || die "No Linux executable signatures in $dist."
echo "Verified SHA256SUMS ($(wc -l < SHA256SUMS | tr -d ' ') files), its signature and $checked executable signature(s)."
