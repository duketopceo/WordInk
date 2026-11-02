#!/usr/bin/env bash
# Report personal or machine-specific strings left in the repo.
#
# usage: scripts/scrub-check.sh <expressions-file> [--history]
#
# The expressions file uses the git-filter-repo --replace-text format, one per line:
#   literal==>replacement          (also: a bare literal with no "==>")
#   regex:pattern==>replacement    (PCRE; keep to syntax Python's re shares)
# Blank lines and lines starting with "#" are ignored. Only the left-hand side is searched.
# Keep the file OUTSIDE the repository: it is the list of values that must not appear in it.
#
# Without --history: search the tracked files at HEAD (git grep). Exit 1 if anything matches.
# With --history:    search every added and removed line in `git log -p master`. Exit 1 on a match.
set -euo pipefail

usage() { echo "usage: ${0##*/} <expressions-file> [--history]" >&2; exit 2; }

file=""
history=0
for arg in "$@"; do
  case "$arg" in
    --history) history=1 ;;
    -h|--help) usage ;;
    *) [[ -z "$file" ]] && file="$arg" || usage ;;
  esac
done
[[ -n "$file" && -r "$file" ]] || usage

cd "$(git rev-parse --show-toplevel)"

# Parse into parallel arrays: kind (F fixed, P pcre) and pattern.
kinds=(); pats=()
while IFS= read -r line || [[ -n "$line" ]]; do
  [[ -z "${line//[[:space:]]/}" || "$line" == \#* ]] && continue
  lhs="${line%%==>*}"
  case "$lhs" in
    regex:*) kinds+=(P); pats+=("${lhs#regex:}") ;;
    glob:*)  echo "skipping unsupported glob: expression" >&2 ;;
    *)       kinds+=(F); pats+=("$lhs") ;;
  esac
done < "$file"

# Map the validated kind to an explicit grep flag, so nothing is interpolated into an option.
flag_for() { [[ "$1" == P ]] && echo -P || echo -F; }

total=0
if (( history )); then
  log=$(mktemp); trap 'rm -f "$log"' EXIT
  git log -p master --format= > "$log"
  for i in "${!pats[@]}"; do
    # Only count patch lines (added or removed), not context.
    n=$(grep -E '^[+-]' "$log" | grep -c "$(flag_for "${kinds[$i]}")" -e "${pats[$i]}" -- || true)
    (( total += n )) || true
    (( n )) && echo "history: $n changed line(s) match expression #$((i+1))"
  done
  echo "history total: $total matching changed line(s)"
else
  for i in "${!pats[@]}"; do
    out=$(git grep -n -I "$(flag_for "${kinds[$i]}")" -e "${pats[$i]}" -- . ':(exclude)scripts/scrub-check.sh' || true)
    if [[ -n "$out" ]]; then
      n=$(printf '%s\n' "$out" | wc -l)
      (( total += n )) || true
      echo "expression #$((i+1)): $n hit(s)"
      printf '%s\n' "$out" | cut -d: -f1,2 | sed 's/^/  /'
    fi
  done
  echo "tree total: $total hit(s)"
fi
(( total == 0 ))
