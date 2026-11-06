#!/bin/sh
# make-dmg.sh <WordInk.app> <out.dmg>: packs the release app bundle into a
# compressed disk image (volume "WordInk") holding WordInk.app and a link to
# /Applications, so a user drags one onto the other. The image and the app are
# unsigned and not notarized: Gatekeeper asks the user to allow WordInk on
# first open (right-click > Open, or System Settings > Privacy & Security >
# Open Anyway). Needs macOS (hdiutil). The image carries the app's icon as its volume
# icon (.VolumeIcon.icns). It has no styled background: that needs Finder
# AppleScript and a window session CI does not have, and the one in
# packaging/macos/dmg is Hex's, so the window stays plain.
set -eu

app=${1:?Usage: make-dmg.sh <WordInk.app> <out.dmg>}
out=${2:?Usage: make-dmg.sh <WordInk.app> <out.dmg>}
[ -d "$app/Contents/MacOS" ] || { echo "$app is not an app bundle." >&2; exit 1; }
stage=$(mktemp -d "${TMPDIR:-/tmp}/wordink-dmg.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
ditto "$app" "$stage/WordInk.app"
ln -s /Applications "$stage/Applications"
icns=$app/Contents/Resources/AppIcon.icns
[ -f "$icns" ] || { echo "$app has no AppIcon.icns." >&2; exit 1; }
cp "$icns" "$stage/.VolumeIcon.icns"
rm -f "$out"
# A volume shows its custom icon only when the volume root carries the
# custom-icon flag, which is set on the mounted image: build it read-write,
# flag it, then compress.
rw="$stage.rw.dmg"
trap 'rm -rf "$stage" "$rw"' EXIT HUP INT TERM
hdiutil create -quiet -volname WordInk -srcfolder "$stage" -fs HFS+ -format UDRW -ov "$rw"
mount=$(mktemp -d "${TMPDIR:-/tmp}/wordink-dmg-mount.XXXXXX")
hdiutil attach -quiet -readwrite -noverify -noautoopen -nobrowse -mountpoint "$mount" "$rw"
SetFile -a C "$mount" || echo "SetFile is unavailable: the volume keeps the default icon." >&2
sync
hdiutil detach -quiet "$mount"
rmdir "$mount"
hdiutil convert -quiet "$rw" -format UDZO -ov -o "$out"
hdiutil verify -quiet "$out"
echo "$out"
