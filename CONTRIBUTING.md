# Contributing

Thanks for helping. Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first.
Report security problems privately, as described in [SECURITY.md](SECURITY.md).

## Dev setup

| Package | Setup | Test |
| --- | --- | --- |
| `apps/desktop` (Rust) | `cargo build` in `apps/desktop` | `cargo test` |
| `crates/*` (Rust workspace) | `cargo build --workspace` | `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` |
| `sdks/python` | `cd sdks/python && uv sync` (add `--extra mic` for the microphone) | `uv run pytest` |
| `packages/*`, `apps/docs` (pnpm) | `pnpm install` at the repo root | `pnpm test`, `pnpm typecheck`, `pnpm build` |

`apps/ios` and `sdks/ios` have their own READMEs. CI (`.github/workflows/`) runs
the same commands, so a green local run is a good predictor.

User-visible changes to a published package need a changeset: `pnpm changeset`.

## Pull requests

- Keep a PR to one change, and add or update tests with it.
- Describe what changed and why. Fill in the PR template.

## Sign-off (DCO), no CLA

We use the [Developer Certificate of Origin](https://developercertificate.org/).
There is no CLA. Sign every commit with `git commit -s`, which adds:

    Signed-off-by: Your Name <you@example.com>

The name and email must match the commit author. To fix an unsigned commit, run
`git commit --amend -s`; for several, `git rebase --signoff <base>`.

## Clean-room rule

Do not copy code, strings or assets from GPL-licensed projects such as
TypeWhisper into WordInk, and do not translate their code. WordInk is MIT, and
a copy would taint it. Ideas are fine; see [docs/clean-room.md](docs/clean-room.md)
for how to write an inspired feature from scratch.

Code ported from MIT-licensed projects must keep its notice and be listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
