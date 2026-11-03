#!/bin/sh
# Installs the released WordInk.app into ~/Applications. Never uses sudo.
#
#   curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install-macos.sh | sh
#   ... | sh -s -- --force                  reinstall even when up to date
#   ... | sh -s -- uninstall [--purge]      remove the app (--purge: and its data)
#
# WORDINK_INSTALL_BASE overrides the download base (https, or http to loopback).
set -eu

if [ -z "${HOME:-}" ]; then
  echo 'WordInk: HOME is not set.' >&2
  exit 1
fi
repo_url=https://github.com/duketopceo/WordInk/releases
base=${WORDINK_INSTALL_BASE:-$repo_url/latest/download}
base=${base%/}
bundle_id=io.github.duketopceo.wordink
apps_dir=$HOME/Applications
app=$apps_dir/WordInk.app
support_dir=$HOME/Library/Application\ Support/wordink

work=
stage=
backup=
cleanup() {
  [ -z "$work" ] || rm -rf "$work"
  [ -z "$stage" ] || rm -rf "$stage"
  if [ -n "$backup" ] && [ -d "$backup" ]; then
    # An interrupted swap: put the previous app back.
    [ -d "$app" ] || mv "$backup" "$app" 2>/dev/null || true
    rm -rf "$backup"
  fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

fail() {
  printf 'WordInk: %s\n' "$*" >&2
  exit 1
}

if [ "$(id -u)" -eq 0 ]; then
  fail "run this as your normal user, not root; it installs into ~/Applications and never needs sudo."
fi

mode=install
force=0
purge=0
assume_yes=0
for arg in "$@"; do
  case "$arg" in
    install) mode=install ;;
    uninstall) mode=uninstall ;;
    --force) force=1 ;;
    --purge) purge=1; mode=uninstall ;;
    --yes) assume_yes=1 ;;
    *) fail "unknown option '$arg'; use --force, uninstall, --purge or --yes." ;;
  esac
done

unsupported() {
  printf 'WordInk for macOS needs an Apple silicon Mac on macOS 15 or newer (also supported: Linux x86_64/aarch64, Windows x64); see %s\n' "$repo_url" >&2
  exit 1
}

check_platform() {
  [ "$(uname -s)" = Darwin ] || unsupported
  arch=$(uname -m)
  if [ "$arch" != arm64 ]; then
    # A shell running under Rosetta reports x86_64 on an Apple silicon Mac.
    [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ] || unsupported
  fi
  major=$(sw_vers -productVersion 2>/dev/null | cut -d. -f1)
  case "$major" in
    '' | *[!0-9]*) unsupported ;;
  esac
  [ "$major" -ge 15 ] || unsupported
}

check_base() {
  case "$base" in
    *@* | *[[:space:]]*) fail "WORDINK_INSTALL_BASE must not contain '@' or spaces; unset it to use GitHub Releases." ;;
    https://*) return 0 ;;
    http://*) ;;
    *) fail "WORDINK_INSTALL_BASE must be https (http only to 127.0.0.1 or localhost); unset it to use GitHub Releases." ;;
  esac
  authority=${base#http://}
  authority=${authority%%/*}
  host=${authority%%:*}
  port=${authority#"$host"}
  case "$host" in
    127.0.0.1 | localhost) ;;
    *) fail "WORDINK_INSTALL_BASE must be https (http only to 127.0.0.1 or localhost); unset it to use GitHub Releases." ;;
  esac
  if [ -n "$port" ]; then
    digits=${port#:}
    case "$digits" in
      '' | *[!0-9]*) fail "WORDINK_INSTALL_BASE has an invalid port; unset it to use GitHub Releases." ;;
    esac
  fi
}

fetch() { # fetch <name> <dest> [progress]
  if [ "${3:-}" = progress ] && [ -t 2 ]; then
    curl -fL --progress-bar --proto '=https,http' --proto-redir '=https' --retry 2 -o "$2" "$base/$1" ||
      fail "could not download $1 from $base; check your connection and try again."
  else
    curl -fsL --proto '=https,http' --proto-redir '=https' --retry 2 -o "$2" "$base/$1" ||
      fail "could not download $1 from $base; check your connection and try again."
  fi
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    sha256sum "$1" | awk '{ print $1 }'
  fi
}

plist_version() { # plist_version <app>
  defaults read "$1/Contents/Info" CFBundleShortVersionString 2>/dev/null ||
    /usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$1/Contents/Info.plist" 2>/dev/null ||
    true
}

# newer <a> <b>: success when version a is strictly newer than b. X.Y.Z with an
# optional -rc.N: a release outranks its own release candidates.
newer() {
  awk -v a="$1" -v b="$2" '
    function parse(v, out,    core, pre, n, i, parts) {
      pre = ""
      i = index(v, "-")
      core = v
      if (i > 0) { core = substr(v, 1, i - 1); pre = substr(v, i + 1) }
      n = split(core, parts, ".")
      out[1] = parts[1] + 0; out[2] = parts[2] + 0; out[3] = parts[3] + 0
      out[4] = pre
    }
    function cmp(x, y,    k, nx, ny, px, py, i) {
      for (k = 1; k <= 3; k++) if (x[k] != y[k]) return x[k] < y[k] ? -1 : 1
      if (x[4] == y[4]) return 0
      if (x[4] == "") return 1
      if (y[4] == "") return -1
      nx = split(x[4], px, "."); ny = split(y[4], py, ".")
      for (i = 1; i <= nx && i <= ny; i++) {
        if (px[i] == py[i]) continue
        if (px[i] ~ /^[0-9]+$/ && py[i] ~ /^[0-9]+$/) return (px[i] + 0 < py[i] + 0) ? -1 : 1
        return px[i] < py[i] ? -1 : 1
      }
      return nx < ny ? -1 : (nx > ny ? 1 : 0)
    }
    BEGIN { parse(a, x); parse(b, y); exit (cmp(x, y) > 0) ? 0 : 1 }'
}

app_running() { pgrep -x wordink >/dev/null 2>&1; }

quit_app() {
  app_running || return 0
  [ ! -x "$app/Contents/MacOS/wordink" ] || "$app/Contents/MacOS/wordink" quit >/dev/null 2>&1 || true
  waited=0
  while app_running && [ "$waited" -lt 5 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if app_running; then
    osascript -e "tell application id \"$bundle_id\" to quit" >/dev/null 2>&1 || true
    sleep 2
  fi
  if app_running; then
    pkill -x wordink >/dev/null 2>&1 || true
    sleep 1
  fi
  ! app_running || fail "WordInk is still running and would not quit; quit it from the menu bar and run this again."
}

# Starts the installed app and runs `wordink launch-at-login <on|off>` once it
# answers. Prints the app's reply on stdout; empty when it never answered.
login_item() {
  [ -x "$app/Contents/MacOS/wordink" ] || return 0
  open "$app" >/dev/null 2>&1 || return 0
  tries=0
  while [ "$tries" -lt 20 ]; do
    if reply=$("$app/Contents/MacOS/wordink" launch-at-login "$1" 2>&1); then
      printf '%s\n' "$reply"
      return 0
    fi
    tries=$((tries + 1))
    sleep 1
  done
}

do_install() {
  check_platform
  check_base
  work=$(mktemp -d "${TMPDIR:-/tmp}/wordink-install.XXXXXX") || fail "could not create a temporary folder; free some disk space."
  fetch SHA256SUMS "$work/SHA256SUMS"
  entry=$(awk '{ n = $2; sub(/^\*/, "", n); if (n ~ /^wordink-.*-macos-arm64\.zip$/) { print $1 " " n; exit } }' "$work/SHA256SUMS")
  [ -n "$entry" ] || fail "SHA256SUMS lists no macOS arm64 zip; see $repo_url."
  want=${entry% *}
  zip_name=${entry#* }
  version=${zip_name#wordink-}
  version=${version%-macos-arm64.zip}
  case "$version" in
    '' | *[!0-9A-Za-z.+-]*) fail "SHA256SUMS names an invalid version '$version'; see $repo_url." ;;
  esac
  case "$want" in
    *[!0-9a-f]*) fail "SHA256SUMS holds a malformed hash for $zip_name; see $repo_url." ;;
  esac
  [ "${#want}" -eq 64 ] || fail "SHA256SUMS holds a malformed hash for $zip_name; see $repo_url."

  if [ -d "$app" ] && [ "$force" -eq 0 ]; then
    installed=$(plist_version "$app")
    if [ -n "$installed" ] && ! newer "$version" "$installed"; then
      printf 'WordInk %s is already installed in ~/Applications (release is %s); use --force to reinstall.\n' "$installed" "$version"
      return 0
    fi
  fi

  [ -t 2 ] || printf 'Downloading WordInk %s...\n' "$version"
  fetch "$zip_name" "$work/$zip_name" progress
  got=$(sha256_of "$work/$zip_name")
  [ "$got" = "$want" ] || fail "$zip_name does not match SHA256SUMS, nothing was installed; try again, and report it at $repo_url if it persists."

  mkdir -p "$apps_dir" || fail "could not create ~/Applications; check its permissions."
  stage=$(mktemp -d "$apps_dir/.wordink-stage.XXXXXX") || fail "could not write to ~/Applications; check its permissions."
  if command -v ditto >/dev/null 2>&1; then
    ditto -x -k "$work/$zip_name" "$stage" || fail "could not unpack $zip_name; try again."
  else
    unzip -q "$work/$zip_name" -d "$stage" || fail "could not unpack $zip_name; try again."
  fi
  [ -d "$stage/WordInk.app" ] || fail "$zip_name does not contain WordInk.app; report it at $repo_url."
  chmod 755 "$stage/WordInk.app/Contents/MacOS/wordink" 2>/dev/null || true
  [ -x "$stage/WordInk.app/Contents/MacOS/wordink" ] || fail "$zip_name has no wordink executable; report it at $repo_url."
  packed=$(plist_version "$stage/WordInk.app")
  [ "$packed" = "$version" ] || fail "$zip_name holds WordInk '$packed', not $version; report it at $repo_url."
  # Downloads made by curl carry no quarantine flag; clear it anyway.
  xattr -dr com.apple.quarantine "$stage/WordInk.app" >/dev/null 2>&1 || true

  quit_app
  if [ -d "$app" ]; then
    backup="$apps_dir/.WordInk.previous.$$"
    mv "$app" "$backup" || fail "could not replace the installed WordInk; quit it and try again."
  fi
  if ! mv "$stage/WordInk.app" "$app"; then
    fail "could not move WordInk into ~/Applications; your previous version was kept."
  fi
  [ -z "$backup" ] || { rm -rf "$backup"; backup=; }

  printf 'Installed WordInk %s in ~/Applications.\n' "$version"
  printf 'Note: WordInk is unsigned (not notarized); macOS may ask you to approve microphone and accessibility access.\n'
  reply=$(login_item on)
  case "$reply" in
    *approval*)
      printf 'WordInk is open in the menu bar.\n'
      printf 'Approve launch at login in System Settings > General > Login Items & Extensions.\n'
      ;;
    *"Launch at login is on"* | *"already on"*)
      printf 'WordInk is open in the menu bar and starts at login.\n'
      ;;
    *)
      printf 'WordInk is open in the menu bar; launch at login could not be turned on, enable it in WordInk settings.\n'
      ;;
  esac
}

confirm_purge() {
  [ "$assume_yes" -eq 0 ] || return 0
  tty=${WORDINK_TTY:-/dev/tty}
  { : <"$tty"; } 2>/dev/null || fail "--purge needs a confirmation; run it in a terminal or add --yes."
  printf 'Delete WordInk settings, history and models in ~/Library/Application Support/wordink? [y/N] ' >&2
  answer=
  read -r answer <"$tty" || answer=
  case "$answer" in
    y | Y | yes | YES) ;;
    *) printf 'Nothing was removed.\n'; exit 0 ;;
  esac
}

do_uninstall() {
  [ "$(uname -s)" = Darwin ] || unsupported
  [ "$purge" -eq 0 ] || confirm_purge
  if [ -d "$app" ]; then
    # The login item can only be removed by the app itself.
    login_item off >/dev/null
    quit_app
    rm -rf "$app" || fail "could not remove ~/Applications/WordInk.app; quit WordInk and try again."
    printf 'Removed ~/Applications/WordInk.app and its login item.\n'
  else
    printf 'WordInk is not installed in ~/Applications.\n'
  fi
  if [ "$purge" -eq 1 ]; then
    rm -rf "$support_dir" || fail "could not remove ~/Library/Application Support/wordink; delete it by hand."
    printf 'Removed ~/Library/Application Support/wordink.\n'
  fi
}

case "$mode" in
  install) do_install ;;
  uninstall) do_uninstall ;;
esac
