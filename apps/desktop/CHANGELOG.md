# Changelog

Release notes for the WordInk desktop app. Each release is published from the
section whose heading is its version (`## X.Y.Z`); a release candidate
(`-rc.N`) uses its own `## X.Y.Z-rc.N` section if there is one, else the
`## X.Y.Z` section it is a candidate for. The release workflow refuses to start
when the section is missing. See [`docs/releasing.md`](../../docs/releasing.md).

## 1.0.0

WordInk is local voice dictation for your desktop. Hold the shortcut, speak,
let go, and the text is pasted into whatever app you are in. Speech is
transcribed on your device; after the first-run model download, nothing leaves
your machine.

- **Linux (x86_64, aarch64):** signed and self-updating. Releases carry an
  ed25519 signature that the installer and the app's updater verify before
  anything is installed, and the app keeps itself up to date from the release
  feed.
- **macOS (Apple silicon) and Windows (x64):** unsigned for 1.0. Gatekeeper and
  SmartScreen will warn on first launch. Updates are download-only: the app
  tells you when a newer release exists and links to it, and never installs one
  itself.
- **Requirements:** Linux with glibc 2.39 or newer (Ubuntu 24.04 or equivalent)
  and a desktop session; macOS 15 or newer on Apple silicon; 64-bit Windows.
  A microphone, and free disk space for the speech model.
- **Verifying a download:** every release lists all of its files in
  `SHA256SUMS`, and `SHA256SUMS.sig` is an ed25519 signature over that list.
  Each file also has a build-provenance attestation
  (`gh attestation verify <file> -R duketopceo/WordInk`).
