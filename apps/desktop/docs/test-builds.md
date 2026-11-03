# Test Builds

WordInk has no per-push test-build workflow or download page. To try an
unreleased build:

- Build from a checkout (see [../README.md](../README.md) and
  [linux.md](linux.md), [windows.md](windows.md)). `HEX_TEST_BUILD=1
  scripts/build-app.sh` produces an ad-hoc signed macOS `Hex Test.app` with its
  own bundle ID and data folder (`hex-test`), so it never reads or changes an
  installed WordInk's data. Test and local builds never self-update.
- Or install a published release or the Dev channel build; see
  [downloads.md](downloads.md) and [releasing.md](../../../docs/releasing.md).

Unsigned macOS builds may need `xattr -dr com.apple.quarantine <app>` or
**Open Anyway** in System Settings > Privacy & Security, and macOS asks for
Microphone and Accessibility again after each ad-hoc build.
