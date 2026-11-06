# Signing macOS and Windows builds

WordInk's macOS and Windows builds ship unsigned today. Linux releases are signed with an ed25519 key and self-update. This page lists what signing needs, what already exists in the tree, and the order to switch it on. Nothing here can run until the paid accounts exist.

## What it needs

| Platform | Account | Secrets the release workflow would read |
|---|---|---|
| macOS | Apple Developer Program membership, a Developer ID Application certificate, an App Store Connect API key for notarization | `MACOS_CERTIFICATE_P12` (base64), `MACOS_CERTIFICATE_PASSWORD`, `MACOS_TEAM_ID`, `MACOS_NOTARY_KEY` (base64 `.p8`), `MACOS_NOTARY_KEY_ID`, `MACOS_NOTARY_ISSUER` |
| Windows | A code-signing service or certificate trusted by SmartScreen (for example Azure Artifact Signing, or an OV/EV certificate on a hardware token or cloud HSM) | Depends on the service: for Azure Artifact Signing, `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_SIGNING_ACCOUNT`, `AZURE_CERT_PROFILE` with OIDC federation instead of a client secret |

Keep every signing secret in the protected `release` environment, like `WORDINK_LINUX_SIGNING_KEY`, so only an owner-approved tag run can read it.

## What already exists

- `apps/desktop/scripts/macos-signing.sh`: finds the Developer ID identity (`HEX_CODESIGN_IDENTITY` or the team's identity in the keychain) and submits to `notarytool` with either a keychain profile (`HEX_NOTARY_PROFILE`) or an API key (`HEX_NOTARY_KEY`, `HEX_NOTARY_KEY_ID`, `HEX_NOTARY_ISSUER`).
- `apps/desktop/scripts/notarize-app.sh <app> <dmg> <volume>`: notarizes and staples the app, then builds, signs, notarizes and staples the dmg. It needs `HEX_TEAM_ID`.
- `apps/desktop/scripts/build-app.sh`: builds unsigned when `HEX_RELEASE_UNSIGNED=1`, which the release workflow sets today.
- `apps/desktop/src/updater/macos.rs`: verifies a Developer ID signature and team before swapping the bundle.
- `apps/desktop/src/updater/windows.rs`: verifies an Authenticode signer before staging an update. `AUTHENTICODE_SIGNER` is a placeholder until a certificate exists, so installed copies only open the download.

## Switching it on

### macOS

1. Create the Developer ID Application certificate and an App Store Connect API key with the Developer role.
2. Add the macOS secrets above to the `release` environment.
3. In `.github/workflows/desktop-release.yml`, on the macOS build leg only:
   1. Import the certificate into a temporary keychain when `MACOS_CERTIFICATE_P12` is set.
   2. Build without `HEX_RELEASE_UNSIGNED` when it is set, and with it otherwise.
   3. Replace the `make-dmg.sh` step with `notarize-app.sh` when the secrets are set, so an unsigned dry-run still produces a plain dmg.
   4. Delete the temporary keychain in an `always()` step.
4. Gate each new step on the secret being present (`if: env.MACOS_CERTIFICATE_P12 != ''` with the secret mapped into `env`), so forks and dry-runs stay green.
5. Set the team ID the updater expects in `updater/macos.rs`, then enable the in-app bundle swap.
6. Verify: `codesign --verify --deep --strict`, `spctl --assess --type execute` and `xcrun stapler validate` pass on the downloaded app and dmg, on a Mac that never saw the build.

### Windows

1. Set up the signing service and its OIDC trust for this repository's `release` environment.
2. Add the Windows secrets to the `release` environment.
3. In `desktop-release.yml`, on the Windows leg, sign `wordink.exe` before packaging and `WordInk-Setup-windows-x64.exe` after building it, each gated on the secret being present.
4. Put the certificate's subject in `AUTHENTICODE_SIGNER` (`updater/windows.rs`).
5. Turn on the installed-update path the owner described on #113:
   - stage and verify the update;
   - replace files after the running app exits and restart on the new version;
   - keep `%APPDATA%\wordink` (settings, history, models);
   - keep the download fallback when no signer is configured;
   - never weaken the signature or hash checks;
   - make the app, the CLI, the installer and Apps & Features report the same version.
6. Verify with the #113 acceptance criteria:
   - an isolated older-to-newer installed-app update restarts, keeps user data and reports the new version;
   - a bad signature or hash leaves the old install untouched with an actionable error;
   - a real signed installation runs the in-app update path. `scripts/test-windows-installer.ps1` covers installer fixtures only.

## Until then

The READMEs and the download page explain the first launch of an unsigned build (macOS Privacy & Security › Open Anyway; Windows SmartScreen › More info › Run anyway). The updater opens the new installer's download on both systems.
