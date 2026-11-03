#!/bin/sh
# Runs packaging/macos/install-macos.sh against a loopback fixture release
# server, with fake macOS commands (uname, sw_vers, open, defaults, ditto, ...)
# first on PATH, so it runs on Linux. Hermetic: HOME is a temp folder, nothing
# is downloaded from the internet and nothing outside the temp folder changes.
# shellcheck disable=SC2016  # the fake commands are scripts, quoted on purpose
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
installer="$root/packaging/macos/install-macos.sh"
for command in curl python3; do
  command -v "$command" >/dev/null 2>&1 || { echo "Installer test requires $command." >&2; exit 1; }
done
if command -v sha256sum >/dev/null 2>&1; then
  real_sha='sha256sum "$@"'
else
  real_sha='/usr/bin/shasum -a 256 "$@"'
fi

temporary=$(mktemp -d)
server_pid=
cleanup() {
  [ -z "$server_pid" ] || { kill "$server_pid" >/dev/null 2>&1 || true; wait "$server_pid" 2>/dev/null || true; }
  rm -rf "$temporary"
}
trap cleanup EXIT HUP INT TERM

bin="$temporary/bin"
server="$temporary/server"
home="$temporary/home"
log="$temporary/calls.log"
mkdir -p "$bin" "$server" "$home"
: >"$log"

fake() { # fake <name> <script body>
  printf '#!/bin/sh\n%s\n' "$2" >"$bin/$1"
  chmod 755 "$bin/$1"
}
fake uname 'case "$1" in -s) echo "${FAKE_UNAME_S:-Darwin}" ;; -m) echo "${FAKE_UNAME_M:-arm64}" ;; *) echo Darwin ;; esac'
fake sw_vers 'echo "${FAKE_OSVER:-15.1}"'
fake sysctl 'echo 0'
fake open 'echo "open $*" >>"$FAKE_LOG"'
fake osascript 'echo "osascript $*" >>"$FAKE_LOG"'
fake xattr 'echo "xattr $*" >>"$FAKE_LOG"'
fake pgrep 'exit 1'
fake pkill 'exit 1'
fake sleep 'exit 0'
fake sudo 'echo "sudo $*" >>"$FAKE_LOG"; exit 1'
fake shasum "shift 2; exec $real_sha"
fake id 'echo "${FAKE_UID:-1000}"'
fake defaults 'python3 -I -c "import plistlib, sys; print(plistlib.load(open(sys.argv[1] + \".plist\", \"rb\"))[sys.argv[2]])" "$2" "$3"'
fake ditto 'python3 -I -c "import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$3" "$4"'
PATH="$bin:$PATH"
export PATH FAKE_LOG="$log"

port=$(python3 -I -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
python3 -I -m http.server "$port" --bind 127.0.0.1 --directory "$server" >/dev/null 2>&1 &
server_pid=$!
tries=0
until curl -fs "http://127.0.0.1:$port/" >/dev/null 2>&1; do
  tries=$((tries + 1))
  [ "$tries" -lt 50 ] || { echo "Fixture server did not start." >&2; exit 1; }
  python3 -I -c 'import time; time.sleep(0.1)'
done

# release <dir> <version> [bad-hash]: a fixture release with SHA256SUMS.
release() {
  python3 -I - "$server/$1" "$2" "${3:-}" <<'PY'
import hashlib, os, sys, zipfile
out, version, bad = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(out, exist_ok=True)
name = f"wordink-{version}-macos-arm64.zip"
plist = f"""<?xml version="1.0"?>
<plist version="1.0"><dict>
<key>CFBundleShortVersionString</key>
<string>{version}</string>
</dict></plist>
"""
script = f"""#!/bin/sh
case "$1" in
  --version) echo "wordink {version}" ;;
  quit) echo "quit" >>"$FAKE_LOG" ;;
  launch-at-login)
    echo "launch-at-login $2" >>"$FAKE_LOG"
    if [ "$2" = on ] && [ "${{FAKE_APPROVAL:-}}" = 1 ]; then
      echo "Launch at login is waiting for approval in System Settings"
    else
      echo "Launch at login is $2"
    fi ;;
esac
"""
with zipfile.ZipFile(os.path.join(out, name), "w") as z:
    z.writestr("WordInk.app/Contents/Info.plist", plist)
    z.writestr("WordInk.app/Contents/MacOS/wordink", script)
digest = hashlib.sha256(open(os.path.join(out, name), "rb").read()).hexdigest()
if bad:
    digest = "0" * 64
open(os.path.join(out, "SHA256SUMS"), "w").write(f"{digest}  {name}\n")
PY
}

base="http://127.0.0.1:$port"
app="$home/Applications/WordInk.app"
support="$home/Library/Application Support/wordink"
out="$temporary/out"
err="$temporary/err"
failures=0

# run <args...>: runs the installer in the temp HOME; records the exit status, $out and $err.
run() {
  # Callers set per-run variables in a subshell, `(VAR=x run ...)`: a
  # prefix assignment on a function call persists in some shells (macOS sh).
  code=0
  HOME="$home" sh "$installer" "$@" >"$out" 2>"$err" || code=$?
  echo "$code" >"$temporary/status"
}
check() { # check <description> <command...>
  description=$1
  shift
  if "$@"; then
    echo "ok   $description"
  else
    echo "FAIL $description" >&2
    echo "--- stdout" >&2; cat "$out" >&2
    echo "--- stderr" >&2; cat "$err" >&2
    failures=$((failures + 1))
  fi
}
status_is() { [ "$(cat "$temporary/status")" -eq "$1" ]; }
contains() { grep -q -F -- "$1" "$out" "$err"; }
logged() { grep -q -F -- "$1" "$log"; }
not_logged() { ! grep -q -F -- "$1" "$log"; }
lines_at_most() { [ "$(cat "$out" "$err" | wc -l)" -le "$1" ]; }
installed_is() { [ "$("$app/Contents/MacOS/wordink" --version)" = "wordink $1" ]; }
no_app() { [ ! -e "$app" ]; }
marker_kept() { [ -e "$app/marker" ]; }
marker_gone() { [ ! -e "$app/marker" ]; }

release v100 1.0.0
release v101 1.0.1
release v090 0.9.0
release bad 1.0.2 bad

# Refusals.
export WORDINK_INSTALL_BASE="$base/v100"
(FAKE_UNAME_M=x86_64 run)
check "Intel Mac is refused with the supported-platform line" status_is 1
check "  names the releases page" contains "https://github.com/duketopceo/WordInk/releases"
check "  prints one line" lines_at_most 1
check "  installs nothing" no_app
(FAKE_OSVER=14.6 run)
check "macOS 14 is refused" status_is 1
check "  names Apple silicon and macOS 15" contains "macOS 15 or newer"
(FAKE_UNAME_S=Linux run)
check "a non-Mac is refused" status_is 1
(WORDINK_INSTALL_BASE=http://example.com/releases run)
check "a non-loopback http base is refused" status_is 1
check "  says why in one line" lines_at_most 1
(WORDINK_INSTALL_BASE=http://127.0.0.1@example.com/releases run)
check "a base with userinfo is refused" status_is 1
(WORDINK_INSTALL_BASE=http://127.0.0.1.example.com/releases run)
check "a lookalike loopback host is refused" status_is 1
check "  installs nothing" no_app

# Base host parsing: loopback with a port is accepted, malformed authorities are not.
for bad in http://localhost: http://127.0.0.1: 'http://127.0.0.1:80:80' 'http://[::1]:8080' http://127.0.0.1:8x; do
  (WORDINK_INSTALL_BASE="$bad/releases" run)
  check "base $bad is refused" status_is 1
  check "  installs nothing" no_app
done
(WORDINK_INSTALL_BASE="http://localhost:$port/v100" run)
check "http://localhost:<port> is accepted" status_is 0
rm -rf "$app"
(FAKE_UID=0 run)
check "running as root is refused in one line" status_is 1
check "  says not root" contains "not root"
check "  installs nothing" no_app

# Fresh install.
run
check "fresh install succeeds" status_is 0
check "  writes WordInk.app" installed_is 1.0.0
check "  prints at most five lines" lines_at_most 5
check "  says the app is unsigned" contains "unsigned"
check "  turns on launch at login" logged "launch-at-login on"
check "  opens the app" logged "open $app"
check "  clears quarantine" logged "com.apple.quarantine"
check "  never calls sudo" not_logged "sudo"
check "  leaves no staging folders" [ -z "$(find "$home/Applications" -maxdepth 1 -name '.*' ! -name . | head -1)" ]

# Same version again changes nothing.
touch "$app/marker"
: >"$log"
run
check "same version is a no-op" status_is 0
check "  says it is already installed" contains "already installed"
check "  leaves the app untouched" marker_kept
check "  does not open the app" not_logged "open "

# --force reinstalls.
run --force
check "--force reinstalls" status_is 0
check "  replaces the app" marker_gone
check "  keeps the right version" installed_is 1.0.0

# Newer replaces, older does not.
(WORDINK_INSTALL_BASE="$base/v101" run)
check "a newer release replaces the app" status_is 0
check "  now at 1.0.1" installed_is 1.0.1
touch "$app/marker"
(WORDINK_INSTALL_BASE="$base/v090" run)
check "an older release is a no-op" status_is 0
check "  keeps 1.0.1" installed_is 1.0.1
check "  leaves the app untouched" marker_kept

# Hash mismatch fails without touching the installed app.
(WORDINK_INSTALL_BASE="$base/bad" run)
check "a hash mismatch fails" status_is 1
check "  names SHA256SUMS in one line" contains "does not match SHA256SUMS"
check "  keeps the installed app" marker_kept
check "  keeps its version" installed_is 1.0.1
check "  leaves no staging folders" [ -z "$(find "$home/Applications" -maxdepth 1 -name '.*' ! -name . | head -1)" ]
(WORDINK_INSTALL_BASE="$base/missing" run)
check "an unreachable release fails in one line" status_is 1
check "  keeps the installed app" marker_kept

# Launch at login held for approval: last line says to approve it.
rm -rf "$app"
(FAKE_APPROVAL=1 run)
check "an approval-held login item still installs" status_is 0
check "  last line says to approve it in System Settings" sh -c 'tail -n 1 "$1" | grep -q "System Settings"' _ "$out"
check "  prints at most five lines" lines_at_most 5

# Uninstall and purge.
mkdir -p "$support/models"
echo data >"$support/settings.json"
: >"$log"
run uninstall
check "uninstall succeeds" status_is 0
check "  removes the app" no_app
check "  turns off the login item" logged "launch-at-login off"
check "  keeps settings and models" [ -e "$support/settings.json" ]

run install
printf 'n\n' >"$temporary/answer"
(WORDINK_TTY="$temporary/answer" run uninstall --purge)
check "--purge declined removes nothing" status_is 0
check "  keeps the app" [ -d "$app" ]
check "  keeps settings" [ -e "$support/settings.json" ]
printf 'y\n' >"$temporary/answer"
(WORDINK_TTY="$temporary/answer" run uninstall --purge)
check "--purge confirmed succeeds" status_is 0
check "  removes the app" no_app
check "  removes the support folder" [ ! -e "$support" ]
mkdir -p "$app/Contents/MacOS"
: >"$log"
run uninstall
check "uninstall of a bundle with no executable succeeds quietly" status_is 0
check "  does not try to open it" not_logged "open "
check "  removes it" no_app
run uninstall
check "uninstall with nothing installed succeeds" status_is 0
check "  says so" contains "not installed"
(WORDINK_TTY="$temporary/no-such-tty" run --purge)
check "--purge with no terminal asks for --yes" status_is 1

if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed." >&2
  exit 1
fi
echo "All installer checks passed."
