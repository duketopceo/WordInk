# Releasing the desktop app

The desktop app (`apps/desktop`, the `wordink` binary) is released by pushing a
tag. Nothing else publishes it: the Changesets `release.yml` workflow only
publishes npm packages and pushes its own tags, and creates no GitHub Release,
so "latest" always means the latest desktop release.

Two kinds of tag exist:

| Tag | What it is | Channel | GitHub Release |
| --- | --- | --- | --- |
| `desktop-vX.Y.Z-rc.N` | release candidate | built with `HEX_BUILD_CHANNEL=dev` | prerelease, never latest |
| `desktop-vX.Y.Z` | final release | built with `HEX_BUILD_CHANNEL=stable` | latest, with `stable.json` |

No other tag suffix is accepted.

## Cut a release: candidate first, then final

1. Write the release notes. Add a `## X.Y.Z` section to
   [`apps/desktop/CHANGELOG.md`](../apps/desktop/CHANGELOG.md) and merge it to
   `master`. The release body is exactly that section. A candidate uses its own
   `## X.Y.Z-rc.N` section if there is one, else `## X.Y.Z`. A missing or empty
   section fails the run in its first job, before anything is built or signed.
2. Tag the merged `master` commit as a candidate and push the tag:

   ```sh
   git tag desktop-v1.2.3-rc.1 <commit>
   git push origin desktop-v1.2.3-rc.1
   ```

3. The **Desktop release** workflow
   (`.github/workflows/desktop-release.yml`) runs these jobs in order:

   ```text
   prepare -> gate -> build (x4), sbom -> package-linux -> sign-linux
           -> draft -> fetch-draft -> smoke (x4) -> publish
   ```

   - `prepare` checks the tag name and extracts the release notes.
   - `gate` stops the run unless the tagged commit is on `master` and the
     **Desktop** workflow succeeded on it. A docs-only commit has no Desktop
     run (it is path-filtered), so the gate dispatches one on the tag and waits
     for it; a cancelled run counts as a failure. A final tag also needs a
     published (not draft) `desktop-vX.Y.Z-rc.N` release whose tag points at the
     same commit.
   - `build` builds Linux x86_64 and aarch64, Windows x64 and macOS arm64;
     `sbom` writes the CycloneDX SBOM; `package-linux` packs the two Linux
     tarballs and gathers the three installers. None of these has a secret. The
     macOS build is the unsigned `WordInk.app` from `scripts/build-app.sh`,
     zipped with `ditto` so the zip root is `WordInk.app/`.
   - `sign-linux` waits for approval (next step), signs the bare Linux
     executables, writes `stable.json` (final tags only), lists every file in
     `SHA256SUMS`, signs that list, and verifies the result.
   - `draft` attests every file, creates the GitHub Release as a **draft** and
     checks the draft holds exactly the files in `SHA256SUMS`.
   - `fetch-draft` downloads the draft's assets with the job token, checks them
     against `SHA256SUMS` and hands them on as a workflow artifact. It checks
     out and runs nothing.
   - `smoke` runs on Linux x86_64, Linux aarch64, macOS arm64 and Windows x64.
     Each runner is read-only, has no secrets and no checkout. It serves the
     draft's assets from a loopback server, runs that OS's published installer
     with `WORDINK_INSTALL_BASE` pointed at it, checks that `wordink --version`
     prints the tag's version, uninstalls, and installs again. The Linux runners
     have no user systemd session, so there the installer ends with its "run
     `wordink app`" hint, which still counts as success; the service, autostart
     and the source-build migration are covered by the installer test script
     and the manual checklist. A failed smoke keeps the release a draft.
   - `publish` only un-drafts the release. It checks out and runs nothing.

4. **Approve the `release` environment.** The run pauses at `sign-linux`;
   GitHub asks the owner to review the deployment. That approval is what lets
   the job read the Linux signing key, so check before approving that the run is
   for the tag you pushed and that the tagged commit is the reviewed one. Reject
   it for anything else.
5. Test the candidate: run [`docs/release-checklist.md`](release-checklist.md)
   on a real machine of each OS. If anything is wrong, fix it on `master` and
   cut `-rc.N+1`.
6. When a candidate passes, tag **the same commit** as the final release and
   approve the `release` environment again:

   ```sh
   git tag desktop-v1.2.3 <commit of the passing candidate>
   git push origin desktop-v1.2.3
   ```

   The gate refuses a final tag whose commit has no published candidate.

## What a release contains

For version `V` (`stable.json` only on final releases):

| File | Purpose |
| --- | --- |
| `wordink-V-linux-x64`, `wordink-V-linux-arm64` | bare executables, what the in-app updater installs |
| `wordink-V-linux-x64.sig`, `wordink-V-linux-arm64.sig` | raw 64-byte ed25519 signature of each executable |
| `wordink-V-linux-x64.tar.gz`, `wordink-V-linux-arm64.tar.gz` | tarball with the installer, desktop files and icons (made before signing, so it holds no `.sig`) |
| `wordink-V-macos-arm64.zip` | unsigned `WordInk.app` (zip root `WordInk.app/`) |
| `wordink-V-windows-x64.zip` | unsigned build with `wordink.exe` |
| `install.sh`, `install-macos.sh`, `install.ps1` | the one-line installers (Linux, macOS, Windows); users run them from `releases/latest/download/` |
| `wordink-V.sbom.cdx.json` | CycloneDX SBOM |
| `stable.json` | update manifest the stable-channel updater reads |
| `SHA256SUMS` | sha256 of every other file above, in `sha256sum -c` format |
| `SHA256SUMS.sig` | raw 64-byte ed25519 signature of `SHA256SUMS` |

`SHA256SUMS` lists every published file except itself and `SHA256SUMS.sig`;
the workflow fails if the release has any other file or lacks one.

Check a download with `sha256sum -c SHA256SUMS` (after verifying
`SHA256SUMS.sig`, below) or `gh attestation verify <file> -R duketopceo/WordInk`.

### Signature domains

Both signatures use the Linux release key (`wordink-signing.pub`, an ed25519
key, PEM) over a short message that starts with a fixed domain string, so a
signature made for one purpose can never verify as the other:

| Signature | Message that is signed |
| --- | --- |
| `wordink-V-linux-<arch>.sig` | `"wordink.linux-artifact.v1\n" + sha256(executable) + "\n"` |
| `SHA256SUMS.sig` | `"wordink.sha256sums.v1\n" + sha256(SHA256SUMS file) + "\n"` |

`sha256(...)` is the lowercase hex digest of the file's exact bytes. The
signature is raw (64 bytes), made with `openssl pkeyutl -sign -rawin`. To verify
`SHA256SUMS`:

```sh
printf 'wordink.sha256sums.v1\n%s\n' "$(sha256sum SHA256SUMS | cut -c1-64)" > message
openssl pkeyutl -verify -rawin -pubin -inkey wordink-signing.pub -in message -sigfile SHA256SUMS.sig
```

The installers use the same message, so do not change the domain strings
without changing every verifier. `apps/desktop/packaging/linux/verify-release.sh`
runs this check (and every other) on a release directory, and
`apps/desktop/scripts/test-release-scripts.sh` tests the scripts.

## Pulling a bad release

Move it back to a draft; it disappears from the release page and from
`releases/latest`:

```sh
gh release edit desktop-v1.2.3 -R duketopceo/WordInk --draft=true
```

Do not delete the tag unless the release is abandoned; to re-run the workflow
for a tag, delete the draft release and the tag, then push the tag again (the
gate refuses a tag that already has a release). After pulling a final release,
the previous final becomes "latest" again only once you run
`gh release edit <previous tag> --latest`. Fix forward with a new version
rather than re-using a published one.

Updaters that already installed the bad version do not roll back on their own:
Linux users can revert by hand (below), or a fixed release supersedes it.

### Manual Linux revert

The Linux install keeps every version it has installed and `current` points at
the active one:

```text
~/.local/share/wordink/versions/<version>/wordink
~/.local/share/wordink/current -> versions/<version>
```

To go back, stop the service, point `current` at the previous version and
start it again:

```sh
systemctl --user stop wordink
ls ~/.local/share/wordink/versions
ln -sfn versions/<previous version> ~/.local/share/wordink/current
systemctl --user start wordink
```

Check with `wordink --version`. The app may update itself forward again on the
next check unless the bad release has been pulled back to a draft.

## What users get

| Platform | Download | Updates |
| --- | --- | --- |
| Linux x86_64, aarch64 | tarball with `install.sh`; the signed executable | The app downloads the newest release, verifies its ed25519 signature against the key compiled into it, and restarts into it. |
| macOS (Apple silicon) | zip with `WordInk.app`, **unsigned**; `install-macos.sh` | The app only links to the newest release. It never installs one. |
| Windows x64 | zip with `wordink.exe`, **unsigned**; `install.ps1` | The app only links to the newest release. It never installs one. |

The tarball is assembled before signing and no longer contains `wordink.sig`,
which the unpacked `install.sh` still looks for. Until the bootstrap installer
(which verifies `SHA256SUMS.sig` and the tarball's hash before it unpacks
anything) replaces that path, an unpacked tarball installs only with
`WORDINK_INSTALL_UNVERIFIED=1`, or with the executable's `.sig` copied beside it
as `wordink.sig`.

macOS Gatekeeper and Windows SmartScreen will warn about the unsigned builds.
Signing and notarization are deferred until there are paid developer accounts;
until then, say so wherever a macOS or Windows download is offered.

## One-time setup (owner)

The `release` environment and the `WORDINK_LINUX_SIGNING_KEY` secret must exist
before the first tag. The commands are in the overnight report's "Needs you".
If the environment is missing when a tag is pushed, GitHub creates an
unprotected one on the first run, so create it first. Rotating the key is in
[`SECURITY.md`](../SECURITY.md).
