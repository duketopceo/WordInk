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
   prepare -> gate -> build (x4), sbom -> package-linux -> sign-linux
           -> draft -> [smoke, once added] -> publish
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
     tarballs. None of these has a secret.
   - `sign-linux` waits for approval (next step), signs the bare Linux
     executables, writes `stable.json` (final tags only), lists every file in
     `SHA256SUMS`, signs that list, and verifies the result.
   - `draft` attests every file, creates the GitHub Release as a **draft** and
     checks the draft holds exactly the files in `SHA256SUMS`.
   - `smoke` is not there yet (it arrives with the install smoke test). It will
     sit between `draft` and `publish`, and a failed smoke keeps the release a
     draft.
   - `publish` only un-drafts the release. It checks out and runs nothing.

4. **Approve the `release` environment.** The run pauses at `sign-linux`;
   GitHub asks the owner to review the deployment. That approval is what lets
   the job read the Linux signing key, so check before approving that the run is
   for the tag you pushed and that the tagged commit is the reviewed one. Reject
   it for anything else.
5. Test the candidate: check the release on a real machine of each OS (see
   `docs/release-checklist.md` once it exists). If anything is wrong, fix it on
   `master` and cut `-rc.N+1`.
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
| `wordink-V-macos-arm64.zip`, `wordink-V-windows-x64.zip` | unsigned builds |
| `wordink-V.sbom.cdx.json` | CycloneDX SBOM |
| `stable.json` | update manifest the stable-channel updater reads (final releases only) |
| `dev.json` | update manifest the Dev-channel updater reads (every release) |
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
| macOS (Apple silicon) | zip with the bare executable, **unsigned** | The app only links to the newest release. It never installs one. |
| Windows x64 | zip with `wordink.exe`, **unsigned** | The app only links to the newest release. It never installs one. |

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
