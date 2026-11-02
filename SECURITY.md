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

## Release signing key

Linux release artifacts are signed with an ed25519 key, and the in-app updater
verifies the signature before installing. macOS and Windows builds are not
signed yet; their in-app updater only links to the GitHub release.

- **Private key.** It exists only as a secret in a protected `release`
  environment on GitHub. Deploying to that environment needs the owner's
  approval, and only the separate signing job reads the secret. It is never
  committed or stored in the repo.
- **Public key.** It is a constant in the app's updater module
  (`apps/desktop/src/updater*`). CODEOWNERS requires the owner's review for
  changes to that path and to `.github/workflows/**`.

### Rotating the key

1. Generate a new ed25519 keypair on a trusted machine.
2. Replace the public key constant in `apps/desktop/src/updater*`.
3. Replace the secret in the `release` environment with the new private key.
4. Cut a release. It must still be signed with the old key so that installed
   apps accept it and learn the new public key.
5. Revoke the old key: delete it everywhere and publish a security advisory
   stating the date it stopped being valid.

If the private key may have leaked, report it as above, rotate immediately and
say so in the advisory.
