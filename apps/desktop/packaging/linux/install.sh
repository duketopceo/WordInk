#!/bin/sh
# WordInk's one-line installer for Linux (x86_64, aarch64). It is published as
# a release asset, so the latest release serves it:
#
#   curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh | sh -s -- uninstall [--purge] [--yes]
#   ... | sh -s -- --force        install even over the same or a newer version
#
# What it does, in order:
#   1. Preflight, before any download: not root, a supported CPU, glibc 2.39+,
#      OpenSSL 3, curl.
#   2. Downloads SHA256SUMS and SHA256SUMS.sig and verifies the signature with
#      the ed25519 public key embedded below (the same key as
#      packaging/linux/wordink-signing.pub). The signature is a raw 64-byte
#      ed25519 signature over
#        "wordink.sha256sums.v1\n<lowercase hex sha256 of SHA256SUMS>\n"
#   3. Downloads the tarball for this CPU (its name comes from SHA256SUMS) and
#      checks its sha256 against SHA256SUMS before unpacking it.
#   4. Runs the unpacked install-release.sh, telling it which executable hash
#      was verified (WORDINK_BOOTSTRAP_VERIFIED); it re-hashes the executable.
#
# Environment:
#   WORDINK_INSTALL_BASE   where the release files live. Default
#                          https://github.com/duketopceo/WordInk/releases/latest/download
#                          https only, or http to 127.0.0.1, localhost or [::1]
#                          (the test harness and CI smoke jobs); a URL with
#                          user:password@ is refused.
#   WORDINK_INSTALL_TEST_PUBLIC_KEY
#                          TEST ONLY. A base64 ed25519 public key (DER, same
#                          form as the embedded one) used instead of the
#                          embedded key, so the test harness can sign fixtures
#                          with a throwaway key. The script refuses it unless
#                          WORDINK_INSTALL_BASE is a loopback address, so it
#                          can never redirect trust for a real download.
#
# The whole script runs from main() at the bottom, so a truncated download
# runs nothing, and no child process reads the script from stdin.

set -eu

signing_key=MCowBQYDK2VwAyEAupIraDuQFZun9b4ZGlL5P6j9klVOOFmXTzaPNuxkKAc=
default_base=https://github.com/duketopceo/WordInk/releases/latest/download
releases_page=https://github.com/duketopceo/WordInk/releases
min_glibc_major=2
min_glibc_minor=39

temporary=

fail() {
  echo "$1" >&2
  exit 1
}

cleanup() {
  status=$?
  if [ -n "$temporary" ]; then rm -rf "${temporary:?}"; fi
  exit "$status"
}

# Succeeds when the base's host is a loopback address.
loopback_host() {
  case "$1" in
    127.0.0.1 | localhost | "[::1]") return 0 ;;
    *) return 1 ;;
  esac
}

# Sets base, scheme and base_is_loopback from WORDINK_INSTALL_BASE.
check_base() {
  base=${WORDINK_INSTALL_BASE:-$default_base}
  case "$base" in
    *[[:space:]\"\'\\\`\$\<\>\|\;\&\*\?\#]*) fail "WORDINK_INSTALL_BASE contains characters that are not allowed in a URL." ;;
  esac
  case "$base" in
    https://*) scheme=https; rest=${base#https://} ;;
    http://*) scheme=http; rest=${base#http://} ;;
    *) fail "WORDINK_INSTALL_BASE must start with https:// (or http:// to a loopback address)." ;;
  esac
  authority=${rest%%/*}
  case "$authority" in
    *@*) fail "WORDINK_INSTALL_BASE must not contain a user name or password." ;;
    "") fail "WORDINK_INSTALL_BASE has no host." ;;
  esac
  # Strip :port (IPv6 literals keep their brackets).
  case "$authority" in
    "[::1]" | "[::1]":*) host="[::1]" ;;
    *) host=${authority%%:*} ;;
  esac
  base_is_loopback=0
  if loopback_host "$host"; then base_is_loopback=1; fi
  if [ "$scheme" = http ] && [ "$base_is_loopback" = 0 ]; then
    fail "WORDINK_INSTALL_BASE uses plain http to $host; only https, or http to 127.0.0.1, localhost or [::1], is allowed."
  fi
  base=${base%/}
}

preflight() {
  if [ "$(id -u)" = 0 ]; then
    fail "Run this as your desktop user, not root (it installs into your home directory)."
  fi
  os=$(uname -s)
  machine=$(uname -m)
  case "$os $machine" in
    "Linux x86_64") arch=x64 ;;
    "Linux aarch64") arch=arm64 ;;
    *) fail "WordInk supports Linux (x86_64, aarch64), macOS 15+ on Apple silicon and 64-bit Windows; this is $os $machine. See $releases_page" ;;
  esac
  if [ "$action" = install ]; then
    glibc=
    if command -v getconf >/dev/null 2>&1; then
      glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{ print $2 }') || glibc=
    fi
    if [ -z "$glibc" ] && command -v ldd >/dev/null 2>&1; then
      first=$(ldd --version 2>&1 | head -n 1) || first=
      case "$first" in
        *GLIBC* | *glibc*) glibc=$(printf '%s\n' "$first" | awk '{ print $NF }') ;;
      esac
    fi
    glibc_need="WordInk needs glibc $min_glibc_major.$min_glibc_minor or newer (Ubuntu 24.04, Debian 13, Fedora 40, Arch)"
    [ -n "$glibc" ] || fail "$glibc_need; this system's glibc was not found."
    glibc_major=${glibc%%.*}
    glibc_minor=${glibc#*.}
    glibc_minor=${glibc_minor%%.*}
    case "$glibc_major$glibc_minor" in
      *[!0-9]* | "") fail "$glibc_need; could not read glibc version \"$glibc\"." ;;
    esac
    if [ "$glibc_major" -lt "$min_glibc_major" ] ||
      { [ "$glibc_major" -eq "$min_glibc_major" ] && [ "$glibc_minor" -lt "$min_glibc_minor" ]; }; then
      fail "$glibc_need; this system has glibc $glibc."
    fi
  fi
  command -v openssl >/dev/null 2>&1 || fail "WordInk's installer needs OpenSSL 3 (the openssl command) to verify the download; install it and run this again."
  case "$(openssl version 2>/dev/null)" in
    "OpenSSL 3"*) ;;
    *) fail "WordInk's installer needs OpenSSL 3; this system has: $(openssl version 2>/dev/null | head -n 1)." ;;
  esac
  command -v curl >/dev/null 2>&1 || fail "WordInk's installer needs curl; install it and run this again."
  command -v sha256sum >/dev/null 2>&1 || fail "WordInk's installer needs sha256sum (coreutils)."
  command -v tar >/dev/null 2>&1 || fail "WordInk's installer needs tar."
}

# fetch <name> <destination> [progress]
fetch() {
  quiet=-sS
  if [ "${3:-}" = progress ] && [ -t 2 ]; then quiet=--progress-bar; fi
  # shellcheck disable=SC2086 # $quiet is one option
  curl -f -L $quiet --proto "=$scheme" --proto-redir "=$scheme" --retry 2 --connect-timeout 20 \
    -o "$2" "$base/$1" ||
    fail "Could not download $base/$1; check your connection, or download it by hand from $releases_page"
}

main() {
  action=install
  install_args=
  for argument in "$@"; do
    case "$argument" in
      install) action=install ;;
      uninstall) action=uninstall ;;
      --force | --purge | --yes | -y) install_args="$install_args $argument" ;;
      *) echo "Usage: sh -s -- [install [--force] | uninstall [--purge] [--yes]]" >&2; exit 2 ;;
    esac
  done

  check_base
  preflight

  key=$signing_key
  if [ -n "${WORDINK_INSTALL_TEST_PUBLIC_KEY:-}" ]; then
    [ "$base_is_loopback" = 1 ] || fail "WORDINK_INSTALL_TEST_PUBLIC_KEY is only for tests against a loopback WORDINK_INSTALL_BASE; refusing."
    key=$WORDINK_INSTALL_TEST_PUBLIC_KEY
  fi

  temporary=$(mktemp -d) || fail "Could not create a temporary directory."
  trap cleanup EXIT
  trap 'exit 1' HUP INT TERM

  fetch SHA256SUMS "$temporary/SHA256SUMS"
  fetch SHA256SUMS.sig "$temporary/SHA256SUMS.sig"
  [ "$(wc -c < "$temporary/SHA256SUMS.sig" | tr -d ' ')" = 64 ] || fail "SHA256SUMS.sig is not a 64-byte signature; signature failed, nothing was installed."
  printf -- '-----BEGIN PUBLIC KEY-----\n%s\n-----END PUBLIC KEY-----\n' "$key" > "$temporary/signing.pem"
  sums_sha=$(sha256sum "$temporary/SHA256SUMS" | cut -c1-64)
  printf 'wordink.sha256sums.v1\n%s\n' "$sums_sha" > "$temporary/message"
  openssl pkeyutl -verify -rawin -pubin -inkey "$temporary/signing.pem" \
    -in "$temporary/message" -sigfile "$temporary/SHA256SUMS.sig" > /dev/null 2>&1 ||
    fail "The release checksum list's signature failed verification; nothing was installed."

  # The tarball for this CPU, named by the signed list.
  names=$(awk -v arch="$arch" '
    NF == 2 && $1 ~ /^[0-9a-f]{64}$/ {
      name = $2
      sub(/^\*/, "", name)
      if (name ~ ("^wordink-[0-9A-Za-z.+_-]+-linux-" arch "\\.tar\\.gz$")) print $1 " " name
    }' "$temporary/SHA256SUMS")
  [ -n "$names" ] || fail "The signed release list has no $arch Linux build; see $releases_page"
  [ "$(printf '%s\n' "$names" | wc -l | tr -d ' ')" = 1 ] || fail "The signed release list names more than one $arch Linux build; see $releases_page"
  expected=${names%% *}
  tarball=${names#* }
  name=${tarball%.tar.gz}
  version=${name#wordink-}
  version=${version%-linux-"$arch"}

  if [ "$action" = install ]; then
    echo "Downloading WordInk $version for Linux $arch..."
  fi
  fetch "$tarball" "$temporary/$tarball" progress
  actual=$(sha256sum "$temporary/$tarball" | cut -c1-64)
  [ "$actual" = "$expected" ] || fail "$tarball does not match the signed checksum list; it was corrupted or tampered with, nothing was installed."

  mkdir "$temporary/unpacked"
  entries=$(tar -tzf "$temporary/$tarball" 2>/dev/null) || fail "$tarball could not be read; nothing was installed."
  bad=$(printf '%s\n' "$entries" | awk -v top="$name" '
    $0 == top || $0 == top "/" { next }
    index($0, top "/") != 1 || $0 ~ /(^|\/)\.\.(\/|$)/ { print "bad"; exit }')
  [ -z "$bad" ] || fail "$tarball holds files outside $name/; nothing was installed."
  tar -xzf "$temporary/$tarball" -C "$temporary/unpacked" --no-same-owner --no-same-permissions ||
    fail "$tarball could not be unpacked; nothing was installed."
  root="$temporary/unpacked/$name"
  for file in wordink install-release.sh; do
    if [ -L "$root/$file" ] || [ ! -f "$root/$file" ]; then
      fail "$tarball is missing $file; nothing was installed."
    fi
  done
  binary_sha=$(sha256sum "$root/wordink" | cut -c1-64)

  # stdin is the script itself under `curl | sh`; the installer must not read it.
  # shellcheck disable=SC2086 # install_args is a list of known options
  WORDINK_BOOTSTRAP_VERIFIED=$binary_sha sh "$root/install-release.sh" $action $install_args < /dev/null
}

main "$@"
