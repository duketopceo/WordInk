# Releasing the desktop app

The desktop app (`apps/desktop`, the `wordink` binary) is released by pushing a
tag. Nothing else publishes it.

## Cut a release

1. Make sure `master` is green on the **Desktop** workflow
   (`.github/workflows/desktop.yml`: Linux x86_64 and aarch64, Windows x64 and
   macOS arm64 build, lint and test).
2. Tag the commit and push the tag. The tag is the version:

   ```sh
   git tag desktop-v1.2.3 <commit>
   git push origin desktop-v1.2.3
   ```

   `desktop-vX.Y.Z` is a stable release; `desktop-vX.Y.Z-beta.1` is published as
   a prerelease and is never offered by the updater.
3. **Approve the `release` environment.** The **Desktop release** workflow
   (`.github/workflows/desktop-release.yml`) builds all four platforms, then
   pauses at the `sign-linux` job. GitHub asks the owner to review the
   deployment to `release`. That approval is the one that lets the job read the
   Linux signing key, so check before you approve that the run is for the tag
   you pushed and that the tagged commit is the reviewed one. Reject it for
   anything else.
4. The `publish` job then writes the GitHub Release: Linux tarballs and bare
   executables with their signatures, the Windows and macOS zips, `SHA256SUMS`,
   a CycloneDX SBOM, build-provenance attestations and `stable.json`, the update
   manifest. The release is published only once all of them are attached.

Check a download with `sha256sum -c SHA256SUMS` or
`gh attestation verify <file> -R duketopceo/WordInk`.

## What users get

| Platform | Download | Updates |
| --- | --- | --- |
| Linux x86_64, aarch64 | tarball with `install.sh`; the signed executable | The app downloads the newest release, verifies its ed25519 signature against the key compiled into it, and restarts into it. |
| macOS (Apple silicon) | zip with the bare executable, **unsigned** | The app only links to the newest release. It never installs one. |
| Windows x64 | zip with `wordink.exe`, **unsigned** | The app only links to the newest release. It never installs one. |

macOS Gatekeeper and Windows SmartScreen will warn about the unsigned builds.
Signing and notarization are deferred until there are paid developer accounts;
until then, say so wherever a macOS or Windows download is offered.

## One-time setup (owner)

The `release` environment and the `WORDINK_LINUX_SIGNING_KEY` secret must exist
before the first tag. The commands are in the overnight report's "Needs you".
If the environment is missing when a tag is pushed, GitHub creates an
unprotected one on the first run, so create it first. Rotating the key is in
[`SECURITY.md`](../SECURITY.md).
