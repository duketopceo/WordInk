# Upstream: Hex

This directory is a copy of [Hex](https://github.com/anomalyco/hex) by Anomaly, under the MIT license (see `LICENSE` in this directory). It is not a GitHub fork. WordInk changes this copy directly, and pulls later Hex changes on purpose with `scripts/hex-sync.sh`.

| | |
|---|---|
| Source | https://github.com/anomalyco/hex |
| Tag | v3.3.0 |
| Commit | `753b078972b2f7912063db182b8b66b7c8d45ffc` |
| Imported | 2026-11-02 (v3.2.0); synced to v3.3.0 on 2026-11-04 |

## Not copied

These Hex paths were left out because WordInk does not use them:

- `site/` (Hex's website)
- `ios/` (the iOS app is paused; see branch `archive/ios-2026-11-03`)
- `prototypes/`, `skills/`, `release-notes/`
- `flake.nix`, `flake.lock`, `nix/`
- `.github/`, `.cloudflare/`, `.hivemind.toml` (Hex's CI and hosting; WordInk's CI lives in the repo root `.github/`)

## Where WordInk differs from Hex after the v3.3.0 sync

- Speech model picker: Hex's new compare and full-list views, with WordInk's Cloud group (Groq, OpenRouter, gateway) added to the full list.
- First run: WordInk's short first run (Skip on the demo, Finish later) stays; Hex's Skip on the practice page and its `onboarding_skip` check are not adopted. Hex's "Continue without a model" is.
- Terminal paste: WordInk's Auto / Always / Never (`terminal_paste`) stays in place of Hex's `paste_with_shift` bool, which only migrates.
- Default dictation shortcut: WordInk's `dictation_default_for` stays; Hex's KDE Plasma case (#898) feeds its GNOME branch.
- Policy locks (`policy.rs`, macOS managed preferences) read the `io.github.duketopceo.wordink` domain.
- Version stays 1.0.0 (not Hex's 3.3.0); `ModelDefinition.license` and the licence badge stay (GigaAM v3: MIT).

## Pulling a later Hex change

```sh
scripts/hex-sync.sh <hex-tag-or-commit>        # from the repo root
```

The script diffs Hex from the recorded commit to the one you name, limited to the copied paths, and applies the diff under `apps/desktop/` on a review branch. Conflicts stay in the working tree for review. Update the Commit row above when a sync lands.

## Outbound hosts

Every network host the desktop app can contact, audited in U2 (2026-11-02). Hosts Hex used that WordInk dropped are listed at the end.

| Host | Used by | When |
|---|---|---|
| `api.groq.com` | Cloud transcription (`src/transcription/cloud.rs`): sends the dictation audio (a 16 kHz mono WAV) with the user's Groq key | On each dictation when a Groq key is set and the engine is auto or groq. Never when the engine is local or no key resolves. |
| `huggingface.co` | Model downloads (`src/transcription/models.rs`, `src/net/download.rs`): the speech models, the Silero VAD model, imported models | Only when the user installs a model. Kept on purpose. |
| `127.0.0.1` / `localhost` | The owner-only local API, the control CLI, the MCP server, a local OpenCode server | Loopback only. |
| `github.com/duketopceo/WordInk` | "Send feedback" link and the "Download" link of the updater (`RELEASES_URL`) | Only when the user clicks. |
| `opencode.ai/v2/install` | An error message (`src/desktop/opencode_service.rs`) that names where to update OpenCode 2 | Text only; WordInk never contacts it. |
| `v2.opencode.ai` | A docs link in the OpenCode catalog pane (`src/desktop/opencode_catalog.rs`) | Only when the user clicks. |
| User-defined hosts | QuickJS mode scripts using `fetch` (`src/personal_commands/fetch.rs`) | Only after the user allows that exact config in the Output pane. |
| `HEX_RELEASE_BASE_URL` | The updater, when the user sets this HTTPS env var | Off unless set. |
| `HEX_JEV_ENDPOINT` | Jev routing in `crates/router` (an Annotate experiment) | Off unless set. |

Dropped or neutralised in U2:

- `anoma.ly` (release manifests, dev notes, docs): WordInk releases on GitHub (`desktop-vX.Y.Z` tags; the rolling `desktop-dev` prerelease feeds the Dev channel). The updater reads `stable.json` and `dev.json` from there; see [docs/releasing.md](../../docs/releasing.md). Docs links point at this repo.
- `opencode.ai` (the Jev routing gateway in `crates/router/src/routing/jev.rs`): there is no default endpoint now. Jev routing is off until the user sets `HEX_JEV_ENDPOINT`.
- Screen capture (`hex_capture`, Annotate): `capture.enabled` now defaults to off, and the "Annotate on by default" migration no longer switches it on. No capture work and no capture shortcut exist until the user turns it on.

Release signing: Linux updates verify WordInk's ed25519 key. macOS and Windows builds are unsigned and only link to the newest release, so Hex's Developer ID requirement and Authenticode signer are unused.
