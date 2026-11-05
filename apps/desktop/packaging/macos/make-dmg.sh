#!/bin/sh
# make-dmg.sh <WordInk.app> <out.dmg>: packs the release app bundle into a
# compressed disk image (volume "WordInk") holding WordInk.app and a link to
# /Applications, so a user drags one onto the other. The image and the app are
# unsigned and not notarized: Gatekeeper asks the user to allow WordInk on
# first open (right-click > Open, or System Settings > Privacy & Security >
# Open Anyway). Needs macOS (hdiutil). The styled installer window in
# packaging/macos/dmg is Hex's and is not used here.
set -eu

app=${1:?Usage: make-dmg.sh <WordInk.app> <out.dmg>}
out=${2:?Usage: make-dmg.sh <WordInk.app> <out.dmg>}
[ -d "$app/Contents/MacOS" ] || { echo "$app is not an app bundle." >&2; exit 1; }
stage=$(mktemp -d "${TMPDIR:-/tmp}/wordink-dmg.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
ditto "$app" "$stage/WordInk.app"
ln -s /Applications "$stage/Applications"
rm -f "$out"
hdiutil create -quiet -volname WordInk -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$out"
hdiutil verify -quiet "$out"
echo "$out"
