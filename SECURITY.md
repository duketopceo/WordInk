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

## Release signing (planned)

Desktop release binaries are not signed yet, and the in-app updater does not
verify signatures. Today the repository's release workflow publishes npm
packages only; desktop builds on every platform are unsigned and the updater
only links to the GitHub release. Treat any desktop download as unsigned and
get it from the official GitHub releases page.

The plan, which is not yet implemented, is to sign Linux release artifacts with
an ed25519 key and have the updater verify the signature before installing.

- **Private key (planned).** It would exist only as a secret in a protected
  `release` environment on GitHub, require the owner's approval to deploy to,
  be read only by a separate signing job, and never be committed to the repo.
- **Public key (planned).** It would be a constant in the app's updater module
  (`apps/desktop/src/updater*`). CODEOWNERS assigns the owner as reviewer for
  changes to that path and to `.github/workflows/**`. Owner approval is
  mandatory only while the default branch's protection rule or ruleset
  requires review from code owners.

### Rotating the key (planned)

Once signing exists:

1. Generate a new ed25519 keypair on a trusted machine.
2. Replace the public key constant in `apps/desktop/src/updater*`.
3. Cut a release signed with the old key, while the old secret is still in the
   `release` environment, so that installed apps accept it and learn the new
   public key.
4. Replace the secret in the `release` environment with the new private key.
5. Revoke the old key: delete it everywhere and publish a security advisory
   stating the date it stopped being valid.

If a private key may have leaked, report it as above, rotate immediately and
say so in the advisory.
