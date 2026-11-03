# Contributing

Thanks for helping. Please read the [Code of Conduct](CODE_OF_CONDUCT.md) first.
Report security problems privately, as described in [SECURITY.md](SECURITY.md).

## Dev setup

| Package | Setup | Test |
| --- | --- | --- |
| `crates/*` (Rust workspace) | `cargo build --workspace` | `cargo test --workspace`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` |
| `sdks/python` | `cd sdks/python && uv sync` (add `--extra mic` for the microphone) | `uv run pytest` |
| `packages/*`, `apps/docs` (pnpm) | `pnpm install` at the repo root | `pnpm test`, `pnpm typecheck`, `pnpm build` |

CI (`.github/workflows/`) runs
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
`git commit --amend -s`; for several, `git rebase --signoff <base>`. Both
write your configured Git identity into the trailer while keeping the original
author, so if your configured name or email differs from the author's, set
`GIT_COMMITTER_NAME` and `GIT_COMMITTER_EMAIL` to the author's values for that
command.

## Clean-room rule

Do not copy code, strings or assets from GPL-licensed projects such as
TypeWhisper into WordInk, and do not translate their code. WordInk is MIT, and
a copy would taint it. Ideas are fine; see [docs/clean-room.md](docs/clean-room.md)
for how to write an inspired feature from scratch.

Code ported from MIT-licensed projects must keep its notice and be listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
