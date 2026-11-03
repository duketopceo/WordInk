#!/usr/bin/env bash
# write-sums.sh <dist>: writes <dist>/SHA256SUMS, the sha256 of every file in
# <dist> except SHA256SUMS and SHA256SUMS.sig themselves, in `sha256sum -c`
# format (sorted, C locale). Run it last, once every other release file
# exists, and then sign it with `sign-linux.sh --sums`: the signed list is the
# trust root for every release file (tarballs, zips, SBOM, signatures and the
# update manifest included). Holds no secrets.
set -euo pipefail

dist=${1:?Usage: write-sums.sh <dist>}
cd "$dist"
files=()
while IFS= read -r name; do
  case "$name" in SHA256SUMS | SHA256SUMS.sig) continue ;; esac
  [ -f "$name" ] || { echo "::error::$name is not a regular file" >&2; exit 1; }
  files+=("$name")
done < <(LC_ALL=C ls -1A)
[ "${#files[@]}" -gt 0 ] || { echo "::error::no release files in $dist" >&2; exit 1; }
sha256sum -- "${files[@]}" > SHA256SUMS
echo "Wrote SHA256SUMS for ${#files[@]} files."
