#!/usr/bin/env bash
# Pull a later Hex change into apps/desktop on purpose.
#
#   scripts/hex-sync.sh <hex-tag-or-commit> [--dry-run]
#
# Diffs Hex from the commit recorded in apps/desktop/UPSTREAM.md to the one
# named, limited to the paths WordInk copied, and applies the diff under
# apps/desktop/ with --3way. Conflicts stay in the working tree for review.
# Run it on a fresh branch; it never commits.
set -euo pipefail

usage() { echo "usage: $0 <hex-tag-or-commit> [--dry-run]" >&2; exit 2; }
[ $# -ge 1 ] || usage
target=$1
dry_run=0
[ "${2:-}" = "--dry-run" ] && dry_run=1

root=$(git rev-parse --show-toplevel)
desktop="$root/apps/desktop"
upstream_md="$desktop/UPSTREAM.md"
hex_url=${HEX_REPO_URL:-https://github.com/anomalyco/hex}

base=$(sed -n 's/^| Commit | `\([0-9a-f]\{40\}\)` |$/\1/p' "$upstream_md")
[ -n "$base" ] || { echo "no Commit row in $upstream_md" >&2; exit 1; }

# Paths WordInk did not copy (keep in step with UPSTREAM.md "Not copied").
excludes=(site ios prototypes skills release-notes flake.nix flake.lock nix .github .cloudflare .hivemind.toml)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git clone --quiet --filter=blob:none --no-checkout "$hex_url" "$work/hex"
git -C "$work/hex" fetch --quiet origin "$target" 2>/dev/null || true
head=$(git -C "$work/hex" rev-parse --verify "$target^{commit}")

pathspec=(.)
for p in "${excludes[@]}"; do pathspec+=(":(exclude)$p"); done

git -C "$work/hex" diff --binary "$base" "$head" -- "${pathspec[@]}" > "$work/hex.patch"
if [ ! -s "$work/hex.patch" ]; then
  echo "no changes between $base and $head in the copied paths"
  exit 0
fi

echo "Hex $base..$head: $(git -C "$work/hex" diff --stat "$base" "$head" -- "${pathspec[@]}" | tail -1)"
if [ "$dry_run" = 1 ]; then
  git -C "$root" apply --check --3way --directory=apps/desktop "$work/hex.patch" \
    && echo "dry run: applies cleanly" \
    || echo "dry run: conflicts expected; review after a real run"
  exit 0
fi

git -C "$root" apply --3way --directory=apps/desktop "$work/hex.patch" || {
  echo "applied with conflicts; resolve them, then update the Commit row in $upstream_md to $head" >&2
  exit 1
}
echo "applied; review the diff, then update the Commit row in $upstream_md to $head"
