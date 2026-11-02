#!/bin/sh
# Installs a WordInk release for the current user, from the release tarball it
# ships in (wordink-<version>-linux-<x64|arm64>.tar.gz). Run it from the
# unpacked directory:
#
#   ./install.sh [install|uninstall]
#
# It checks the executable against the release's ed25519 signature, then lays
# out what the in-app updater (src/updater/linux.rs) updates itself in:
#   ~/.local/share/wordink/versions/<version>/wordink
#   ~/.local/share/wordink/current            -> versions/<version>
#   ~/.local/share/wordink/install.json       (the receipt that turns updates on)
#   ~/.local/bin/wordink                      -> ~/.local/share/wordink/current/wordink
#   ~/.local/share/applications/io.github.duketopceo.wordink.desktop
#   ~/.local/share/icons/hicolor/*/apps/io.github.duketopceo.wordink.png
#   ~/.config/systemd/user/wordink.service    (enabled, started)
#
# WordInk's Linux release key (packaging/linux/wordink-signing.pub). The
# signature file is the release's raw 64-byte wordink.sig over
# "wordink.linux-artifact.v1\n<sha256 of the executable>\n".
set -eu

signing_key=MCowBQYDK2VwAyEAupIraDuQFZun9b4ZGlL5P6j9klVOOFmXTzaPNuxkKAc=
app_id=io.github.duketopceo.wordink

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
bin_dir=${WORDINK_BIN_DIR:-"$HOME/.local/bin"}
support=${HEX_APPLICATION_SUPPORT_DIR:-"$data_home/wordink"}
applications="$data_home/applications"
icons="$data_home/icons/hicolor"
units="$config_home/systemd/user"

fail() {
  echo "$1" >&2
  exit 1
}

if [ "$(id -u)" = 0 ]; then
  fail "Run this as your desktop user, not root."
fi
case "$(uname -s) $(uname -m)" in
  "Linux x86_64" | "Linux aarch64") ;;
  *) fail "WordInk supports x86_64 and aarch64 Linux." ;;
esac

uninstall() {
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user disable --now wordink.service 2>/dev/null || true
  fi
  rm -f "$units/wordink.service" "$applications/$app_id.desktop"
  if [ -L "$bin_dir/wordink" ]; then rm -f "$bin_dir/wordink"; fi
  if [ -L "$support/current" ]; then
    rm -f "$support/current" "$support/install.json"
    rm -rf "$support/versions"
  fi
  find "$icons" -name "$app_id.png" -delete 2>/dev/null || true
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload 2>/dev/null || true
  fi
  echo "WordInk uninstalled. Settings and history stay in $support."
}

case "${1:-install}" in
  install) ;;
  uninstall) uninstall; exit 0 ;;
  *) echo "Usage: $0 [install|uninstall]" >&2; exit 2 ;;
esac

binary="$here/wordink"
[ -f "$binary" ] || fail "wordink is missing beside this installer: $binary"

# Verify before anything runs or is copied. A release carries its signature;
# without openssl 3 or a signature the install is refused, unless the person
# running it says to skip the check.
if [ "${WORDINK_INSTALL_UNVERIFIED:-}" != 1 ]; then
  command -v openssl >/dev/null 2>&1 || fail "Verifying the release needs openssl. Install it, or set WORDINK_INSTALL_UNVERIFIED=1 to skip the check."
  command -v sha256sum >/dev/null 2>&1 || fail "Verifying the release needs sha256sum."
  [ -f "$here/wordink.sig" ] || fail "wordink.sig is missing beside the executable; refusing to install it."
  [ "$(wc -c < "$here/wordink.sig" | tr -d ' ')" = 64 ] || fail "wordink.sig is not 64 bytes; refusing to install."
  temporary=$(mktemp -d)
  trap 'rm -rf "$temporary"' EXIT
  sha256=$(sha256sum "$binary" | awk '{ print $1 }')
  printf -- '-----BEGIN PUBLIC KEY-----\n%s\n-----END PUBLIC KEY-----\n' "$signing_key" > "$temporary/signing.pem"
  printf 'wordink.linux-artifact.v1\n%s\n' "$sha256" > "$temporary/signed"
  openssl pkeyutl -verify -rawin -pubin -inkey "$temporary/signing.pem" \
    -in "$temporary/signed" -sigfile "$here/wordink.sig" >/dev/null 2>&1 \
    || fail "The release signature does not verify; refusing to install it."
fi

chmod 755 "$binary" 2>/dev/null || true
version=$("$binary" --version 2>/dev/null | awk 'NF { v=$NF } END { print v }') || true
[ -n "$version" ] || fail "The executable cannot start here. Check that its libraries are installed (ldd $binary)."
printf '%s\n' "$version" | grep -Eq '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$' || fail "The executable reports an invalid version."

# An earlier source install (scripts/install-linux.sh) keeps `current` as a
# real directory the updater cannot switch.
if [ -e "$support/current" ] && [ ! -L "$support/current" ]; then
  fail "$support/current is a source install. Run scripts/install-linux.sh uninstall first."
fi
if [ -e "$bin_dir/wordink" ] && [ ! -L "$bin_dir/wordink" ]; then
  fail "Refusing to replace $bin_dir/wordink: it is not a symlink."
fi
case "$support" in
  *\"* | *\\*) fail "The support path contains unsupported characters." ;;
esac

umask 022
mkdir -p "$support/versions/$version" "$bin_dir" "$applications" "$units"
chmod 700 "$support" "$support/versions" "$support/versions/$version"
install -m 755 "$binary" "$support/versions/$version/wordink.new"
mv -f "$support/versions/$version/wordink.new" "$support/versions/$version/wordink"
rm -f "$support/versions/$version/source-build"
ln -sfn "versions/$version" "$support/.current.next"
mv -Tf "$support/.current.next" "$support/current"
printf '{"app":"hex","artifact":"desktop","channel":"stable","method":"direct","root":"%s"}\n' "$support" > "$support/.install.json.next"
mv -f "$support/.install.json.next" "$support/install.json"
ln -sfn "$support/current/wordink" "$bin_dir/wordink"

sed "s#@HEX_BIN@#$bin_dir/wordink#g" "$here/$app_id.desktop" > "$applications/$app_id.desktop"
for png in "$here"/icons/hicolor/*/apps/"$app_id.png"; do
  [ -f "$png" ] || continue
  size=$(basename "$(dirname "$(dirname "$png")")")
  mkdir -p "$icons/$size/apps"
  install -m 644 "$png" "$icons/$size/apps/$app_id.png"
done
sed "s#^ExecStart=/usr/bin/wordink#ExecStart=$bin_dir/wordink#" "$here/wordink.service" > "$units/wordink.service"
if command -v systemctl >/dev/null 2>&1; then
  systemctl --user daemon-reload 2>/dev/null || true
  systemctl --user enable wordink.service >/dev/null 2>&1 || true
  systemctl --user restart wordink.service 2>/dev/null || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$applications" >/dev/null 2>&1 || true
fi

echo "WordInk $version installed. Open the window with: wordink app"
echo "It checks the stable release for updates and verifies each one's signature."
