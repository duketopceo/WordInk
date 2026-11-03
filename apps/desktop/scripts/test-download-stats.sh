#!/bin/sh
# Runs download-stats.sh against the recorded API fixture. Never calls the
# live GitHub API: a stub `gh` on PATH fails the test if the script reaches it.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
fixture="$root/tests/fixtures/download-stats-releases.json"
command -v jq >/dev/null 2>&1 || { echo "download-stats test requires jq." >&2; exit 1; }

temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
mkdir "$temporary/bin"
printf '#!/bin/sh\necho "live gh call: $*" >&2\nexit 99\n' > "$temporary/bin/gh"
chmod 755 "$temporary/bin/gh"
PATH="$temporary/bin:$PATH"

out=$(sh "$root/scripts/download-stats.sh" --fixture "$fixture")

fail() {
  echo "FAIL: $1" >&2
  printf '%s\n' "$out" >&2
  exit 1
}
has() { printf '%s\n' "$out" | grep -Eq "$1" || fail "missing: $1"; }

# v1.0.1 block: one install per OS/arch from the tarball and zips.
block=$(printf '%s\n' "$out" | sed -n '/^v1\.0\.1$/,/^$/p')
for expected in 'linux-x64 +10$' 'linux-arm64 +4$' 'macos-arm64 +20$' 'windows-x64 +15$' \
  'installs +49$' 'updates \(bare Linux binary\) +9$'; do
  printf '%s\n' "$block" | grep -Eq "$expected" || fail "v1.0.1 missing: $expected"
done
has '^v1\.0\.0$'
has '^installs|^  installs +79$'
has 'updates \(bare Linux binary\) +15$'
# Drafts are not releases; signatures, checksums, manifests and the SBOM are
# not downloads that count (their counts in the fixture would inflate totals).
printf '%s\n' "$out" | grep -q 'draft' && fail "draft release listed"
printf '%s\n' "$out" | grep -Eq '(\.sig|SHA256SUMS|stable\.json|sbom)' && fail "skipped asset shown"
printf '%s\n' "$out" | grep -Eq ' (99|900|800|5000|700)$' && fail "skipped asset counted"

# A paginated API prints one array per page; both pages are read.
two_pages="$temporary/pages.json"
jq -c '.[0:1]' "$fixture" > "$two_pages"
jq -c '.[1:2]' "$fixture" >> "$two_pages"
out=$(sh "$root/scripts/download-stats.sh" --fixture "$two_pages")
has '^v1\.0\.1$'
has '^v1\.0\.0$'

# An empty release list is reported, not an error.
echo '[]' > "$temporary/empty.json"
out=$(sh "$root/scripts/download-stats.sh" --fixture "$temporary/empty.json")
has '^No releases\.$'

# Input jq rejects stops the report with a failure, not an empty success.
echo 'not json' > "$temporary/bad.json"
if sh "$root/scripts/download-stats.sh" --fixture "$temporary/bad.json" >/dev/null 2>&1; then
  fail "a malformed fixture should fail"
fi

# A missing fixture is an error.
if sh "$root/scripts/download-stats.sh" --fixture "$temporary/none.json" >/dev/null 2>&1; then
  fail "a missing fixture should fail"
fi

echo "download-stats: ok"
