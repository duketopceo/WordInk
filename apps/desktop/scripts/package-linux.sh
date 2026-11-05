#!/bin/sh
# Wraps a Linux release executable in the wordink .deb and .rpm
# (packaging/linux/nfpm.yaml):
#
#   package-linux.sh <executable> <version> <x64|arm64> <dist>
#
# writes <dist>/wordink_<amd64|arm64>.deb and <dist>/wordink-<x86_64|aarch64>.rpm
# and prints their paths. The names carry no version, so
# `releases/latest/download/<name>` always fetches the newest one; the package
# metadata carries the version. The Desktop release workflow publishes both.
# Requires nfpm (NFPM names another binary).
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
[ $# -eq 4 ] || {
  echo "Usage: $0 <executable> <version> <x64|arm64> <dist>" >&2
  exit 1
}
executable=$(realpath "$1") version=$2 label=$3 dist=$(realpath "$4")
nfpm=${NFPM:-nfpm}
command -v "$nfpm" >/dev/null 2>&1 || {
  echo "Packaging requires nfpm." >&2
  exit 1
}
case "$label" in
  x64) deb_arch=amd64 rpm_arch=x86_64 ;;
  arm64) deb_arch=arm64 rpm_arch=aarch64 ;;
  *)
    echo "The architecture must be x64 or arm64, not $label." >&2
    exit 1
    ;;
esac
test "$(head -c 4 "$executable" | od -An -tx1 | tr -d ' ')" = 7f454c46 || {
  echo "$executable is not an ELF executable." >&2
  exit 1
}

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
install -m755 "$executable" "$temporary/wordink"
sed 's#@HEX_BIN@#/usr/bin/wordink#g' "$root/packaging/io.github.duketopceo.wordink.desktop" > "$temporary/io.github.duketopceo.wordink.desktop"
cp "$root/packaging/wordink.service" "$root/LICENSE" "$root/packaging/linux/refresh-desktop.sh" "$temporary/"
cp -R "$root/packaging/linux/icons" "$temporary/icons"

# Debian and RPM order a tilde before a release, and neither allows a hyphen
# in the upstream version: 1.2.0-rc.1 is 1.2.0~rc.1.
package_version=$(printf '%s' "$version" | sed -e 's/-/~/' -e 's/-/./g')

for format in deb rpm; do
  case "$format" in
    deb) arch=$deb_arch target="$dist/wordink_$deb_arch.deb" ;;
    rpm) arch=$deb_arch target="$dist/wordink-$rpm_arch.rpm" ;;
  esac
  (
    cd "$temporary"
    # nfpm takes Go's architecture names and maps them to dpkg's and rpm's.
    HEX_PACKAGE_ARCH=$arch HEX_PACKAGE_VERSION=$package_version \
      "$nfpm" package --config "$root/packaging/linux/nfpm.yaml" --packager "$format" \
      --target "$target" >&2
  )
  echo "$target"
done
