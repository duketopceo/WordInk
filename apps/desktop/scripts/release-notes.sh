#!/usr/bin/env bash
# release-notes.sh <version> [changelog]: prints the body of the CHANGELOG.md
# section for <version> (default apps/desktop/CHANGELOG.md). The section starts
# at a "## <version>" heading (anything after the version and a space, such as
# a date, is allowed) and ends at the next "## " heading. For X.Y.Z-rc.N with
# no section of its own it falls back to the "## X.Y.Z" section. Fails when
# there is no section or it is empty, so a release cannot start without notes.
set -euo pipefail

version=${1:?Usage: release-notes.sh <version> [changelog]}
changelog=${2:-"$(cd "$(dirname "$0")/.." && pwd)/CHANGELOG.md"}
[ -f "$changelog" ] || { echo "::error::$changelog is missing" >&2; exit 1; }

section() { # <version>: the section body, trimmed of blank edges
  awk -v want="$1" '
    /^## / {
      heading = substr($0, 4)
      if (found) exit
      if (heading == want || index(heading, want " ") == 1) found = 1
      next
    }
    found { lines[++n] = $0 }
    END {
      first = 1; while (first <= n && lines[first] ~ /^[[:space:]]*$/) first++
      last = n; while (last >= first && lines[last] ~ /^[[:space:]]*$/) last--
      for (i = first; i <= last; i++) print lines[i]
    }
  ' "$changelog"
}

body=$(section "$version")
if [ -z "$body" ]; then
  case "$version" in *-rc.*) body=$(section "${version%%-rc.*}") ;; esac
fi
[ -n "$body" ] || { echo "::error::CHANGELOG.md has no non-empty '## $version' section" >&2; exit 1; }
printf '%s\n' "$body"
