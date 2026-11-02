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
- `ios/` (WordInk has its own iOS app in `apps/ios`)
- `prototypes/`, `skills/`, `release-notes/`
- `flake.nix`, `flake.lock`, `nix/`
- `.github/`, `.cloudflare/`, `.hivemind.toml` (Hex's CI and hosting; WordInk's CI lives in the repo root `.github/`)

## Pulling a later Hex change

```sh
scripts/hex-sync.sh <hex-tag-or-commit>        # from the repo root
```

The script diffs Hex from the recorded commit to the one you name, limited to the copied paths, and applies the diff under `apps/desktop/` on a review branch. Conflicts stay in the working tree for review. Update the Commit row above when a sync lands.
