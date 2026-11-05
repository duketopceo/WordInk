#!/usr/bin/env bash
# make-appimage.sh <executable> <x64|arm64> <dist>: wraps a Linux release
# executable in WordInk-linux-<x86_64|aarch64>.AppImage in <dist>, with the
# desktop entry and icons from this checkout. The AppImage bundles no
# libraries: like the tarball, it needs glibc 2.39 or newer and the system's
# audio, Vulkan, X11/Wayland and xkbcommon libraries.
#
# Environment:
#   APPIMAGETOOL   appimagetool (an x86_64 AppImage; run with
#                  APPIMAGE_EXTRACT_AND_RUN=1 where FUSE is missing)
#   APPIMAGE_RUNTIME_DIR  directory holding the pinned type2 runtimes
#                  runtime-x86_64 and runtime-aarch64, so appimagetool never
#                  downloads one
#
# The name carries no version, so `releases/latest/download/<name>` always
# fetches the newest one. Runs before signing, in a job with no secrets;
# SHA256SUMS (signed) lists the result.
set -euo pipefail

executable=${1:?Usage: make-appimage.sh <executable> <x64|arm64> <dist>}
label=${2:?Usage: make-appimage.sh <executable> <x64|arm64> <dist>}
dist=${3:?Usage: make-appimage.sh <executable> <x64|arm64> <dist>}
: "${APPIMAGETOOL:?Set APPIMAGETOOL}"
: "${APPIMAGE_RUNTIME_DIR:?Set APPIMAGE_RUNTIME_DIR}"
case "$label" in
  x64) arch=x86_64 ;;
  arm64) arch=aarch64 ;;
  *) echo "::error::The architecture must be x64 or arm64, not $label." >&2; exit 1 ;;
esac
here=$(cd "$(dirname "$0")" && pwd)
desktop=$(cd "$here/../.." && pwd)
app_id=io.github.duketopceo.wordink
runtime="$APPIMAGE_RUNTIME_DIR/runtime-$arch"
[ -f "$runtime" ] || { echo "::error::$runtime is missing" >&2; exit 1; }
[ "$(head -c 4 "$executable" | od -An -tx1 | tr -d ' ')" = 7f454c46 ] \
  || { echo "::error::$executable is not an ELF executable" >&2; exit 1; }

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
appdir="$stage/WordInk.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications"
install -m 755 "$executable" "$appdir/usr/bin/wordink"
# AppRun: run the bundled executable with the caller's arguments (none opens
# the app, as the launcher entry does).
cat > "$appdir/AppRun" <<'SH'
#!/bin/sh
here=$(dirname "$(readlink -f "$0")")
exec "$here/usr/bin/wordink" "$@"
SH
chmod 755 "$appdir/AppRun"
sed 's#@HEX_BIN@#wordink#g' "$desktop/packaging/$app_id.desktop" > "$appdir/usr/share/applications/$app_id.desktop"
cp "$appdir/usr/share/applications/$app_id.desktop" "$appdir/$app_id.desktop"
cp -R "$here/icons" "$appdir/usr/share/icons"
install -m 644 "$here/icons/hicolor/256x256/apps/$app_id.png" "$appdir/$app_id.png"
ln -s "$app_id.png" "$appdir/.DirIcon"
install -m 644 "$desktop/LICENSE" "$desktop/THIRD_PARTY_NOTICES.md" "$appdir/"

out="$dist/WordInk-linux-$arch.AppImage"
rm -f "$out"
ARCH=$arch "$APPIMAGETOOL" --no-appstream --runtime-file "$runtime" "$appdir" "$out" >&2
chmod 755 "$out"
echo "$out"
