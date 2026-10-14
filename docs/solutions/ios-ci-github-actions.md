# iOS app CI on GitHub Actions without a local Mac

`apps/ios` compiles and tests only in CI (the dev host has no Xcode). These are the
failure modes hit bringing the `ios` job up on `macos-15`, so the next round doesn't
re-derive them one CI cycle at a time.

## Recipe that works

```yaml
- run: brew install xcodegen
- run: xcodegen --spec project.yml
- run: |
    SIM=$(xcrun simctl list devices available --json | python3 -c \
      'import json,sys; devs=json.load(sys.stdin)["devices"]; \
       names=[d["name"] for r in devs.values() for d in r if d["name"].startswith("iPhone")]; \
       print(sorted(names)[-1])')
    xcodebuild -scheme <Scheme> -destination "platform=iOS Simulator,name=$SIM" \
      CODE_SIGN_IDENTITY="-" -derivedDataPath build test
```

## Pitfalls in order hit

- **`xcodebuild -scheme` needs a shared scheme.** XcodeGen only emits shared schemes from a
  top-level `schemes:` block in `project.yml`; without one, scheme lookup fails. The scheme's
  `test.targets` must name the test bundle.
- **Control widget type name is `ControlWidgetButton`**, not `ControlButton` (iOS 18).
- **`@Parameter(supportedContentTypes:)` is iOS 18+.** For an iOS 17 floor use a plain
  `@Parameter var f: IntentFile` and validate content yourself.
- **Hand-written Info.plists need `CFBundleExecutable` and `CFBundleVersion`** on both the app
  and any embedded `.appex`, or install fails with "missing or invalid CFBundleExecutable" /
  "bundleVersion must be set in placeholder attributes".
- **Keychain in hosted tests returns `errSecMissingEntitlement` (-34018)** under
  `CODE_SIGNING_ALLOWED=NO`: entitlements only embed when signing. Fix is ad-hoc signing —
  `CODE_SIGN_IDENTITY="-"` ("Sign to Run Locally") — plus a `keychain-access-groups`
  entitlement (`$(AppIdentifierPrefix)<bundle-id>`), and `GENERATE_INFOPLIST_FILE=YES` on the
  test bundle so it can be signed too. No explicit `kSecAttrAccessGroup` needed: the default
  group matches the declared one.
- **`URLRequest.httpBody` is nil inside a `URLProtocol`** when URLSession delivers the body
  via `httpBodyStream`; mock-protocol assertions must read the stream.
- **Typed `catch let e as SomeError` is non-exhaustive** — always append a generic `catch`.
- **A bare `catch {}` shadows `@State` properties named `error`** — the implicit binding wins;
  write `catch let thrown`.
- **Widget bundle version keys** must come from build settings (`MARKETING_VERSION`,
  `CURRENT_PROJECT_VERSION`) set on the extension target, not just the app.

## Verification posture

CI compiles + runs unit tests on a simulator. It does not exercise the mic, real keychain UX,
Shortcuts round-trips, or a live gateway — those stay manual on-device checks.
