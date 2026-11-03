#!/bin/sh
# Install counts per release and per OS/architecture, read from the public
# GitHub Releases API (`download_count` per asset). Nothing here runs in the
# app and nothing is sent anywhere: this is the owner's read-only view.
#
# Counting rules (R16):
#   - one archive per OS and architecture counts as an install: the Linux
#     tarball, and the macOS and Windows zips;
#   - a bare Linux binary is what the in-app updater downloads, so it is
#     reported on its own "updates" line, never as an install;
#   - signatures (.sig), SHA256SUMS(.sig), *.json manifests and the SBOM are
#     skipped.
#
# Usage: download-stats.sh [--fixture FILE]
#   --fixture FILE  read a saved API response instead of calling GitHub
#                   (tests use this; they never touch the live API).
set -eu

repo=duketopceo/WordInk
fixture=

while [ "$#" -gt 0 ]; do
  case "$1" in
    --fixture)
      [ "$#" -ge 2 ] || { echo "download-stats: --fixture needs a file" >&2; exit 2; }
      fixture=$2
      shift 2
      ;;
    -h | --help)
      sed -n '2,/^set -eu/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "usage: download-stats.sh [--fixture FILE]" >&2
      exit 2
      ;;
  esac
done

command -v jq >/dev/null 2>&1 || { echo "download-stats: jq is required" >&2; exit 1; }

if [ -n "$fixture" ]; then
  [ -r "$fixture" ] || { echo "download-stats: cannot read $fixture" >&2; exit 1; }
  releases=$(cat "$fixture")
else
  command -v gh >/dev/null 2>&1 || { echo "download-stats: gh is required" >&2; exit 1; }
  releases=$(gh api --paginate "repos/$repo/releases?per_page=100")
fi

# `gh api --paginate` prints one JSON array per page; slurp and join them.
printf '%s\n' "$releases" | jq -rs '
  def kind:
    if test("\\.sig$") or test("^SHA256SUMS") or test("\\.json$") or test("sbom"; "i") then "skip"
    elif test("\\.(tar\\.gz|tgz|zip)$") then "install"
    elif test("-linux-(x64|arm64)$") then "update"
    else "skip" end;
  def target:
    (capture("(?<os>linux|macos|windows)-(?<arch>x64|arm64)") | "\(.os)-\(.arch)") // "other";
  def counted:
    [ .assets[]
      | {kind: (.name | kind), target: (.name | target), count: (.download_count // 0)}
      | select(.kind != "skip") ];
  def sum(k): map(select(.kind == k) | .count) | add // 0;
  def targets(k):
    map(select(.kind == k)) | group_by(.target)
    | map({target: .[0].target, count: (map(.count) | add)}) | sort_by(.target);
  def lines($rows; k):
    $rows | targets(k)[] | "  \(.target)\t\(.count)";

  [ add // [] | .[] | select(.draft | not) | {tag: .tag_name, rows: counted} ] as $all
  | if ($all | length) == 0 then "No releases."
    else
      ( $all[]
        | "\(.tag)",
          ( lines(.rows; "install") ),
          "  installs\t\(.rows | sum("install"))",
          "  updates (bare Linux binary)\t\(.rows | sum("update"))",
          "" ),
      "All releases",
      ( lines([$all[].rows[]]; "install") ),
      "  installs\t\([$all[].rows[]] | sum("install"))",
      "  updates (bare Linux binary)\t\([$all[].rows[]] | sum("update"))"
    end
' | awk -F '\t' 'NF == 2 { printf "%-34s %s\n", $1, $2; next } { print }'
