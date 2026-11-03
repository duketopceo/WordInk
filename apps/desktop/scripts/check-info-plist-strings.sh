#!/bin/sh
# Fails when a user-visible Info.plist string (bundle name, display name, any
# *UsageDescription) still says "Hex". Internal ids (bundle identifiers, URL
# schemes, executable names) are not checked. With no arguments it checks the
# plist templates; build-app.sh runs it on the built bundle's Info.plist.
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$#" -eq 0 ]; then
  set -- "$root/app/Info.plist" "$root/app/ServiceInfo.plist" "$root/packaging/macos/scrawl-helper-Info.plist"
fi
python3 -I - "$@" <<'PY'
import plistlib, re, sys
bad = 0
for path in sys.argv[1:]:
    with open(path, "rb") as handle:
        plist = plistlib.load(handle)
    for key, value in plist.items():
        visible = key in ("CFBundleName", "CFBundleDisplayName") or key.endswith("UsageDescription")
        if visible and isinstance(value, str) and re.search(r"\bhex\b", value, re.I):
            print(f"{path}: {key} says Hex: {value}", file=sys.stderr)
            bad = 1
sys.exit(bad)
PY
