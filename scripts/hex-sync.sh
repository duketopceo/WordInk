#!/usr/bin/env bash
# Pull a later Hex change into apps/desktop on purpose.
#
#   scripts/hex-sync.sh <hex-tag-or-commit> [--dry-run]
#
# Diffs Hex from the commit recorded in apps/desktop/UPSTREAM.md to the one
# named, limited to the paths WordInk copied, and applies it one file at a
# time under apps/desktop/. Each file is tried with `git apply --3way`; when
# that can't work (WordInk renamed or deleted the file, or its blobs are
# missing), it falls back to `git apply --reject`, so the hunks that fit land
# and a `.rej` file marks the rest. (git refuses --3way and --reject in one
# call, hence the two steps.) The summary says exactly what was and wasn't
# applied. A real run needs a clean tree (staged or not) and never commits;
# run it on a fresh branch. `--dry-run` changes nothing and lists the files
# that would fail.
set -euo pipefail

usage() { echo "usage: $0 <hex-tag-or-commit> [--dry-run]" >&2; exit 2; }
[ $# -ge 1 ] && [ $# -le 2 ] || usage
target=$1
dry_run=0
case "${2:-}" in
  "") ;;
  --dry-run) dry_run=1 ;;
  *) echo "unknown argument: $2" >&2; usage ;;
esac

root=$(git rev-parse --show-toplevel)
desktop="$root/apps/desktop"
upstream_md="$desktop/UPSTREAM.md"
hex_url=${HEX_REPO_URL:-https://github.com/anomalyco/hex}

if [ "$dry_run" = 0 ]; then
  if ! git -C "$root" diff --quiet || ! git -C "$root" diff --cached --quiet; then
    echo "the working tree has uncommitted changes; commit or discard them first (a sync is easier to review on its own)" >&2
    exit 1
  fi
fi

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

# No rename detection: an upstream rename is a delete plus an add, each of
# which applies (or fails) on its own path.
files=()
while IFS= read -r -d '' file; do files+=("$file"); done \
  < <(git -C "$work/hex" diff --no-renames --name-only -z "$base" "$head" -- "${pathspec[@]}")
if [ ${#files[@]} -eq 0 ]; then
  echo "no changes between $base and $head in the copied paths"
  exit 0
fi

echo "Hex $base..$head: ${#files[@]} files changed in the copied paths"

clean=() conflicted=() partial=() failed=() fine=() risky=()
i=0
for file in "${files[@]}"; do
  i=$((i + 1))
  patch="$work/file-$i.patch"
  git -C "$work/hex" diff --binary --no-renames "$base" "$head" -- "$file" > "$patch"
  apply=(git -C "$root" apply --directory=apps/desktop)

  if [ "$dry_run" = 1 ]; then
    if out=$("${apply[@]}" --check --3way "$patch" 2>&1) \
      || out=$("${apply[@]}" --check "$patch" 2>&1); then
      fine+=("$file")
    else
      risky+=("$file")
      echo "would not apply cleanly: $file" >&2
      printf '%s\n' "$out" | sed 's/^/    git: /' >&2
    fi
    continue
  fi

  if out=$("${apply[@]}" --3way "$patch" 2>&1); then
    clean+=("$file")
  elif printf '%s' "$out" | grep -q 'with conflicts'; then
    conflicted+=("$file")
    printf '%s\n' "$out" | sed 's/^/    git: /' >&2
  else
    # --3way applied nothing for this file (it is gone or renamed here, or
    # a blob is missing). Land the hunks that fit and reject the rest.
    first=$(printf '%s' "$out" | head -n 3)
    if out=$("${apply[@]}" --reject "$patch" 2>&1); then
      clean+=("$file")
    elif [ -e "$desktop/$file.rej" ]; then
      partial+=("$file")
      echo "partly applied: $file (rejected hunks in apps/desktop/$file.rej)" >&2
      printf '%s\n' "$out" | sed 's/^/    git: /' >&2
    else
      failed+=("$file")
      echo "not applied: $file" >&2
      printf '%s\n%s\n' "$first" "$out" | sed 's/^/    git: /' >&2
    fi
  fi
done

if [ "$dry_run" = 1 ]; then
  echo "dry run: ${#fine[@]} files would apply, ${#risky[@]} would fail or conflict"
  if [ ${#risky[@]} -gt 0 ]; then
    echo "would fail (WordInk likely renamed, deleted or changed them):"
    printf '  %s\n' "${risky[@]}"
  fi
  exit 0
fi

echo "applied cleanly: ${#clean[@]}; with conflict markers: ${#conflicted[@]}; partly applied (.rej): ${#partial[@]}; not applied: ${#failed[@]}"
if [ ${#conflicted[@]} -gt 0 ]; then
  echo "conflict markers in:"; printf '  apps/desktop/%s\n' "${conflicted[@]}"
fi
if [ ${#partial[@]} -gt 0 ]; then
  echo "applied except the hunks in these .rej files:"; printf '  apps/desktop/%s.rej\n' "${partial[@]}"
fi
if [ ${#failed[@]} -gt 0 ]; then
  echo "nothing applied for:"; printf '  apps/desktop/%s\n' "${failed[@]}"
fi
if [ ${#conflicted[@]} -gt 0 ] || [ ${#partial[@]} -gt 0 ] || [ ${#failed[@]} -gt 0 ]; then
  echo "resolve these, delete the .rej files, then update the Commit row in $upstream_md to $head" >&2
  exit 1
fi
echo "applied; review the diff, then update the Commit row in $upstream_md to $head"
