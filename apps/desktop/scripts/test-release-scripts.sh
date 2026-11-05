#!/usr/bin/env bash
# Tests the release workflow's scripts without GitHub or the real signing key:
# release-notes.sh, write-sums.sh, sign-linux.sh (binaries and --sums),
# write-manifest.sh, verify-release.sh, check-release-files.sh, the AUR
# render-pkgbuild.sh and release-gate.sh (against a fake `gh`). Needs OpenSSL 3
# and jq.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
linux="$here/../packaging/linux"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fails=0
ok() { echo "ok   $*"; }
bad() { echo "FAIL $*"; fails=$((fails + 1)); }
expect_ok() { local name=$1; shift; if "$@" > "$work/out" 2>&1; then ok "$name"; else bad "$name"; cat "$work/out"; fi; }
expect_fail() { local name=$1; shift; if "$@" > "$work/out" 2>&1; then bad "$name (passed)"; else ok "$name"; fi; }

# --- release-notes.sh -------------------------------------------------------
cat > "$work/CHANGELOG.md" <<'MD'
# Changelog

## 1.0.0 - 2026-01-01

First release.

- one

## 1.0.0-rc.2

Candidate two.

## 0.9.0

Old.
MD
if [ "$("$here/release-notes.sh" 1.0.0 "$work/CHANGELOG.md")" = $'First release.\n\n- one' ]; then ok "notes: final section"; else bad "notes: final section"; fi
if [ "$("$here/release-notes.sh" 1.0.0-rc.2 "$work/CHANGELOG.md")" = "Candidate two." ]; then ok "notes: rc section"; else bad "notes: rc section"; fi
if [ "$("$here/release-notes.sh" 1.0.0-rc.1 "$work/CHANGELOG.md")" = $'First release.\n\n- one' ]; then ok "notes: rc falls back"; else bad "notes: rc falls back"; fi
expect_fail "notes: missing section" "$here/release-notes.sh" 2.0.0 "$work/CHANGELOG.md"
expect_fail "notes: 1.0 is not 1.0.0" "$here/release-notes.sh" 1.0 "$work/CHANGELOG.md"
expect_ok "notes: the real changelog has the 1.0.0 section" "$here/release-notes.sh" 1.0.0

# --- sums and signatures ----------------------------------------------------
openssl genpkey -algorithm ed25519 -out "$work/key.pem" 2> /dev/null
openssl pkey -in "$work/key.pem" -pubout -out "$work/pub.pem"
export WORDINK_SIGNING_PUBLIC_KEY_FILE="$work/pub.pem"
export WORDINK_LINUX_SIGNING_KEY WORDINK_VERSION=1.0.0-rc.1
WORDINK_LINUX_SIGNING_KEY=$(cat "$work/key.pem")
dist="$work/dist"
mkdir "$dist"
v=$WORDINK_VERSION
for f in "wordink-$v-linux-x64" "wordink-$v-linux-arm64" "wordink-$v-linux-x64.tar.gz" "wordink-$v-macos-arm64.zip"; do
  printf 'data %s\n' "$f" > "$dist/$f"
done
expect_ok "sign: binaries" "$linux/sign-linux.sh" "$dist"
expect_ok "sums: write" "$linux/write-sums.sh" "$dist"
expect_ok "sign: SHA256SUMS" "$linux/sign-linux.sh" --sums "$dist"
expect_ok "verify: complete release" "$linux/verify-release.sh" "$dist"
if grep -q "wordink-$v-linux-x64.sig" "$dist/SHA256SUMS"; then ok "sums: lists the signatures"; else bad "sums: lists the signatures"; fi
if [ "$(wc -c < "$dist/SHA256SUMS.sig" | tr -d ' ')" = 64 ]; then ok "sums: signature is 64 bytes"; else bad "sums: signature is 64 bytes"; fi

cp -R "$dist" "$work/tampered"
printf 'x' >> "$work/tampered/wordink-$v-macos-arm64.zip"
expect_fail "verify: a changed file" "$linux/verify-release.sh" "$work/tampered"
cp -R "$dist" "$work/extra"
printf 'x' > "$work/extra/stable.json"
expect_fail "verify: an unlisted file" "$linux/verify-release.sh" "$work/extra"
cp -R "$dist" "$work/resigned"
printf 'wordink.sha256sums.v1\nnot the hash\n' > "$work/msg"
openssl pkeyutl -sign -rawin -inkey "$work/key.pem" -in "$work/msg" -out "$work/resigned/SHA256SUMS.sig"
expect_fail "verify: a signature over another message" "$linux/verify-release.sh" "$work/resigned"
cp -R "$dist" "$work/swapped"
cp "$work/swapped/wordink-$v-linux-x64.sig" "$work/swapped/SHA256SUMS.sig"
expect_fail "verify: a binary signature is not a sums signature (domains differ)" "$linux/verify-release.sh" "$work/swapped"
openssl genpkey -algorithm ed25519 -out "$work/other.pem" 2> /dev/null
cp -R "$dist" "$work/wrongkey"
WORDINK_LINUX_SIGNING_KEY=$(cat "$work/other.pem") expect_fail "sign: a key that does not match the public key" "$linux/sign-linux.sh" --sums "$work/wrongkey"

# --- write-manifest.sh ------------------------------------------------------
export WORDINK_TAG="desktop-v$v" WORDINK_REPO=r/w
manifests="$work/manifests"
mkdir "$manifests"
cp "$dist"/wordink-* "$manifests/"
expect_ok "manifest: stable" "$linux/write-manifest.sh" "$manifests"
expect_ok "manifest: dev" "$linux/write-manifest.sh" "$manifests" dev
if [ "$(jq -r .channel "$manifests/stable.json")" = stable ]; then ok "manifest: stable channel"; else bad "manifest: stable channel"; fi
if [ "$(jq -r .channel "$manifests/dev.json")" = dev ]; then ok "manifest: dev.json says \"channel\":\"dev\""; else bad "manifest: dev.json says dev"; fi
if [ "$(jq -S 'del(.channel, .sequence, .published_at)' "$manifests/stable.json")" = "$(jq -S 'del(.channel, .sequence, .published_at)' "$manifests/dev.json")" ]; then ok "manifest: dev names the same release as stable"; else bad "manifest: dev names the same release as stable"; fi
if [ "$(jq -r .release.version "$manifests/dev.json")" = "$v" ]; then ok "manifest: dev names this version"; else bad "manifest: dev names this version"; fi
expect_fail "manifest: unknown channel" "$linux/write-manifest.sh" "$manifests" beta
rm "$manifests/wordink-$v-linux-arm64.sig"
expect_fail "manifest: stable refuses a Linux target without its signature" "$linux/write-manifest.sh" "$manifests"
expect_fail "manifest: dev refuses a Linux target without its signature" "$linux/write-manifest.sh" "$manifests" dev

# --- check-release-files.sh ---------------------------------------------------
files="$work/files"
mkdir "$files"
for f in "wordink-$v-linux-x64" "wordink-$v-linux-x64.tar.gz" "wordink-$v-linux-arm64" "wordink-$v-linux-arm64.tar.gz" \
  "wordink-$v-windows-x64.zip" "wordink-$v-macos-arm64.zip" "wordink-$v.sbom.cdx.json" install.sh install-macos.sh install.ps1 \
  WordInk-Setup-windows-x64.exe WordInk-macos-arm64.dmg WordInk-linux-x86_64.AppImage WordInk-linux-aarch64.AppImage \
  wordink_amd64.deb wordink_arm64.deb wordink-x86_64.rpm wordink-aarch64.rpm; do
  : > "$files/$f"
done
expect_ok "files: the complete set" "$linux/check-release-files.sh" "$files"
: > "$files/extra.txt"
expect_fail "files: an extra file" "$linux/check-release-files.sh" "$files"
rm "$files/extra.txt" "$files/wordink_arm64.deb"
expect_fail "files: a missing package" "$linux/check-release-files.sh" "$files"

# --- AUR render-pkgbuild.sh -----------------------------------------------
aur="$here/../packaging/aur"
printf '%064d  wordink-1.2.0-rc.1-linux-x64.tar.gz\n%064d  wordink-1.2.0-rc.1-linux-arm64.tar.gz\n' 1 2 > "$work/aur-sums"
if "$aur/render-pkgbuild.sh" 1.2.0-rc.1 "$work/aur-sums" > "$work/PKGBUILD" 2> "$work/out"; then ok "aur: renders"; else bad "aur: renders"; cat "$work/out"; fi
if grep -q '^pkgver=1.2.0rc1$' "$work/PKGBUILD" && grep -q '^_version=1.2.0-rc.1$' "$work/PKGBUILD"; then ok "aur: pacman version"; else bad "aur: pacman version"; fi
if grep -q "^sha256sums_aarch64=('0\{63\}2')$" "$work/PKGBUILD"; then ok "aur: arm64 checksum"; else bad "aur: arm64 checksum"; fi
if grep -v HEX_BIN "$work/PKGBUILD" | grep -q '@[A-Z0-9_]*@'; then bad "aur: placeholders left"; else ok "aur: no placeholders left"; fi
expect_fail "aur: a tarball missing from SHA256SUMS" "$aur/render-pkgbuild.sh" 1.2.0 "$work/aur-sums"
expect_fail "aur: not a release version" "$aur/render-pkgbuild.sh" 1.2 "$work/aur-sums"

# --- release-gate.sh against a fake gh -------------------------------------
mkdir "$work/bin"
cat > "$work/bin/gh" <<'SH'
#!/usr/bin/env bash
# Fake gh: answers from files in $FAKE; logs calls to $FAKE/calls.
printf '%s\n' "$*" >> "$FAKE/calls"
case "$1 $2" in
  "api repos/r/compare/"*) cat "$FAKE/compare" ;;
  "release view") [ -f "$FAKE/release-exists" ] ;;
  "workflow run") : > "$FAKE/dispatched"; [ ! -f "$FAKE/runs-after-dispatch" ] || cp "$FAKE/runs-after-dispatch" "$FAKE/runs" ;;
  "api --paginate") cat "$FAKE/rc-tags" ;;
  "api repos/r/commits/"*) tag=${2#repos/r/commits/}; sed -n "s/^$tag //p" "$FAKE/rc-commits" ;;
  "api repos/r/actions/workflows/desktop.yml/runs"*) cat "$FAKE/runs" ;;
  *) echo "fake gh: unexpected $*" >&2; exit 99 ;;
esac
SH
chmod +x "$work/bin/gh"
gate() { # name expected(ok|fail) -- uses $FAKE files, TAG/VERSION from env
  local name=$1 expected=$2
  if env GH="$work/bin/gh" FAKE="$FAKE" REPO=r SHA=abc POLL_SECONDS=0 GATE_TIMEOUT_SECONDS=5 "$here/release-gate.sh" > "$work/out" 2>&1; then
    if [ "$expected" = ok ]; then ok "gate: $name"; else bad "gate: $name (passed)"; cat "$work/out"; fi
  else
    if [ "$expected" = fail ]; then ok "gate: $name"; else bad "gate: $name (failed)"; cat "$work/out"; fi
  fi
}
newfake() { FAKE=$(mktemp -d -p "$work"); export FAKE; echo ahead > "$FAKE/compare"; : > "$FAKE/rc-tags"; : > "$FAKE/rc-commits"; printf '' > "$FAKE/runs"; }
run_line() { printf '%s\n' "$1" > "$FAKE/runs"; }
export TAG=desktop-v1.0.0-rc.1 VERSION=1.0.0-rc.1

newfake; run_line "1 completed success"; gate "rc on a green commit" ok
newfake; echo diverged > "$FAKE/compare"; run_line "1 completed success"; gate "a commit not on master" fail
newfake; run_line "1 completed failure"; gate "a failed Desktop run" fail
newfake; run_line "1 completed cancelled"; gate "a cancelled Desktop run" fail
newfake; run_line "1 completed success"; : > "$FAKE/release-exists"; gate "a release for the tag already exists" fail
newfake; run_line "1 in_progress none"; gate "a run still in progress that never finishes" fail
newfake; echo "2 completed success" > "$FAKE/runs-after-dispatch"; gate "no run: dispatches and passes" ok
if grep -q "workflow run desktop.yml -R r --ref $TAG" "$FAKE/calls"; then ok "gate: dispatched on the tag"; else bad "gate: dispatched on the tag"; fi
newfake; echo "2 completed cancelled" > "$FAKE/runs-after-dispatch"; gate "no run: dispatched run cancelled" fail
newfake; gate "no run and the dispatch never shows up" fail

export TAG=desktop-v1.0.0 VERSION=1.0.0
newfake; run_line "1 completed success"; gate "final without an rc" fail
newfake; run_line "1 completed success"; echo desktop-v1.0.0-rc.1 > "$FAKE/rc-tags"; echo "desktop-v1.0.0-rc.1 other" > "$FAKE/rc-commits"; gate "final with an rc on another commit" fail
newfake; run_line "1 completed success"; echo desktop-v1.0.0-rc.1 > "$FAKE/rc-tags"; echo "desktop-v1.0.0-rc.1 abc" > "$FAKE/rc-commits"; gate "final with an rc on the same commit" ok

[ "$fails" = 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
