#!/usr/bin/env bash
# sign-linux.sh [--sums] <dist>: signs each Linux executable in <dist>
# (wordink-<version>-linux-<x64|arm64>) with WordInk's ed25519 release key,
# WORDINK_LINUX_SIGNING_KEY (PEM), and writes the raw 64-byte signature beside
# it as <file>.sig. The signed message is
#   "wordink.linux-artifact.v1\n<sha256 of the file>\n"
# WordInk's updater (src/updater/linux.rs) and install-release.sh verify it
# with the public key in wordink-signing.pub, which this script checks every
# signature against first, so a mismatched key fails the run before anything
# publishes. Needs OpenSSL 3 (ed25519 raw signing).
#
# With --sums it instead signs <dist>/SHA256SUMS (write-sums.sh) with the same
# key and writes the raw 64-byte signature as <dist>/SHA256SUMS.sig. That
# message has its own domain, so a binary signature can never pass as a
# checksum-list signature or the reverse:
#   "wordink.sha256sums.v1\n<sha256 of the SHA256SUMS file>\n"
# The Linux bootstrap installer and verify-release.sh recompute it the same way.
#
# WORDINK_SIGNING_PUBLIC_KEY_FILE overrides the public key file (tests only).
#
# Runs only in the release workflow's sign-linux job, on artifacts it treats
# as data: it never builds, installs or executes them.
set -euo pipefail

die() { echo "::error::$*" >&2; exit 1; }

mode=binaries
if [ "${1:-}" = --sums ]; then mode=sums; shift; fi
dist=${1:?Usage: sign-linux.sh [--sums] <dist>}
here=$(cd "$(dirname "$0")" && pwd)
public_key=${WORDINK_SIGNING_PUBLIC_KEY_FILE:-$here/wordink-signing.pub}
: "${WORDINK_LINUX_SIGNING_KEY:?Set WORDINK_LINUX_SIGNING_KEY}"
: "${WORDINK_VERSION:?Set WORDINK_VERSION}"
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
(umask 077 && printf '%s\n' "$WORDINK_LINUX_SIGNING_KEY" > "$temporary/key.pem")
if [ "$mode" = sums ]; then
  file="$dist/SHA256SUMS"
  [ -f "$file" ] || die "$file is missing; run write-sums.sh first."
  sha=$(sha256sum "$file" | cut -c1-64)
  printf 'wordink.sha256sums.v1\n%s\n' "$sha" > "$temporary/message"
  openssl pkeyutl -sign -rawin -inkey "$temporary/key.pem" -in "$temporary/message" -out "$file.sig" \
    || die "Could not sign SHA256SUMS with WORDINK_LINUX_SIGNING_KEY."
  [ "$(wc -c < "$file.sig" | tr -d ' ')" = 64 ] || die "The signature of SHA256SUMS is not 64 bytes."
  openssl pkeyutl -verify -rawin -pubin -inkey "$public_key" \
    -in "$temporary/message" -sigfile "$file.sig" > /dev/null \
    || die "WORDINK_LINUX_SIGNING_KEY does not match wordink-signing.pub, the key WordInk trusts."
  echo "Signed SHA256SUMS ($sha)."
  exit 0
fi
signed=0
for arch in x64 arm64; do
  file="$dist/wordink-$WORDINK_VERSION-linux-$arch"
  [ -f "$file" ] || continue
  sha=$(sha256sum "$file" | cut -c1-64)
  printf 'wordink.linux-artifact.v1\n%s\n' "$sha" > "$temporary/message"
  openssl pkeyutl -sign -rawin -inkey "$temporary/key.pem" -in "$temporary/message" -out "$file.sig" \
    || die "Could not sign $(basename "$file") with WORDINK_LINUX_SIGNING_KEY."
  [ "$(wc -c < "$file.sig" | tr -d ' ')" = 64 ] || die "The signature of $(basename "$file") is not 64 bytes."
  openssl pkeyutl -verify -rawin -pubin -inkey "$public_key" \
    -in "$temporary/message" -sigfile "$file.sig" > /dev/null \
    || die "WORDINK_LINUX_SIGNING_KEY does not match wordink-signing.pub, the key WordInk trusts."
  echo "Signed $(basename "$file") ($sha)."
  signed=$((signed + 1))
done
[ "$signed" -gt 0 ] || die "No Linux executables in $dist to sign."
