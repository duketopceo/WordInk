#!/bin/sh
# Installs a WordInk release for the current user, from the unpacked release
# tarball it ships in (wordink-<version>-linux-<x64|arm64>.tar.gz). Users
# normally reach it through the bootstrap (install.sh), which verifies the
# download first:
#
#   curl -fsSL https://github.com/duketopceo/WordInk/releases/latest/download/install.sh | sh
#
# Run by hand from the unpacked directory:
#
#   ./install-release.sh [install] [--force]
#   ./install-release.sh uninstall [--purge] [--yes]
#
# It lays out what the in-app updater (src/updater/linux.rs) updates itself in:
#   ~/.local/share/wordink/versions/<version>/wordink
#   ~/.local/share/wordink/current            -> versions/<version>
#   ~/.local/share/wordink/install.json       (the receipt that turns updates on)
#   ~/.local/bin/wordink                      -> ~/.local/share/wordink/current/wordink
#   ~/.local/share/applications/io.github.duketopceo.wordink.desktop
#   ~/.local/share/icons/hicolor/*/apps/io.github.duketopceo.wordink.png
#   ~/.config/systemd/user/wordink.service    (enabled, restarted)
#
# Verification. The executable is checked before anything is copied, one of
# two ways:
#   * WORDINK_BOOTSTRAP_VERIFIED=<sha256>: the bootstrap already verified the
#     whole tarball against the signed SHA256SUMS and says which executable
#     hash that was. This script re-hashes the executable and requires an
#     exact match, so it never trusts the claim without checking the bytes.
#   * Otherwise the release's raw 64-byte wordink.sig beside the executable,
#     over "wordink.linux-artifact.v1\n<sha256 of the executable>\n", against
#     the key below (packaging/linux/wordink-signing.pub).
#
# A local source build (scripts/install-linux.sh) is migrated: its `current`
# directory (only the binary and a `source-build` marker, never user data) is
# moved to current.source-bak, the release is installed, and the backup is
# deleted only once `systemctl --user is-active wordink` says active. If the
# new build does not start, the source build and its service are restored.
set -eu

signing_key=MCowBQYDK2VwAyEAupIraDuQFZun9b4ZGlL5P6j9klVOOFmXTzaPNuxkKAc=
app_id=io.github.duketopceo.wordink

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
temporary=
migrating=0
committed=0
backup_unit=
created_receipt=0
created_version_dir=
support=
service_was_active=0

fail() {
  echo "$1" >&2
  exit 1
}

restore_migration() {
  # Put the source build back exactly as it was: directory, unit, entry, service.
  rm -f "${support:?}/current" "${support:?}/.current.next"
  if [ "$created_receipt" = 1 ]; then rm -f "${support:?}/install.json"; fi
  if [ -n "$created_version_dir" ]; then rm -rf "${created_version_dir:?}"; fi
  if [ -d "$support/current.source-bak" ] && [ ! -e "$support/current" ]; then
    mv "$support/current.source-bak" "$support/current"
  fi
  if [ -n "$backup_unit" ] && [ -f "$backup_unit/wordink.service" ]; then
    cp -p "$backup_unit/wordink.service" "$units/wordink.service"
  fi
  if [ -n "$backup_unit" ] && [ -f "$backup_unit/$app_id.desktop" ]; then
    cp -p "$backup_unit/$app_id.desktop" "$applications/$app_id.desktop"
  fi
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload >/dev/null 2>&1 || true
    if [ "$service_was_active" = 1 ]; then
      systemctl --user restart wordink.service >/dev/null 2>&1 || true
    fi
  fi
}

cleanup() {
  status=$?
  if [ "$migrating" = 1 ] && [ "$committed" = 0 ] && [ "$status" -ne 0 ]; then
    restore_migration
  fi
  if [ -n "$temporary" ]; then rm -rf "${temporary:?}"; fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

# --- arguments ---------------------------------------------------------
action=install
force=0
purge=0
yes=0
for argument in "$@"; do
  case "$argument" in
    install) action=install ;;
    uninstall) action=uninstall ;;
    --force) force=1 ;;
    --purge) purge=1 ;;
    --yes | -y) yes=1 ;;
    *) fail "Usage: install-release.sh [install [--force] | uninstall [--purge] [--yes]]" ;;
  esac
done
if [ "$purge" = 1 ] && [ "$action" != uninstall ]; then fail "--purge only goes with uninstall."; fi
if [ "$force" = 1 ] && [ "$action" != install ]; then fail "--force only goes with install."; fi

# --- environment -------------------------------------------------------
if [ "$(id -u)" = 0 ]; then
  fail "Run this as your desktop user, not root."
fi
case "$(uname -s) $(uname -m)" in
  "Linux x86_64" | "Linux aarch64") ;;
  *) fail "WordInk supports x86_64 and aarch64 Linux. See https://github.com/duketopceo/WordInk/releases" ;;
esac
case "${HOME:-}" in
  /?*) ;;
  *) fail "HOME must be an absolute path." ;;
esac

# Every path below lands in sed replacements and unit files, so each must be
# absolute and free of whitespace, quotes and the characters sed treats as
# special.
checked_path() {
  case "$2" in
    /?*) ;;
    *) fail "$1 must be an absolute path (got: $2)." ;;
  esac
  case "$2" in
    *[[:space:]\&\\#%\"\'\`\$\;\|\<\>*?]* | */../* | */.. | */./* | */.)
      fail "$1 ($2) can't contain spaces, quotes, wildcards, & \\ # % or dot segments; pick another directory."
      ;;
  esac
}
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
bin_dir=${WORDINK_BIN_DIR:-"$HOME/.local/bin"}
support=${HEX_APPLICATION_SUPPORT_DIR:-"$data_home/wordink"}
checked_path XDG_DATA_HOME "$data_home"
checked_path XDG_CONFIG_HOME "$config_home"
checked_path WORDINK_BIN_DIR "$bin_dir"
checked_path "The data directory" "$support"
applications="$data_home/applications"
icons="$data_home/icons/hicolor"
units="$config_home/systemd/user"
# The binary the packages install (the .deb, .rpm and AUR package); only the
# location is configurable, for tests.
system_binary=${WORDINK_SYSTEM_BINARY:-/usr/bin/wordink}

have_bus() {
  command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1
}
service_active() {
  command -v systemctl >/dev/null 2>&1 && [ "$(systemctl --user is-active wordink.service 2>/dev/null)" = active ]
}

# --- uninstall ---------------------------------------------------------
confirm_purge() {
  [ "$yes" = 1 ] && return 0
  # Under `curl | sh` stdin is the script, so ask on the terminal itself.
  if ( : </dev/tty ) 2>/dev/null; then
    printf 'This permanently deletes WordInk settings, history, models and saved keys (%s). Type yes to continue: ' "$support" >/dev/tty
    answer=
    read -r answer </dev/tty || answer=
    [ "$answer" = yes ] && return 0
    fail "Cancelled; nothing was removed."
  fi
  fail "--purge deletes your settings, history and models; there is no terminal to confirm on, so add --yes to confirm."
}

purge_data() {
  # A support directory that is a symlink, $HOME, or the filesystem root is
  # never something to delete.
  [ ! -L "$support" ] || fail "Refusing to delete $support: it is a symlink."
  resolved=$(CDPATH='' cd -- "$support" 2>/dev/null && pwd -P) || resolved=
  if [ -n "$resolved" ]; then
    home_resolved=$(CDPATH='' cd -- "$HOME" && pwd -P)
    case "$resolved" in
      / | "$home_resolved") fail "Refusing to delete $resolved." ;;
    esac
    [ "$(printf '%s' "$resolved" | awk -F/ '{ print NF }')" -ge 4 ] || fail "Refusing to delete $resolved."
    rm -rf -- "${resolved:?}"
  fi
  rm -rf -- "${HOME:?}/.config/wordink"
  # `wordink keys` can only set and show keys, not delete them; the
  # secret-service entries carry service=wordink (src/keys.rs), which
  # secret-tool can clear.
  if command -v secret-tool >/dev/null 2>&1; then
    secret-tool clear service wordink >/dev/null 2>&1 || true
  fi
}

uninstall() {
  if [ "$purge" = 1 ]; then confirm_purge; fi
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user disable --now wordink.service >/dev/null 2>&1 || true
  fi
  rm -f "${units:?}/wordink.service" "${applications:?}/$app_id.desktop" "${config_home:?}/autostart/$app_id.desktop"
  if [ -L "$bin_dir/wordink" ]; then rm -f "${bin_dir:?}/wordink"; fi
  if [ -L "$support/current" ]; then
    rm -f "${support:?}/current" "${support:?}/install.json"
    rm -rf "${support:?}/versions" "${support:?}/updates"
  fi
  find "$icons" -name "$app_id.png" -delete 2>/dev/null || true
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload >/dev/null 2>&1 || true
  fi
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications" >/dev/null 2>&1 || true
  fi
  if [ "$purge" = 1 ]; then
    purge_data
    echo "WordInk uninstalled, and its settings, history, models and saved keys were deleted."
  else
    echo "WordInk uninstalled. Settings and history stay in $support (add --purge to delete them)."
  fi
}

if [ "$action" = uninstall ]; then
  uninstall
  exit 0
fi

# --- already installed by a package? ----------------------------------
if [ -e "$system_binary" ] || [ -L "$system_binary" ]; then
  target=$(readlink "$system_binary" 2>/dev/null || true)
  case "$target" in
    "$HOME"/.local/* | "$support"/*) ;;
    *)
      owner=
      if command -v pacman >/dev/null 2>&1; then
        owner=$(pacman -Qo "$system_binary" 2>/dev/null | awk '{ print $(NF-1) " " $NF }') || owner=
      fi
      if [ -z "$owner" ] && command -v dpkg >/dev/null 2>&1; then
        owner=$(dpkg -S "$system_binary" 2>/dev/null | awk -F: 'NR == 1 { print $1 }') || owner=
      fi
      if [ -z "$owner" ] && command -v rpm >/dev/null 2>&1; then
        owner=$(rpm -qf "$system_binary" 2>/dev/null | head -n 1) || owner=
      fi
      if [ -n "$owner" ]; then
        fail "WordInk is already installed by the package $owner; update or remove it with your package manager first."
      fi
      fail "$system_binary already exists and was not installed by this script; remove it or use your package manager."
      ;;
  esac
fi

# --- the executable ----------------------------------------------------
binary="$here/wordink"
[ -f "$binary" ] || fail "wordink is missing beside this installer: $binary"
command -v sha256sum >/dev/null 2>&1 || fail "Verifying the release needs sha256sum."
sha256=$(sha256sum "$binary" | awk '{ print $1 }')
temporary=$(mktemp -d)
if [ -n "${WORDINK_BOOTSTRAP_VERIFIED:-}" ]; then
  [ "$WORDINK_BOOTSTRAP_VERIFIED" = "$sha256" ] || fail "The executable does not match what the bootstrap verified; refusing to install it."
elif [ "${WORDINK_INSTALL_UNVERIFIED:-}" != 1 ]; then
  command -v openssl >/dev/null 2>&1 || fail "Verifying the release needs openssl. Install it, or set WORDINK_INSTALL_UNVERIFIED=1 to skip the check."
  [ -f "$here/wordink.sig" ] || fail "wordink.sig is missing beside the executable; refusing to install it."
  [ "$(wc -c < "$here/wordink.sig" | tr -d ' ')" = 64 ] || fail "wordink.sig is not 64 bytes; refusing to install."
  printf -- '-----BEGIN PUBLIC KEY-----\n%s\n-----END PUBLIC KEY-----\n' "$signing_key" > "$temporary/signing.pem"
  printf 'wordink.linux-artifact.v1\n%s\n' "$sha256" > "$temporary/signed"
  openssl pkeyutl -verify -rawin -pubin -inkey "$temporary/signing.pem" \
    -in "$temporary/signed" -sigfile "$here/wordink.sig" >/dev/null 2>&1 \
    || fail "The release signature does not verify; refusing to install it."
fi

chmod 755 "$binary" 2>/dev/null || true
version=$("$binary" --version 2>/dev/null | awk 'NF { v=$NF } END { print v }') || true
[ -n "$version" ] || fail "The executable cannot start here. Check that its libraries are installed (ldd $binary)."
printf '%s\n' "$version" | grep -Eq '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$' || fail "The executable reports an invalid version."

if [ -e "$bin_dir/wordink" ] && [ ! -L "$bin_dir/wordink" ]; then
  fail "Refusing to replace $bin_dir/wordink: it is not a symlink."
fi

# --- the updater's lock ------------------------------------------------
umask 077
mkdir -p "$support/updates"
chmod 700 "$support" "$support/updates" 2>/dev/null || true
if command -v flock >/dev/null 2>&1; then
  exec 9> "$support/updates/update.lock"
  flock -n 9 || fail "WordInk is updating itself right now; try again in a minute."
fi

# --- what is installed now (fail closed on anything unclear) ----------
# `ls` tells "absent" from "cannot read": a permission error is not absence.
current="$support/current"
state=absent
if listing=$(ls -dn -- "$current" 2>&1); then
  if [ -L "$current" ]; then
    state=release
  elif [ -d "$current" ]; then
    state=directory
  else
    state=unknown
  fi
else
  case "$listing" in
    *"No such file"*) state=absent ;;
    *) state=unknown ;;
  esac
fi
installed=
case "$state" in
  unknown) fail "Cannot tell what is installed at $current (unreadable or not a WordInk install); not touching it." ;;
  release)
    link=$(readlink "$current")
    installed=${link#versions/}
    if [ "$installed" = "$link" ] || ! printf '%s\n' "$installed" | grep -Eq '^[0-9A-Za-z][0-9A-Za-z.+_-]{0,127}$'; then
      fail "$current points somewhere unexpected ($link); not touching it."
    fi
    ;;
  directory)
    [ -f "$current/source-build" ] || fail "$current is a directory WordInk did not create; not touching it."
    # Only the source install's own files may be in there; anything else
    # might be user data, so stop.
    entries=$(ls -A -- "$current" 2>/dev/null) || fail "Cannot read $current; not touching it."
    for entry in $entries; do
      case "$entry" in
        wordink | wordink.new | source-build) ;;
        *) fail "$current holds an unexpected file ($entry); not touching it." ;;
      esac
    done
    ;;
esac

# A backup left by an earlier run that could not confirm the service (no user
# bus) is only safe to drop once a release install is in place.
if [ -e "$support/current.source-bak" ]; then
  if [ "$state" = release ]; then
    rm -rf "${support:?}/current.source-bak"
  else
    fail "$support/current.source-bak exists from an earlier attempt; move it aside and run this again."
  fi
fi

# --- version guard (R10) ------------------------------------------------
# Succeeds when $1 >= $2 by semver (a release candidate sorts before its final).
version_ge() {
  awk -v a="$1" -v b="$2" '
    function split_version(v, out,   core, pre, i, k, parts) {
      pre = ""
      i = index(v, "-")
      core = (i ? substr(v, 1, i - 1) : v)
      if (i) pre = substr(v, i + 1)
      sub(/[+].*/, "", core)
      sub(/[+].*/, "", pre)
      out["n"] = split(core, parts, ".")
      for (k = 1; k <= out["n"]; k++) out[k] = parts[k] + 0
      out["pre"] = pre
    }
    function compare_pre(x, y,   xs, ys, nx, ny, k, m, xn, yn) {
      if (x == y) return 0
      if (x == "") return 1
      if (y == "") return -1
      nx = split(x, xs, "."); ny = split(y, ys, ".")
      m = (nx > ny ? nx : ny)
      for (k = 1; k <= m; k++) {
        if (k > nx) return -1
        if (k > ny) return 1
        xn = (xs[k] ~ /^[0-9]+$/); yn = (ys[k] ~ /^[0-9]+$/)
        if (xn && yn) { if (xs[k] + 0 != ys[k] + 0) return (xs[k] + 0 > ys[k] + 0) ? 1 : -1 }
        else if (xn) return -1
        else if (yn) return 1
        else if (xs[k] != ys[k]) return (xs[k] > ys[k]) ? 1 : -1
      }
      return 0
    }
    BEGIN {
      split_version(a, A); split_version(b, B)
      m = (A["n"] > B["n"] ? A["n"] : B["n"])
      for (k = 1; k <= m; k++) {
        x = (k <= A["n"] ? A[k] : 0); y = (k <= B["n"] ? B[k] : 0)
        if (x != y) exit ((x > y) ? 0 : 1)
      }
      exit ((compare_pre(A["pre"], B["pre"]) >= 0) ? 0 : 1)
    }'
}
if [ "$state" = release ] && [ "$force" = 0 ] && version_ge "$installed" "$version"; then
  echo "WordInk $installed is already installed."
  exit 0
fi

# --- install -----------------------------------------------------------
umask 022
mkdir -p "$support/versions" "$bin_dir" "$applications" "$units"
chmod 700 "$support" "$support/versions"
if [ ! -e "$support/versions/$version" ]; then created_version_dir="$support/versions/$version"; fi
if [ ! -e "$support/install.json" ]; then created_receipt=1; fi

if [ "$state" = directory ]; then
  migrating=1
  backup_unit="$temporary/backup"
  mkdir -p "$backup_unit"
  if [ -f "$units/wordink.service" ]; then cp -p "$units/wordink.service" "$backup_unit/"; fi
  if [ -f "$applications/$app_id.desktop" ]; then cp -p "$applications/$app_id.desktop" "$backup_unit/"; fi
  if service_active; then service_was_active=1; fi
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop wordink.service >/dev/null 2>&1 || true
  fi
  mv "$current" "$support/current.source-bak" || fail "Could not move the source build aside; nothing was changed."
fi

mkdir -p "$support/versions/$version"
chmod 700 "$support/versions/$version"
install -m 755 "$binary" "$support/versions/$version/wordink.new"
mv -f "$support/versions/$version/wordink.new" "$support/versions/$version/wordink"
rm -f "${support:?}/versions/$version/source-build"
ln -sfn "versions/$version" "$support/.current.next"
mv -Tf "$support/.current.next" "$current"
printf '{"app":"hex","artifact":"desktop","channel":"stable","method":"direct","root":"%s"}\n' "$support" > "$support/.install.json.next"
mv -f "$support/.install.json.next" "$support/install.json"
ln -sfn "$support/current/wordink" "$bin_dir/wordink"

sed "s#@HEX_BIN@#$bin_dir/wordink#g" "$here/$app_id.desktop" > "$applications/$app_id.desktop"
for png in "$here"/icons/hicolor/*/apps/"$app_id.png"; do
  [ -f "$png" ] || continue
  size=$(basename "$(dirname "$(dirname "$png")")")
  mkdir -p "$icons/$size/apps"
  install -m 644 "$png" "$icons/$size/apps/$app_id.png"
done
sed "s#^ExecStart=/usr/bin/wordink#ExecStart=$bin_dir/wordink#" "$here/wordink.service" > "$units/wordink.service"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$applications" >/dev/null 2>&1 || true
fi

# --- start it and check ------------------------------------------------
running=0
bus=0
if have_bus; then
  bus=1
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  systemctl --user enable wordink.service >/dev/null 2>&1 || true
  systemctl --user restart wordink.service >/dev/null 2>&1 || true
  attempt=0
  while [ "$attempt" -lt 5 ]; do
    if service_active; then running=1; break; fi
    attempt=$((attempt + 1))
    sleep 1
  done
fi

if [ "$migrating" = 1 ] && [ "$bus" = 1 ] && [ "$running" = 0 ]; then
  # Exits non-zero: the cleanup trap restores the source build and its service.
  fail "The release build did not start; your source build was restored."
fi

if [ "$migrating" = 1 ]; then
  printf 'WordInk %s installed, replacing your source build (settings and history kept).\n' "$version"
else
  printf 'WordInk %s installed.\n' "$version"
fi
echo "It checks the stable release for updates and verifies each one's signature."
if [ "$migrating" = 1 ]; then
  # The backup goes only once the service is confirmed active; with no user
  # bus it stays, and the next run removes it.
  if [ "$running" = 1 ]; then rm -rf "${support:?}/current.source-bak"; fi
  committed=1
fi
# The one hint when the bin directory is not on PATH.
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "Add $bin_dir to your PATH to run wordink from a terminal." ;;
esac
if [ "$running" = 1 ]; then
  echo "WordInk is running. Open the window with: wordink app"
else
  echo "The background service is not running here (no user systemd session); run: wordink app"
fi
