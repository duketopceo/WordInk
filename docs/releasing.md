# Releasing the desktop app

The desktop app (`apps/desktop`, the `wordink` binary) is released by pushing a
tag. Nothing else publishes it: the Changesets `release.yml` workflow only
publishes npm packages and pushes its own tags, and creates no GitHub Release,
so "latest" always means the latest desktop release.

Two kinds of tag exist:

| Tag | What it is | Channel | GitHub Release |
| --- | --- | --- | --- |
| `desktop-vX.Y.Z-rc.N` | release candidate | built with `HEX_BUILD_CHANNEL=dev` | prerelease, never latest |
| `desktop-vX.Y.Z` | final release | built with `HEX_BUILD_CHANNEL=stable` | latest, with `stable.json` and `dev.json` |

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
   prepare -> gate -> build (x4), sbom -> package-linux -> check-installers (x4)
           -> sign-linux -> draft -> fetch-draft -> smoke (x4) -> publish
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
     tarballs, AppImages, `.deb` and `.rpm` packages and gathers the three
     one-line installers. None of these has a secret. The macOS build is the
     unsigned `WordInk.app` from `scripts/build-app.sh`, zipped with `ditto` so
     the zip root is `WordInk.app/`, and packed into `WordInk-macos-arm64.dmg`
     (`packaging/macos/make-dmg.sh`). The Windows leg also compiles the Inno
     Setup installer (`packaging/windows/wordink.iss`) into
     `WordInk-Setup-windows-x64.exe`. `package-linux` downloads nfpm,
     appimagetool and the AppImage runtimes at pinned versions and checks
     their SHA-256 before running them.
   - `check-installers` installs what was packed on clean runners, with no
     secrets and no checkout: the `.deb` (and the AppImage, unpacked) on Ubuntu
     x86_64 and aarch64, the `.rpm` in a Fedora container, the Setup `.exe`
     silently (then its uninstaller) on Windows, and the `.dmg` mounted on
     macOS. Each checks `wordink --version` against the release version. A
     failure stops the run before anything is signed.
     That macOS leg runs on the self-hosted Mac (label `promax-macos`); to use
     the GitHub-hosted `macos-26` image instead, set the repository variable
     `WORDINK_MACOS_RUNNER` to `hosted`. The self-hosted runner only ever sees
     tag-triggered runs and dispatched dry runs of this workflow (dispatching
     needs write access), never pull requests.
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

## Release-cut checklist

The app version comes from the tag (the workflow stamps it into the build with
`scripts/set-crate-version.sh`), so no version bump is committed. Before
tagging:

- [ ] `apps/desktop/CHANGELOG.md` has a non-empty `## X.Y.Z` section, merged.
- [ ] The **Desktop** workflow is green on the commit to tag (it includes the
      icon-size check and the installer tests).
- [ ] A dry run on `master` is green: `gh workflow run desktop-release.yml
      --ref master -f dry_run=true` builds every installer and `check-installers`
      passes on all four legs.
- [ ] Tag `desktop-vX.Y.Z-rc.N`, approve `release`, then run the
      [per-OS update test](release-checklist.md#update-test-per-os) and the rest
      of the checklist on real machines.
- [ ] Tag the same commit `desktop-vX.Y.Z`, approve `release`, and confirm the
      `releases/latest/download/` links in the README resolve.

## What a release contains

For version `V` (`stable.json` only on final releases):

| File | Purpose |
| --- | --- |
| `wordink-V-linux-x64`, `wordink-V-linux-arm64` | bare executables, what the in-app updater installs |
| `wordink-V-linux-x64.sig`, `wordink-V-linux-arm64.sig` | raw 64-byte ed25519 signature of each executable |
| `wordink-V-linux-x64.tar.gz`, `wordink-V-linux-arm64.tar.gz` | tarball with the installer, desktop files and icons (made before signing, so it holds no `.sig`) |
| `wordink-V-macos-arm64.zip` | unsigned `WordInk.app` (zip root `WordInk.app/`); what `install-macos.sh` and the macOS updater link use |
| `wordink-V-windows-x64.zip` | unsigned build with `wordink.exe`; what `install.ps1` and the Windows updater link use |
| `WordInk-Setup-windows-x64.exe` | per-user Inno Setup installer (no admin, Start Menu shortcut, uninstaller), unsigned |
| `WordInk-macos-arm64.dmg` | disk image with `WordInk.app` and an Applications link, unsigned, not notarized |
| `WordInk-linux-x86_64.AppImage`, `WordInk-linux-aarch64.AppImage` | AppImage of the same executable (no bundled libraries, no self-update) |
| `wordink_amd64.deb`, `wordink_arm64.deb` | Debian/Ubuntu package (nfpm): `/usr/bin/wordink`, launcher entry, icons, systemd user unit |
| `wordink-x86_64.rpm`, `wordink-aarch64.rpm` | Fedora/openSUSE package with the same contents |
| `install.sh`, `install-macos.sh`, `install.ps1` | the one-line installers (Linux, macOS, Windows); users run them from `releases/latest/download/` |
| `wordink-V.sbom.cdx.json` | CycloneDX SBOM |
| `stable.json` | update manifest the stable-channel updater reads (final releases only) |
| `dev.json` | update manifest the Dev-channel updater reads (every release) |
| `SHA256SUMS` | sha256 of every other file above, in `sha256sum -c` format |
| `SHA256SUMS.sig` | raw 64-byte ed25519 signature of `SHA256SUMS` |

`SHA256SUMS` lists every published file except itself and `SHA256SUMS.sig`;
the workflow fails if the release has any other file or lacks one
(`apps/desktop/packaging/linux/check-release-files.sh` holds the list).

The installers and packages have no version in their names, so
`https://github.com/duketopceo/WordInk/releases/latest/download/<name>` (the
README's Download links) always fetches the newest one. GitHub resolves
`latest` to the newest **final** release only: a release candidate is a
prerelease and never `latest`, so its installers are only reachable from its
own release page (`releases/download/desktop-vX.Y.Z-rc.N/<name>`). The
versioned zips, tarballs and bare executables keep their names, which the
updater manifests and the one-line installers rely on; do not rename them.

The AUR template in `apps/desktop/packaging/aur` is filled from a release with
`render-pkgbuild.sh <version> SHA256SUMS` (it uses the two Linux tarballs).
Nothing publishes it to the AUR yet.

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

## Dry run (no release)

To check the build and packaging without a tag, dispatch the workflow on any
branch:

```sh
gh workflow run desktop-release.yml -R duketopceo/WordInk --ref <branch> -f dry_run=true
```

It builds as version `0.0.0-dryrun.<run number>` (channel dev), runs `build`,
`sbom`, `package-linux` and `check-installers`, then `dry-run-bundle` checks
the file set, writes an unsigned `SHA256SUMS` and uploads everything as the
`dry-run-release` workflow artifact. `gate`, `sign-linux`, `draft`,
`fetch-draft`, `smoke` and `publish` only run for a pushed `desktop-v*` tag,
so a dispatched run never asks for the `release` environment, never sees the
signing key, and creates no tag or release. `dry_run=false` fails in `prepare`.
The macOS leg runs on the self-hosted Mac here too (dispatching needs write
access to the repository).

## The Dev channel

Choosing "Dev" under Settings > Updates follows release candidates. The
updater reads
`https://github.com/duketopceo/WordInk/releases/download/desktop-dev/dev.json`
(stable keeps `releases/latest/download/stable.json`).

- `desktop-dev` is a rolling prerelease that holds only `dev.json`. The
  `publish` job creates it if it is missing, never marks it latest, and
  replaces the asset (`--clobber`) after each release is made public.
- A candidate (`-rc.N`) is built stamped `dev` and refreshes `dev.json` only.
  A final tag refreshes both `stable.json` and `dev.json`, so Dev never lags
  stable and a stable user who picks Dev is not moved to an older candidate.
- A Dev-stamped install follows `dev.json` with no switch prompt. Any channel
  switch whose target is older than the running build (for example Dev back
  to Stable while stable trails the candidate) shows
  "Switch to ... (installs an older version)" and installs nothing until the
  user confirms.
- `dev.json` is copied from the release's own asset, which `SHA256SUMS`
  lists and signs. Like `stable.json`, the manifest file carries no signature
  of its own: Linux updates are verified by the per-binary ed25519 signature,
  and macOS and Windows only link to the release. `publish` is the only job
  holding write access to the feed.
- Test it by hand: `write-manifest.sh <dist> dev` writes `dev.json`
  (`"channel":"dev"`); `apps/desktop/scripts/test-release-scripts.sh` covers it.

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
| Linux x86_64, aarch64 | `.deb`, `.rpm` | The package manager; the app never replaces a package-managed executable. |
| Linux x86_64, aarch64 | AppImage | None: download the new AppImage. |
| macOS (Apple silicon) | `.dmg` or zip with `WordInk.app`, **unsigned**; `install-macos.sh` | The app only links to the newest release. It never installs one. |
| Windows x64 | Setup `.exe` or zip with `wordink.exe`, **unsigned**; `install.ps1` | The app only links to the newest release. It never installs one. The Setup `.exe` and `install.ps1` install the same layout (`%LOCALAPPDATA%\Programs\WordInk`), so either updates the other's install. |

The tarball is assembled before signing and no longer contains `wordink.sig`,
which the unpacked `install.sh` still looks for. Until the bootstrap installer
(which verifies `SHA256SUMS.sig` and the tarball's hash before it unpacks
anything) replaces that path, an unpacked tarball installs only with
`WORDINK_INSTALL_UNVERIFIED=1`, or with the executable's `.sig` copied beside it
as `wordink.sig`.

macOS Gatekeeper and Windows SmartScreen will warn about the unsigned builds.
Signing and notarization are deferred until there are paid developer accounts;
until then, say so wherever a macOS or Windows download is offered, with the
first-launch steps: Windows SmartScreen **More info > Run anyway**; macOS
**System Settings > Privacy & Security > Open Anyway** after a first blocked
open (right-click > **Open** no longer skips Gatekeeper on macOS 15 and later).

## One-time setup (owner)

The `release` environment and the `WORDINK_LINUX_SIGNING_KEY` secret must exist
before the first tag. The commands are in the overnight report's "Needs you".
If the environment is missing when a tag is pushed, GitHub creates an
unprotected one on the first run, so create it first. Rotating the key is in
[`SECURITY.md`](../SECURITY.md).
