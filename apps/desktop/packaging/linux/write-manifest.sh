#!/usr/bin/env bash
# write-manifest.sh <dist> [stable|dev]: writes <dist>/<channel>.json
# (default stable), the schema-1 update
# manifest WordInk's in-app updater reads (src/updater/protocol.rs), from the
# release files in <dist>. Target URLs are absolute github.com release-asset
# URLs, on the manifest's own host as the protocol requires.
#
#   linux-x64, linux-arm64            the bare executable (what the updater installs)
#   linux-x64-sig, linux-arm64-sig    its raw 64-byte ed25519 signature (sign-linux.sh)
#   macos-arm64, windows-x64          the zips; unsigned, so the updater only links to them
#
# The dev manifest is identical but for `"channel":"dev"`; release candidates
# publish only it, and a final release publishes both so Dev never lags stable.
#
# Environment: WORDINK_VERSION, WORDINK_TAG, WORDINK_REPO (owner/name).
set -euo pipefail

die() { echo "::error::$*" >&2; exit 1; }

dist=${1:?Usage: write-manifest.sh <dist> [stable|dev]}
channel=${2:-stable}
case "$channel" in stable | dev) ;; *) die "Unknown channel '$channel' (stable or dev)." ;; esac
: "${WORDINK_VERSION:?Set WORDINK_VERSION}"
: "${WORDINK_TAG:?Set WORDINK_TAG}"
: "${WORDINK_REPO:?Set WORDINK_REPO}"
v=$WORDINK_VERSION
base="https://github.com/$WORDINK_REPO/releases/download/$WORDINK_TAG"

targets='{}'
add() { # <target> <file>
  local file="$dist/$2"
  [ -f "$file" ] || return 0
  local sha size
  sha=$(sha256sum "$file" | cut -c1-64)
  size=$(wc -c < "$file" | tr -d ' ')
  targets=$(jq -c --arg target "$1" --arg url "$base/$2" --arg sha "$sha" --argjson size "$size" \
    '. + {($target): {url: $url, sha256: $sha, size: $size}}' <<< "$targets")
}
for arch in x64 arm64; do
  add "linux-$arch" "wordink-$v-linux-$arch"
  add "linux-$arch-sig" "wordink-$v-linux-$arch.sig"
done
add macos-arm64 "wordink-$v-macos-arm64.zip"
add windows-x64 "wordink-$v-windows-x64.zip"
# The updater refuses a Linux target without its signature, so never publish one.
for arch in x64 arm64; do
  has() { jq -e --arg t "$1" 'has($t)' <<< "$targets" > /dev/null; }
  if has "linux-$arch" && ! has "linux-$arch-sig"; then die "linux-$arch has no signature."; fi
done
jq -e 'has("linux-x64") or has("linux-arm64")' <<< "$targets" > /dev/null || die "No Linux target in $dist."

jq -n --arg version "$v" --arg channel "$channel" --argjson targets "$targets" \
  --arg notes "https://github.com/$WORDINK_REPO/releases/tag/$WORDINK_TAG" \
  --argjson sequence "$(date +%s)" --arg published "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '{
    schema: 1, app: "hex", artifact: "desktop", channel: $channel,
    sequence: $sequence, published_at: $published,
    release: {
      version: $version, notes_url: $notes, rollout_percent: 100, required: false,
      revoked_versions: [], distributions: [{kind: "direct"}], targets: $targets
    }
  }' > "$dist/$channel.json"
echo "Wrote $dist/$channel.json for $v."
