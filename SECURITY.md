# Security Policy

## Reporting a vulnerability

Use GitHub private vulnerability reporting: open the repository's
**Security** tab and choose **Report a vulnerability**
(https://github.com/duketopceo/WordInk/security/advisories/new).

Please do not open a public issue or pull request for a vulnerability. You can
expect an acknowledgement within 7 days. Reports are handled on a best-effort
basis by the maintainer.

## Supported versions

Only the latest release receives security fixes.

| Version        | Supported |
| -------------- | --------- |
| latest release | yes       |
| older releases | no        |

## Release signing

Desktop releases come from `desktop-v*` tags (`.github/workflows/desktop-release.yml`).
Each release carries `SHA256SUMS`, a CycloneDX SBOM and build-provenance
attestations (`gh attestation verify <file> -R duketopceo/WordInk`).

- **Linux** executables are signed with an ed25519 key. The in-app updater
  refuses an update whose signature does not verify against the public key
  compiled into it, and the release tarball's `install.sh` checks the same
  signature before installing.
- **macOS and Windows** builds are not signed (no notarization, no
  Authenticode). Treat their downloads as unsigned and take them only from the
  official GitHub releases page. Their in-app updater only links to the newest
  release; it never downloads or installs anything.

How the Linux key is kept:

- **Private key.** It exists only as the secret `WORDINK_LINUX_SIGNING_KEY` of
  the protected `release` environment on GitHub, which requires the owner's
  approval before a job runs in it. Only the `sign-linux` job reads it, and that
  job runs one script on the build output as data. It is never committed.
- **Public key.** `apps/desktop/packaging/linux/wordink-signing.pub`, compiled
  into the updater (`SIGNING_KEY` in `apps/desktop/src/updater/linux.rs`) and
  embedded in `apps/desktop/packaging/linux/install-release.sh`; a unit test
  keeps the three equal. CODEOWNERS assigns the owner as reviewer for those
  paths and for `.github/workflows/**`. Owner approval is mandatory only while
  the default branch's protection rule or ruleset requires review from code
  owners.

### Rotating the key

1. Generate a new ed25519 keypair on a trusted machine:
   `openssl genpkey -algorithm ed25519 -out new.key` and
   `openssl pkey -in new.key -pubout -out wordink-signing.pub`.
2. Replace the public key in `apps/desktop/packaging/linux/wordink-signing.pub`,
   the `SIGNING_KEY` bytes in `apps/desktop/src/updater/linux.rs` and the
   `signing_key=` line in `apps/desktop/packaging/linux/install-release.sh`
   (the test `the_compiled_key_is_the_published_one` checks all three).
3. An installed app trusts only the key compiled into it, so it refuses an
   update signed by the new key. For a planned rotation, first publish a
   release whose app carries the new key (steps 1 and 2 on a commit that keeps
   `wordink-signing.pub` on the old key for this one release, with the test
   updated to match) and sign it with the old key, while the old secret is
   still in the `release` environment. Installed apps accept it and learn the
   new key. After a leak there is no such release: people reinstall from the
   tarball's `install.sh`, which carries the new key.
4. Replace the secret in the `release` environment with the new private key:
   `gh secret set WORDINK_LINUX_SIGNING_KEY --env release -R duketopceo/WordInk < new.key`.
5. Revoke the old key: delete it everywhere and publish a security advisory
   stating the date it stopped being valid.

If a private key may have leaked, report it as above, rotate immediately and
say so in the advisory.
