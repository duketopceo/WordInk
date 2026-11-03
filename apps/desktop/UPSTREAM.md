# Upstream: Hex

This directory is a copy of [Hex](https://github.com/anomalyco/hex) by Anomaly, under the MIT license (see `LICENSE` in this directory). It is not a GitHub fork. WordInk changes this copy directly, and pulls later Hex changes on purpose with `scripts/hex-sync.sh`.

| | |
|---|---|
| Source | https://github.com/anomalyco/hex |
| Tag | v3.2.0 |
| Commit | `00610c581a0501e02631b7d915d504dec3729b03` |
| Imported | 2026-11-02 |

## Not copied

These Hex paths were left out because WordInk does not use them:

- `site/` (Hex's website)
- `ios/` (the iOS app is paused; see branch `archive/ios-2026-11-03`)
- `prototypes/`, `skills/`, `release-notes/`
- `flake.nix`, `flake.lock`, `nix/`
- `.github/`, `.cloudflare/`, `.hivemind.toml` (Hex's CI and hosting; WordInk's CI lives in the repo root `.github/`)

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
| `github.com/duketopceo/WordInk` | "Send feedback" link and the "Download" link of the updater (`RELEASES_URL`) | Only when the user clicks. Placeholder until U14 publishes releases. |
| `v2.opencode.ai` | A docs link in the OpenCode catalog pane (`src/desktop/opencode_catalog.rs`) | Only when the user clicks. |
| User-defined hosts | QuickJS mode scripts using `fetch` (`src/personal_commands/fetch.rs`) | Only after the user allows that exact config in the Output pane. |
| `HEX_RELEASE_BASE_URL` | The updater, when the user sets this HTTPS env var | Off unless set. |
| `HEX_JEV_ENDPOINT` | Jev routing in `crates/router` (an Annotate experiment) | Off unless set. |

Dropped or neutralised in U2:

- `anoma.ly` (release manifests, dev notes, docs): the updater's origin and the dev-notes feed are now empty, so update checks do nothing until U14 sets a manifest origin in `src/updater.rs`. The "What's new" and fallback links point at the WordInk releases page. Docs links point at this repo.
- `opencode.ai` (the Jev routing gateway in `crates/router/src/routing/jev.rs`): there is no default endpoint now. Jev routing is off until the user sets `HEX_JEV_ENDPOINT`.
- Screen capture (`hex_capture`, Annotate): `capture.enabled` now defaults to off, and the "Annotate on by default" migration no longer switches it on. No capture work and no capture shortcut exist until the user turns it on.

Left as Hex's, to replace in U14: the macOS Developer ID requirement (`app/update-requirement.txt`, now a placeholder team id), the Linux release public key (`src/updater/linux.rs`, `SIGNING_KEY`, still Anomaly's), and the Windows Authenticode signer name. With no manifest origin none of them can be reached.
