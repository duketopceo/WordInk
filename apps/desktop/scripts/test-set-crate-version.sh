#!/bin/sh
# Checks that set-crate-version.sh accepts X.Y.Z and X.Y.Z-rc.N and refuses
# every other shape, on a scratch copy of Cargo.toml and Cargo.lock.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
printf '[package]\nname = "hex"\nversion = "1.0.0"\n' >"$tmp/Cargo.toml"
printf '[[package]]\nname = "hex"\nversion = "1.0.0"\n' >"$tmp/Cargo.lock"

for version in 1.0.0-rc.1 1.2.3 1.0.0-rc.12; do
  sh "$here/set-crate-version.sh" "$tmp" "$version" 2>/dev/null || {
    echo "Refused valid version $version." >&2
    exit 1
  }
  grep -q "^version = \"$version\"" "$tmp/Cargo.toml"
  grep -q "^version = \"$version\"" "$tmp/Cargo.lock"
done

for version in 1.0 1 v1.0.0 1.0.0-beta.1 1.0.0-rc 1.0.0-rc.0 1.0.0-rc.1.2 0.0.0-dev-1 01.0.0 ''; do
  if sh "$here/set-crate-version.sh" "$tmp" "$version" 2>/dev/null; then
    echo "Accepted invalid version '$version'." >&2
    exit 1
  fi
done
echo "set-crate-version.sh: ok"
