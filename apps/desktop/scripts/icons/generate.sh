#!/bin/sh
# generate.sh [--sheet <out.png>]: renders every WordInk app icon from the
# one vector source, packaging/icons/wordink.svg:
#   - packaging/linux/icons/hicolor/<N>x<N>/apps/io.github.duketopceo.wordink.png
#   - packaging/linux/icons/hicolor/scalable/apps/io.github.duketopceo.wordink.svg
#   - resources/windows/wordink.ico (the sizes in manifest.txt)
#   - app/AppIcon.icon/Assets/Image.png (macOS glyph layer on a transparent canvas)
# Needs rsvg-convert and ImageMagick (7 `magick` or 6 `convert`). With --sheet it also writes
# a contact sheet of every size, which CI uploads for a visual check.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
svg=$root/packaging/icons/wordink.svg
name=io.github.duketopceo.wordink
hicolor=$root/packaging/linux/icons/hicolor
sheet=
[ "${1:-}" = --sheet ] && sheet=${2:?--sheet needs an output path}

hicolor_sizes=$(sed -n 's/^hicolor //p' "$root/scripts/icons/manifest.txt")
ico_sizes=$(sed -n 's/^ico //p' "$root/scripts/icons/manifest.txt")
work=$(mktemp -d "${TMPDIR:-/tmp}/wordink-icons.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

im() { tool=$1; shift; if command -v magick >/dev/null 2>&1; then if [ "$tool" = convert ]; then magick "$@"; else magick "$tool" "$@"; fi; else "$tool" "$@"; fi; }
render() { rsvg-convert -w "$1" -h "$1" "$svg" -o "$2"; }

for size in $hicolor_sizes; do
  mkdir -p "$hicolor/${size}x${size}/apps"
  render "$size" "$hicolor/${size}x${size}/apps/$name.png"
done
mkdir -p "$hicolor/scalable/apps"
cp "$svg" "$hicolor/scalable/apps/$name.svg"

frames=
for size in $ico_sizes; do
  render "$size" "$work/ico-$size.png"
  frames="$frames $work/ico-$size.png"
done
# shellcheck disable=SC2086
im convert $frames "$root/resources/windows/wordink.ico"

# macOS: the Icon Composer layer is the glyph alone (the app icon's tile and
# gradient come from icon.json), rendered square on a transparent canvas.
glyph=$work/glyph.svg
cat > "$glyph" <<'GLYPH'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="4 5 24 24">
  <rect x="5" y="11" width="4" height="10" rx="2" fill="#E8ECEF"/>
  <rect x="23" y="11" width="4" height="10" rx="2" fill="#E8ECEF"/>
  <path d="M14 6a2 2 0 0 1 4 0v14l-2 8-2-8z" fill="#4FD1C5" stroke="#4FD1C5" stroke-linejoin="round" stroke-width="0.5"/>
</svg>
GLYPH
rsvg-convert -w 1024 -h 1024 "$glyph" -o "$root/app/AppIcon.icon/Assets/Image.png"

if [ -n "$sheet" ]; then
  for size in $hicolor_sizes; do
    im convert "$hicolor/${size}x${size}/apps/$name.png" -background '#808080' \
      -gravity center -extent 520x520 -gravity south -fill white -pointsize 24 \
      -annotate +0+0 "${size}px" "$work/sheet-$size.png"
  done
  # shellcheck disable=SC2046
  im montage $(for s in $hicolor_sizes; do echo "$work/sheet-$s.png"; done) \
    -tile 4x -geometry 260x260+6+6 -background '#808080' "$sheet"
  echo "wrote $sheet"
fi
echo "icons generated from $svg"
