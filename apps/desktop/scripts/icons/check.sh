#!/bin/sh
# check.sh: fails when an icon size in manifest.txt is missing or has the
# wrong dimensions, or when the scalable SVG, the macOS glyph or a frame of
# wordink.ico is missing.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
name=io.github.duketopceo.wordink
hicolor=$root/packaging/linux/icons/hicolor
status=0
fail() { echo "icon check: $1" >&2; status=1; }

for size in $(sed -n 's/^hicolor //p' "$root/scripts/icons/manifest.txt"); do
  file=$hicolor/${size}x${size}/apps/$name.png
  [ -f "$file" ] || { fail "missing $file"; continue; }
  [ "$(magick identify -format '%wx%h' "$file")" = "${size}x${size}" ] \
    || fail "$file is not ${size}x${size}"
done
[ -f "$hicolor/scalable/apps/$name.svg" ] || fail "missing scalable/apps/$name.svg"
[ -f "$root/app/AppIcon.icon/Assets/Image.png" ] || fail "missing the macOS glyph layer"

ico=$root/resources/windows/wordink.ico
if [ -f "$ico" ]; then
  have=$(magick identify -format '%w\n' "$ico" | sort -n | tr '\n' ' ')
  want=$(sed -n 's/^ico //p' "$root/scripts/icons/manifest.txt" | tr ' ' '\n' | sort -n | tr '\n' ' ')
  [ "$have" = "$want" ] || fail "wordink.ico frames are [$have], expected [$want]"
else
  fail "missing $ico"
fi
[ ! -e "$root/resources/windows/hex.ico" ] || fail "resources/windows/hex.ico is a stale Hex icon"

[ "$status" -eq 0 ] && echo "icon check: all sizes present"
exit "$status"
