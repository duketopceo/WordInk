#!/usr/bin/env bash
# WordInk Remote: build a signed Release archive and upload to TestFlight (internal). Run on a Mac with Xcode.
# Needs ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH (AuthKey_<ID>.p8), APPLE_TEAM_ID.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
die() { echo "testflight: $*" >&2; exit 1; }
[[ "$(uname -s)" == "Darwin" ]] || die "Run on macOS with Xcode."
: "${ASC_KEY_ID:?}"; : "${ASC_ISSUER_ID:?}"; : "${ASC_KEY_PATH:?}"; APPLE_TEAM_ID="${APPLE_TEAM_ID:-6BP7W45U55}"
[[ -f "$ASC_KEY_PATH" ]] || die "ASC_KEY_PATH is not a file"

BUILD_NUMBER="${BUILD_NUMBER:-$(date -u +%Y%m%d%H%M%S)}"
MARKETING_VERSION="${MARKETING_VERSION:-1.0.0}"
ARCHIVE="$ROOT/build/WordInkRemote.xcarchive"; EXPORT_DIR="$ROOT/build/export"; IPA="$EXPORT_DIR/WordInkRemote.ipa"
mkdir -p build
command -v xcodegen >/dev/null && xcodegen generate

AUTH_ARGS=(-allowProvisioningUpdates -authenticationKeyPath "$ASC_KEY_PATH" -authenticationKeyID "$ASC_KEY_ID" -authenticationKeyIssuerID "$ASC_ISSUER_ID")

echo "→ Archive (build $BUILD_NUMBER, version $MARKETING_VERSION)"
xcodebuild -project WordInk.xcodeproj -scheme WordInkRemote -configuration Release \
  -destination 'generic/platform=iOS' -archivePath "$ARCHIVE" archive "${AUTH_ARGS[@]}" \
  DEVELOPMENT_TEAM="$APPLE_TEAM_ID" MARKETING_VERSION="$MARKETING_VERSION" CURRENT_PROJECT_VERSION="$BUILD_NUMBER"

EXPORT_PLIST="$ROOT/build/ExportOptions.plist"; cp ci/ExportOptions.plist "$EXPORT_PLIST"
/usr/libexec/PlistBuddy -c "Add :teamID string $APPLE_TEAM_ID" "$EXPORT_PLIST" 2>/dev/null || /usr/libexec/PlistBuddy -c "Set :teamID $APPLE_TEAM_ID" "$EXPORT_PLIST"
rm -rf "$EXPORT_DIR"
echo "→ Export IPA"
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportPath "$EXPORT_DIR" -exportOptionsPlist "$EXPORT_PLIST" "${AUTH_ARGS[@]}"
[[ -f "$IPA" ]] || die "Expected IPA at $IPA"
[[ "${SKIP_UPLOAD:-}" == "1" ]] && { echo "IPA: $IPA (upload skipped)"; exit 0; }

KEY_DIR="$HOME/.appstoreconnect/private_keys"; mkdir -p "$KEY_DIR"; DEST_KEY="$KEY_DIR/AuthKey_${ASC_KEY_ID}.p8"
[[ "$(realpath "$ASC_KEY_PATH")" == "$(realpath "$DEST_KEY" 2>/dev/null || echo x)" ]] || { cp "$ASC_KEY_PATH" "$DEST_KEY"; chmod 600 "$DEST_KEY"; }
echo "→ Upload to TestFlight"
xcrun altool --upload-app -f "$IPA" -t ios --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
echo "Done (build $BUILD_NUMBER). Processing usually takes 10–30 minutes."
