#!/bin/sh
# Tests the Linux one-line installer (packaging/linux/install.sh, the
# bootstrap) and the tarball installer it runs (install-release.sh) end to
# end, the way a user runs them: `sh -s -- <args> < install.sh`.
#
# Everything is a fixture. Releases are built with the real make-tarball.sh
# around a fake `wordink` script, served from python3's http.server on
# loopback, and signed with a throwaway ed25519 key made here. The bootstrap
# normally trusts only the key embedded in it; this harness swaps in the
# throwaway key through WORDINK_INSTALL_TEST_PUBLIC_KEY, which the bootstrap
# honours ONLY when WORDINK_INSTALL_BASE is a loopback address (one scenario
# proves the refusal). systemctl, pacman, getconf, secret-tool and friends are
# PATH stubs, and HOME is a temp directory, so this never touches the real
# user's systemd manager, keyring, data or settings.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
for command in curl openssl python3 sha256sum tar; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "Installer test requires $command." >&2
    exit 1
  fi
done
case "$(uname -s) $(uname -m)" in
  "Linux x86_64") arch=x64 ;;
  "Linux aarch64") arch=arm64 ;;
  *)
    echo "Installer test requires x86_64 or aarch64 Linux." >&2
    exit 1
    ;;
esac
if [ "$(id -u)" -eq 0 ]; then
  echo "Installer test must run as an unprivileged user." >&2
  exit 1
fi
real_openssl=$(command -v openssl)

temporary=$(mktemp -d)
server_pid=
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "${temporary:?}"
}
trap cleanup EXIT HUP INT TERM

# --- environment: nothing may leak in from, or out to, the real user --------
unset XDG_DATA_HOME XDG_CONFIG_HOME HEX_APPLICATION_SUPPORT_DIR WORDINK_BIN_DIR \
  WORDINK_INSTALL_BASE WORDINK_INSTALL_UNVERIFIED WORDINK_BOOTSTRAP_VERIFIED \
  WORDINK_INSTALL_TEST_PUBLIC_KEY WORDINK_SYSTEM_BINARY
home="$temporary/home"
support="$home/.local/share/wordink"
bin_dir="$home/.local/bin"
units="$home/.config/systemd/user"
sysd="$temporary/sysd"
stubs="$temporary/stubs"
server="$temporary/server"
bootstrap="$root/packaging/linux/install.sh"
mkdir -p "$stubs" "$server" "$sysd"
nowhere="$temporary/no-such-dir/wordink"

# --- stubs --------------------------------------------------------------
# systemctl: a user manager whose state lives in $SYSD. `restart` runs the
# ExecStart binary with `service --check`, so a release that "cannot start"
# really leaves the service inactive.
cat > "$stubs/systemctl" <<'EOF'
#!/bin/sh
[ "$1" = --user ] && shift
command=${1:-}
echo "$command $*" >> "$SYSD/log"
unit="$HOME/.config/systemd/user/wordink.service"
case "$command" in
  show-environment) [ ! -e "$SYSD/nobus" ] || { echo "Failed to connect to bus" >&2; exit 1; } ;;
  is-active)
    if [ -e "$SYSD/active" ] && [ ! -e "$SYSD/nobus" ]; then echo active; exit 0; fi
    echo inactive; exit 3 ;;
  stop) rm -f "$SYSD/active" ;;
  restart)
    [ ! -e "$SYSD/nobus" ] || exit 1
    bin=$(sed -n 's/^ExecStart=//p' "$unit" | awk '{ print $1 }')
    if "$bin" service --check; then
      touch "$SYSD/active"
      readlink -f "$bin" > "$SYSD/running"
    else
      rm -f "$SYSD/active"; exit 1
    fi ;;
  *) : ;;
esac
exit 0
EOF
cat > "$stubs/pacman" <<'EOF'
#!/bin/sh
[ "$1" = -Qo ] && { echo "$2 is owned by wordink-bin 1.0.0-1"; exit 0; }
exit 1
EOF
cat > "$stubs/secret-tool" <<'EOF'
#!/bin/sh
echo "$*" >> "$SYSD/secret-tool.log"
EOF
chmod 755 "$stubs"/*
export SYSD="$sysd"

# make_stub <name> <command> <script>: a directory that fakes one fact about
# the system when put first on PATH.
make_stub() {
  mkdir -p "$temporary/stub-$1"
  printf '%s\n' "$3" > "$temporary/stub-$1/$2"
  chmod 755 "$temporary/stub-$1/$2"
}
# shellcheck disable=SC2016 # the stub scripts are literal
make_stub glibc235 getconf '#!/bin/sh
[ "$1" = GNU_LIBC_VERSION ] && { echo "glibc 2.35"; exit 0; }
exit 1'
# shellcheck disable=SC2016
make_stub arm32 uname '#!/bin/sh
[ "$1" = -m ] && { echo armv7l; exit 0; }
exec /usr/bin/uname "$@"'
make_stub openssl1 openssl "#!/bin/sh
[ \"\$1\" = version ] && { echo 'OpenSSL 1.1.1w  11 Sep 2023'; exit 0; }
exec $real_openssl \"\$@\""

# --- fixture key and releases -------------------------------------------
openssl genpkey -algorithm ed25519 -out "$temporary/signing.pem" 2>/dev/null
fixture_key=$(openssl pkey -in "$temporary/signing.pem" -pubout -outform DER | base64 | tr -d '\n')

# mkbin <path> <version> [bad]: a fake executable. `bad` builds one that
# reports its version but cannot start the service.
mkbin() {
  code=0
  [ "${3:-}" != bad ] || code=1
  cat > "$1" <<EOF
#!/bin/sh
case "\${1:-}" in
  --version) echo "wordink $2" ;;
  service) [ "\${2:-}" = --check ] && exit $code ;;
esac
exit 0
EOF
  chmod 755 "$1"
}

sign_sums() {
  sha=$(sha256sum "$server/SHA256SUMS" | cut -c1-64)
  printf 'wordink.sha256sums.v1\n%s\n' "$sha" > "$temporary/message"
  openssl pkeyutl -sign -rawin -inkey "$temporary/signing.pem" -in "$temporary/message" -out "$server/SHA256SUMS.sig"
}

# publish <version> [bad]: builds the tarball with the real make-tarball.sh,
# and serves it with a signed SHA256SUMS.
publish() {
  dist="$temporary/dist-$1"
  rm -rf "${dist:?}" "${server:?}"/*
  mkdir -p "$dist"
  mkbin "$dist/wordink-$1-linux-$arch" "$1" "${2:-}"
  WORDINK_VERSION=$1 bash "$root/packaging/linux/make-tarball.sh" "$dist" "$arch" > /dev/null
  cp "$dist/wordink-$1-linux-$arch.tar.gz" "$server/"
  (cd "$server" && sha256sum "wordink-$1-linux-$arch.tar.gz" > SHA256SUMS)
  sign_sums
}

port=$((38000 + ($$ % 1000)))
publish 1.0.0
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$server" > "$temporary/server.log" 2>&1 &
server_pid=$!
attempt=0
until curl --fail --silent "http://127.0.0.1:$port/SHA256SUMS" > /dev/null; do
  attempt=$((attempt + 1))
  if [ "$attempt" -ge 30 ]; then
    echo "Fixture server did not start." >&2
    exit 1
  fi
  sleep 0.1
done
requests() { grep -c 'GET ' "$temporary/server.log" || true; }

# --- harness ---------------------------------------------------------
passed=0
failed=0
check() { # <description> <command...>
  description=$1
  shift
  if "$@"; then
    passed=$((passed + 1))
  else
    failed=$((failed + 1))
    echo "FAIL: $description" >&2
  fi
}
contains() { grep -Fq -- "$2" "$1"; }
not_contains() { ! grep -Fq -- "$2" "$1"; }
absent() { [ ! -e "$1" ] && [ ! -L "$1" ]; }
line_count_at_most() { [ "$(wc -l < "$1" | tr -d ' ')" -le "$2" ]; }
last_line_has() { tail -n 1 "$1" | grep -Fq -- "$2"; }

reset() {
  rm -rf "${home:?}" "${sysd:?}"
  mkdir -p "$home" "$sysd" "$bin_dir"
  touch "$sysd/active"
  : > "$sysd/log"
  export HOME="$home"
  export XDG_DATA_HOME="$home/.local/share"
  export XDG_CONFIG_HOME="$home/.config"
  extra_path=
  base="http://127.0.0.1:$port"
  system_binary=$nowhere
  test_key=$fixture_key
}

# run_bootstrap [args]: runs install.sh the way `curl | sh` does. Output goes
# to $temporary/out (stdout and stderr together, so the line counts are what a
# terminal would show); the exit status is in $status. test_key=none runs
# without the test-only key, so the embedded one is what is trusted.
run_bootstrap() {
  key_assignment=
  [ "$test_key" = none ] || key_assignment="WORDINK_INSTALL_TEST_PUBLIC_KEY=$test_key"
  status=0
  # shellcheck disable=SC2086 # key_assignment is empty or one NAME=value word
  env PATH="${extra_path:+$extra_path:}$stubs:$bin_dir:$PATH" \
    WORDINK_INSTALL_BASE="$base" $key_assignment \
    WORDINK_SYSTEM_BINARY="$system_binary" \
    sh -s -- "$@" < "$bootstrap" > "$temporary/out" 2>&1 || status=$?
}

# A local source build as scripts/install-linux.sh leaves it, with a running
# service and the user's data beside it.
make_source_build() {
  mkdir -p "$support/current" "$bin_dir" "$units" "$home/.local/share/applications"
  mkbin "$support/current/wordink" 3.2.0
  echo 3.2.0 > "$support/current/source-build"
  ln -s "$support/current/wordink" "$bin_dir/wordink"
  sed "s#^ExecStart=/usr/bin/wordink#ExecStart=$bin_dir/wordink#" "$root/packaging/wordink.service" > "$units/wordink.service"
  sed "s#@HEX_BIN@#$bin_dir/wordink#g" "$root/packaging/io.github.duketopceo.wordink.desktop" > "$home/.local/share/applications/io.github.duketopceo.wordink.desktop"
  printf '{"engine":"groq","vocabulary":["wordink"]}\n' > "$support/settings.json"
  printf 'history line 1\nhistory line 2\n' > "$support/history.jsonl"
  mkdir -p "$support/models"
  head -c 4096 /dev/zero > "$support/models/model.bin"
  touch "$sysd/active"
  readlink -f "$support/current/wordink" > "$sysd/running"
  cp "$support/settings.json" "$temporary/settings.before"
  cp "$units/wordink.service" "$temporary/unit.before"
  cp "$home/.local/share/applications/io.github.duketopceo.wordink.desktop" "$temporary/desktop.before"
}

same_data() { cmp -s "$support/settings.json" "$temporary/settings.before" && [ -f "$support/history.jsonl" ] && [ -f "$support/models/model.bin" ]; }
installed_version() { readlink "$support/current"; }
is_source_build() { [ -d "$support/current" ] && [ ! -L "$support/current" ] && [ -f "$support/current/source-build" ]; }
runs_binary() { [ "$(cat "$sysd/running")" = "$1" ]; }
status_is() { [ "$status" = "$1" ]; }
status_not() { [ "$status" != "$1" ]; }

# ===========================================================================
echo "scenario: fresh install"
reset
run_bootstrap
check "fresh install exits 0" status_is 0
check "current points at versions/1.0.0" [ "$(installed_version)" = versions/1.0.0 ]
check "the executable is installed" [ -x "$support/versions/1.0.0/wordink" ]
check "the PATH symlink points into current" [ "$(readlink "$bin_dir/wordink")" = "$support/current/wordink" ]
check "the receipt turns updates on" contains "$support/install.json" '"method":"direct"'
check "the unit runs the installed binary" contains "$units/wordink.service" "ExecStart=$bin_dir/wordink service"
check "the desktop entry names the binary" contains "$home/.local/share/applications/io.github.duketopceo.wordink.desktop" "$bin_dir/wordink"
check "the service was enabled" contains "$sysd/log" "enable wordink.service"
check "the service is active" [ -e "$sysd/active" ]
check "no source-build marker is left" absent "$support/versions/1.0.0/source-build"
check "output is at most five lines" line_count_at_most "$temporary/out" 5
check "output ends by saying how to open the app" last_line_has "$temporary/out" "wordink app"
check "a non-TTY run draws no progress bar" not_contains "$temporary/out" '###'

# ===========================================================================
echo "scenario: AE2 already installed, and --force"
# The updater installed 1.0.1; the installer for 1.0.0 must not downgrade.
mkdir -p "$support/versions/1.0.1"
mkbin "$support/versions/1.0.1/wordink" 1.0.1
ln -sfn versions/1.0.1 "$support/current"
: > "$sysd/log"
run_bootstrap
check "a newer install is left alone, exit 0" status_is 0
check "it says WordInk 1.0.1 is already installed" contains "$temporary/out" "WordInk 1.0.1 is already installed"
check "current still points at 1.0.1" [ "$(installed_version)" = versions/1.0.1 ]
check "the service was not touched" not_contains "$sysd/log" restart
run_bootstrap --force
check "--force exits 0" status_is 0
check "--force switches current to 1.0.0" [ "$(installed_version)" = versions/1.0.0 ]
run_bootstrap
check "the same version is also a no-op" contains "$temporary/out" "WordInk 1.0.0 is already installed"
publish 1.0.0-rc.1
run_bootstrap
check "a release candidate does not replace its final" contains "$temporary/out" "WordInk 1.0.0 is already installed"
check "and current is unchanged" [ "$(installed_version)" = versions/1.0.0 ]
publish 1.1.0
run_bootstrap
check "a newer release upgrades in place" [ "$(installed_version)" = versions/1.1.0 ]
check "the previous version stays for the updater" [ -x "$support/versions/1.0.0/wordink" ]
check "upgrade output is at most five lines" line_count_at_most "$temporary/out" 5
publish 1.1.0-rc.2
run_bootstrap
check "an older release candidate does not replace 1.1.0" contains "$temporary/out" "WordInk 1.1.0 is already installed"
run_bootstrap --force
check "a release candidate installs over a final with --force" [ "$(installed_version)" = versions/1.1.0-rc.2 ]
publish 1.1.0
run_bootstrap
check "the final replaces its release candidate" [ "$(installed_version)" = versions/1.1.0 ]

# ===========================================================================
echo "scenario: AE1 migrating a source build"
reset
publish 1.0.0
make_source_build
run_bootstrap
check "migration exits 0" status_is 0
check "current is now the release symlink" [ "$(installed_version)" = versions/1.0.0 ]
check "current.source-bak is removed" absent "$support/current.source-bak"
check "the service is active" [ -e "$sysd/active" ]
check "the service runs the release binary" runs_binary "$support/versions/1.0.0/wordink"
check "the service was stopped before the swap" contains "$sysd/log" "stop wordink.service"
check "settings are byte-identical" cmp "$support/settings.json" "$temporary/settings.before"
check "history and models are kept" same_data
check "the source-build marker is gone" absent "$support/versions/1.0.0/source-build"
check "output says the source build was replaced" contains "$temporary/out" "replacing your source build"
check "migration output is at most five lines" line_count_at_most "$temporary/out" 5

echo "scenario: migration where the new service fails to start"
reset
publish 1.0.1 bad
make_source_build
run_bootstrap
check "a failed start exits non-zero" status_not 0
check "the failure is one line (plus the download line)" line_count_at_most "$temporary/out" 2
check "the failure says the source build was restored" contains "$temporary/out" "source build was restored"
check "current is the source build directory again" is_source_build
check "no backup is left behind" absent "$support/current.source-bak"
check "the failed version is removed" absent "$support/versions/1.0.1"
check "no receipt was left (a source build never updates)" absent "$support/install.json"
check "the PATH symlink still resolves to the source binary" [ "$("$bin_dir/wordink" --version)" = "wordink 3.2.0" ]
check "the unit is restored byte for byte" cmp "$units/wordink.service" "$temporary/unit.before"
check "the desktop entry is restored byte for byte" cmp "$home/.local/share/applications/io.github.duketopceo.wordink.desktop" "$temporary/desktop.before"
check "the service is running the source build again" runs_binary "$support/current/wordink"
check "the service is active" [ -e "$sysd/active" ]
check "user data is untouched" same_data

echo "scenario: migration refuses unknown state"
reset
publish 1.0.0
make_source_build
echo secret > "$support/current/settings.json"
run_bootstrap
check "a current directory holding other files fails closed" status_not 0
check "it names the unexpected file" contains "$temporary/out" "settings.json"
check "the directory is untouched" [ -f "$support/current/settings.json" ]
check "and still has its marker" [ -f "$support/current/source-build" ]
rm "$support/current/settings.json" "$support/current/source-build"
run_bootstrap
check "a current directory without the source-build marker fails closed" status_not 0
check "and is untouched" [ -x "$support/current/wordink" ]
check "with no backup made" absent "$support/current.source-bak"
reset
mkdir -p "$support"
echo x > "$support/current"
run_bootstrap
check "a current that is a plain file fails closed" status_not 0

# ===========================================================================
echo "scenario: no user systemd bus"
reset
touch "$sysd/nobus"
run_bootstrap
check "install still succeeds" status_is 0
check "the install is in place" [ "$(installed_version)" = versions/1.0.0 ]
check "it prints the wordink app hint" last_line_has "$temporary/out" "run: wordink app"
check "the hint output is at most five lines" line_count_at_most "$temporary/out" 5
reset
make_source_build
touch "$sysd/nobus"
run_bootstrap
check "migration without a bus succeeds" status_is 0
check "migration without a bus switches current" [ "$(installed_version)" = versions/1.0.0 ]
check "the backup is kept while the service cannot be confirmed" [ -d "$support/current.source-bak" ]
check "settings are intact" cmp "$support/settings.json" "$temporary/settings.before"
rm -f "$sysd/nobus"
touch "$sysd/active"
run_bootstrap
check "the next run removes the leftover backup" absent "$support/current.source-bak"

# ===========================================================================
echo "scenario: AE3 preflight"
reset
publish 1.0.0
before=$(requests)
extra_path="$temporary/stub-glibc235"
run_bootstrap
extra_path=
check "glibc 2.35 fails" status_not 0
check "it is one line" line_count_at_most "$temporary/out" 1
check "it says glibc 2.39 or newer" contains "$temporary/out" "glibc 2.39 or newer"
check "it lists example distributions" contains "$temporary/out" "Ubuntu 24.04"
check "nothing was downloaded" [ "$(requests)" = "$before" ]
check "nothing was installed" absent "$support/current"
extra_path="$temporary/stub-arm32"
run_bootstrap
extra_path=
check "an unsupported CPU fails with the platforms line" contains "$temporary/out" "WordInk supports Linux (x86_64, aarch64)"
check "the line links the releases page" contains "$temporary/out" "https://github.com/duketopceo/WordInk/releases"
check "it is one line" line_count_at_most "$temporary/out" 1
check "nothing was downloaded for it either" [ "$(requests)" = "$before" ]
extra_path="$temporary/stub-openssl1"
run_bootstrap
extra_path=
check "OpenSSL 1.1 fails naming OpenSSL 3" contains "$temporary/out" "OpenSSL 3"
check "nothing was downloaded for it" [ "$(requests)" = "$before" ]

# ===========================================================================
echo "scenario: verification"
reset
publish 1.0.0
run_bootstrap
check "baseline install works" [ "$(installed_version)" = versions/1.0.0 ]
publish 1.2.0
printf 'x' | dd of="$server/SHA256SUMS.sig" bs=1 seek=3 conv=notrunc 2> /dev/null
run_bootstrap
check "a SHA256SUMS signature that does not verify fails" status_not 0
check "it says the signature failed" contains "$temporary/out" "signature failed"
check "the previous install is intact" [ "$(installed_version)" = versions/1.0.0 ]
check "the previous binary is intact" [ -x "$support/versions/1.0.0/wordink" ]
check "the new version was not installed" absent "$support/versions/1.2.0"
publish 1.2.0
# A list signed by some other key.
openssl genpkey -algorithm ed25519 -out "$temporary/other.pem" 2> /dev/null
openssl pkeyutl -sign -rawin -inkey "$temporary/other.pem" -in "$temporary/message" -out "$server/SHA256SUMS.sig"
run_bootstrap
check "a list signed by another key fails" contains "$temporary/out" "signature failed"
publish 1.2.0
head -c 10 "$server/SHA256SUMS.sig" > "$temporary/short.sig"
cp "$temporary/short.sig" "$server/SHA256SUMS.sig"
run_bootstrap
check "a truncated signature fails" contains "$temporary/out" "signature failed"
check "and installs nothing" [ "$(installed_version)" = versions/1.0.0 ]
# With no test key, the embedded key is what is trusted.
publish 1.2.0
test_key=none
run_bootstrap
test_key=$fixture_key
check "the embedded real key rejects a fixture-signed list" contains "$temporary/out" "signature failed"
embedded=$(sed -n '2p' "$root/packaging/linux/wordink-signing.pub")
check "the embedded key is the one in wordink-signing.pub" contains "$bootstrap" "signing_key=$embedded"

echo "scenario: tarball hash mismatch"
publish 1.2.0
# A tarball with install-release.sh swapped for a script that would run.
evil="$temporary/evil"
rm -rf "${evil:?}"
mkdir -p "$evil"
tar -xzf "$server/wordink-1.2.0-linux-$arch.tar.gz" -C "$evil"
printf '#!/bin/sh\ntouch "%s/evil-ran"\n' "$temporary" > "$evil/wordink-1.2.0-linux-$arch/install-release.sh"
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$evil" -czf "$server/wordink-1.2.0-linux-$arch.tar.gz" "wordink-1.2.0-linux-$arch"
run_bootstrap
check "a tarball that differs from SHA256SUMS fails" status_not 0
check "it says the checksum does not match" contains "$temporary/out" "does not match the signed checksum list"
check "nothing from the tarball ran" absent "$temporary/evil-ran"
check "the previous install is intact" [ "$(installed_version)" = versions/1.0.0 ]
check "the failure is one line (plus the download line)" line_count_at_most "$temporary/out" 2

echo "scenario: tarball with files outside its folder"
publish 1.2.0
rm -rf "${evil:?}"
mkdir -p "$evil/wordink-1.2.0-linux-$arch" "$evil/stray"
touch "$evil/stray/file"
tar --sort=name --owner=0 --group=0 --numeric-owner -C "$evil" -czf "$server/wordink-1.2.0-linux-$arch.tar.gz" stray "wordink-1.2.0-linux-$arch"
(cd "$server" && sha256sum "wordink-1.2.0-linux-$arch.tar.gz" > SHA256SUMS)
sign_sums
run_bootstrap
check "a signed but oddly shaped tarball is refused" contains "$temporary/out" "outside wordink-1.2.0-linux-$arch"

echo "scenario: base URL rules"
publish 1.2.0
before=$(requests)
base="http://example.com/releases"
run_bootstrap
check "a non-loopback http base is refused" status_not 0
check "with the http reason" contains "$temporary/out" "plain http"
base="http://127.0.0.1@example.com/releases"
run_bootstrap
check "a user name before the host is refused" contains "$temporary/out" "user name or password"
base="https://user:secret@example.com/releases"
run_bootstrap
check "https userinfo is refused" contains "$temporary/out" "user name or password"
base="ftp://127.0.0.1/releases"
run_bootstrap
check "other schemes are refused" contains "$temporary/out" "must start with https://"
base="https://example.invalid/releases"
run_bootstrap
check "the test key is refused for a non-loopback base" contains "$temporary/out" "only for tests against a loopback"
check "none of these downloaded anything" [ "$(requests)" = "$before" ]
base="http://127.0.0.1:$port"

echo "scenario: package-owned install"
reset
publish 1.0.0
system_binary="$temporary/usr-bin-wordink"
printf '#!/bin/sh\n' > "$system_binary"
run_bootstrap
check "a package-owned binary stops the install" status_not 0
check "the message names the package" contains "$temporary/out" "wordink-bin"
check "nothing was installed" absent "$support/current"
system_binary=$nowhere

echo "scenario: path overrides"
reset
export XDG_DATA_HOME="$home/share with space"
run_bootstrap
check "an XDG_DATA_HOME with a space is refused" contains "$temporary/out" "XDG_DATA_HOME"
check "it is one line (plus the download line)" line_count_at_most "$temporary/out" 2
export XDG_DATA_HOME="$home/.local/share"
export WORDINK_BIN_DIR="relative/bin"
run_bootstrap
check "a relative WORDINK_BIN_DIR is refused" contains "$temporary/out" "WORDINK_BIN_DIR must be an absolute path"
export WORDINK_BIN_DIR="$home/a&b"
run_bootstrap
check "WORDINK_BIN_DIR with an ampersand is refused" contains "$temporary/out" "WORDINK_BIN_DIR"
check "nothing was created" absent "$support/versions"
unset WORDINK_BIN_DIR

echo "scenario: PATH hint and line budget"
reset
make_source_build
# Run with a PATH that lacks ~/.local/bin.
status=0
env PATH="$stubs:/usr/bin:/bin" WORDINK_INSTALL_BASE="$base" WORDINK_INSTALL_TEST_PUBLIC_KEY="$fixture_key" \
  WORDINK_SYSTEM_BINARY="$nowhere" sh -s -- < "$bootstrap" > "$temporary/out" 2>&1 || status=$?
check "install succeeds without the bin dir on PATH" status_is 0
check "it hints once about PATH" [ "$(grep -c 'to your PATH' "$temporary/out")" = 1 ]
check "migration plus the hint is still at most five lines" line_count_at_most "$temporary/out" 5
check "and it ends by saying how to open the app" last_line_has "$temporary/out" "wordink app"

echo "scenario: the updater's lock"
reset
if command -v flock >/dev/null 2>&1; then
  mkdir -p "$support/updates"
  flock -n "$support/updates/update.lock" sleep 20 &
  holder=$!
  sleep 0.3
  run_bootstrap
  kill "$holder" 2> /dev/null || true
  wait "$holder" 2> /dev/null || true
  check "an install waits out a running update" status_not 0
  check "and says so" contains "$temporary/out" "updating itself"
  check "and changes nothing" absent "$support/current"
fi

# ===========================================================================
echo "scenario: uninstall and --purge"
reset
make_source_build
run_bootstrap
check "install before uninstall" [ "$(installed_version)" = versions/1.0.0 ]
mkdir -p "$home/.local/share/icons/hicolor/48x48/apps" "$home/.config/wordink" "$home/.config/autostart"
printf 'png' > "$home/.local/share/icons/hicolor/48x48/apps/io.github.duketopceo.wordink.png"
printf 'config' > "$home/.config/wordink/hex.config.ts"
touch "$home/.config/autostart/io.github.duketopceo.wordink.desktop"
run_bootstrap uninstall
check "uninstall exits 0" status_is 0
check "the symlink is removed" absent "$bin_dir/wordink"
check "the unit is removed" absent "$units/wordink.service"
check "the desktop entry is removed" absent "$home/.local/share/applications/io.github.duketopceo.wordink.desktop"
check "the autostart entry is removed" absent "$home/.config/autostart/io.github.duketopceo.wordink.desktop"
check "the icon is removed" absent "$home/.local/share/icons/hicolor/48x48/apps/io.github.duketopceo.wordink.png"
check "the versions are removed" absent "$support/versions"
check "current is removed" absent "$support/current"
check "the service was disabled and stopped" contains "$sysd/log" "disable --now wordink.service"
check "settings, history and models stay" same_data
check "the config folder stays" [ -f "$home/.config/wordink/hex.config.ts" ]
check "it says where the data stays" contains "$temporary/out" "Settings and history stay"
check "uninstall did not clear the keyring" absent "$sysd/secret-tool.log"
if command -v setsid >/dev/null 2>&1; then
  # No controlling terminal, so no way to confirm: --purge must refuse.
  status=0
  env PATH="$stubs:$bin_dir:$PATH" WORDINK_INSTALL_BASE="$base" WORDINK_INSTALL_TEST_PUBLIC_KEY="$fixture_key" \
    WORDINK_SYSTEM_BINARY="$nowhere" setsid -w sh -s -- uninstall --purge < "$bootstrap" > "$temporary/out" 2>&1 || status=$?
  check "--purge with no terminal and no --yes refuses" status_not 0
  check "it says to add --yes" contains "$temporary/out" "--yes"
  check "and removes nothing" same_data
fi
run_bootstrap uninstall --purge --yes
check "uninstall --purge --yes exits 0" status_is 0
check "the data directory is removed" absent "$support"
check "the config folder is removed" absent "$home/.config/wordink"
check "the keyring entries are cleared" contains "$sysd/secret-tool.log" "clear service wordink"
check "the home directory itself is untouched" [ -d "$home" ]

echo "scenario: purge refuses dangerous data directories"
reset
mkdir -p "$temporary/important"
touch "$temporary/important/keep"
ln -s "$temporary/important" "$temporary/support-link"
export HEX_APPLICATION_SUPPORT_DIR="$temporary/support-link"
run_bootstrap uninstall --purge --yes
check "a symlinked data directory is never purged" [ -f "$temporary/important/keep" ]
export HEX_APPLICATION_SUPPORT_DIR="$home"
run_bootstrap uninstall --purge --yes
check "HOME is never purged" [ -d "$home" ]
unset HEX_APPLICATION_SUPPORT_DIR

echo "scenario: a release built by the real release scripts"
# make-tarball.sh, sign-linux.sh (binaries, then --sums), write-sums.sh and
# verify-release.sh, with the throwaway key, then the bootstrap against it.
reset
release="$temporary/release-dist"
rm -rf "${release:?}" "${server:?}"/*
mkdir -p "$release"
openssl pkey -in "$temporary/signing.pem" -pubout -out "$temporary/signing.pub.pem" 2> /dev/null
linux="$root/packaging/linux"
for target in x64 arm64; do
  mkbin "$release/wordink-1.0.0-rc.1-linux-$target" 1.0.0-rc.1
  WORDINK_VERSION=1.0.0-rc.1 bash "$linux/make-tarball.sh" "$release" "$target" > /dev/null
done
printf 'zip\n' > "$release/wordink-1.0.0-rc.1-macos-arm64.zip"
cp "$bootstrap" "$release/install.sh"
export WORDINK_SIGNING_PUBLIC_KEY_FILE="$temporary/signing.pub.pem"
WORDINK_LINUX_SIGNING_KEY=$(cat "$temporary/signing.pem") WORDINK_VERSION=1.0.0-rc.1 bash "$linux/sign-linux.sh" "$release" > /dev/null
bash "$linux/write-sums.sh" "$release" > /dev/null
WORDINK_LINUX_SIGNING_KEY=$(cat "$temporary/signing.pem") WORDINK_VERSION=1.0.0-rc.1 bash "$linux/sign-linux.sh" --sums "$release" > /dev/null
check "verify-release.sh accepts the fixture release" bash "$linux/verify-release.sh" "$release"
unset WORDINK_SIGNING_PUBLIC_KEY_FILE WORDINK_LINUX_SIGNING_KEY
cp "$release"/* "$server/"
run_bootstrap
check "the bootstrap installs a release signed by the release scripts" status_is 0
check "it installed the rc for this CPU" [ "$(installed_version)" = versions/1.0.0-rc.1 ]
check "the service runs it" runs_binary "$support/versions/1.0.0-rc.1/wordink"
check "output is at most five lines" line_count_at_most "$temporary/out" 5

echo "scenario: an upgrade whose new version does not start"
reset
publish 1.0.0
run_bootstrap
cp "$units/wordink.service" "$temporary/unit.before"
publish 1.1.0 bad
run_bootstrap
check "a failed upgrade exits non-zero" status_not 0
check "it says the previous version is back" contains "$temporary/out" "WordInk 1.0.0 is back"
check "current points at the previous version again" [ "$(installed_version)" = versions/1.0.0 ]
check "the failed version is removed" absent "$support/versions/1.1.0"
check "the service is running the previous version" runs_binary "$support/versions/1.0.0/wordink"
check "the service is active" [ -e "$sysd/active" ]
check "the unit is restored" cmp "$units/wordink.service" "$temporary/unit.before"
check "the receipt is still there" [ -f "$support/install.json" ]
publish 1.1.0
run_bootstrap
check "a good 1.1.0 then upgrades normally" [ "$(installed_version)" = versions/1.1.0 ]

echo "scenario: version ordering"
sed -n '/^version_ge() {/,/^}/p' "$root/packaging/linux/install-release.sh" > "$temporary/version_ge.sh"
ordered() { # <a> <b> <ge|lt>
  # shellcheck disable=SC1091
  . "$temporary/version_ge.sh"
  if version_ge "$1" "$2"; then got=ge; else got=lt; fi
  [ "$got" = "$3" ]
}
for row in "1.0.0 1.0.0-rc.1 ge" "1.0.0-rc.1 1.0.0 lt" "1.0.0-rc.10 1.0.0-rc.9 ge" "1.0.0-rc.9 1.0.0-rc.10 lt" \
  "1.0.10 1.0.9 ge" "1.0.9 1.0.10 lt" "1.10.0 1.9.0 ge" "1.9.0 1.10.0 lt" "01.0.0 1.0.0 ge" "1.0.0 01.0.0 ge" \
  "1.0.0 1.0.0 ge" "1.0.0-rc.1 1.0.0-rc.1 ge" "1.0.0-beta 1.0.0-alpha ge" "1.0.0-alpha 1.0.0-alpha.1 lt" \
  "1.0.0-alpha.1 1.0.0-alpha.beta lt" "1.0.0+build.5 1.0.0 ge" "1.0 1.0.0 ge" "1.0.0 1.0.1 lt" "2.0.0 1.99.99 ge"; do
  # shellcheck disable=SC2086 # a row is three words
  set -- $row
  check "version_ge $1 $2 is $3" ordered "$1" "$2" "$3"
done

echo "scenario: tarball installer on its own"
reset
publish 1.0.0
unpack="$temporary/unpack"
rm -rf "${unpack:?}"
mkdir -p "$unpack"
tar -xzf "$server/wordink-1.0.0-linux-$arch.tar.gz" -C "$unpack"
inner="$unpack/wordink-1.0.0-linux-$arch"
check "the tarball carries install-release.sh" [ -f "$inner/install-release.sh" ]
check "the tarball carries the unit" [ -f "$inner/wordink.service" ]
check "the tarball carries the icons" [ -d "$inner/icons" ]
check "the tarball has no bootstrap" absent "$inner/install.sh"
check "the tarball has no signature (it is signed through SHA256SUMS)" absent "$inner/wordink.sig"
status=0
env PATH="$stubs:$PATH" WORDINK_SYSTEM_BINARY="$nowhere" sh "$inner/install-release.sh" > "$temporary/out" 2>&1 || status=$?
check "without a signature or the bootstrap's hash it refuses" status_not 0
check "it names the missing signature" contains "$temporary/out" "wordink.sig is missing"
status=0
env PATH="$stubs:$PATH" WORDINK_SYSTEM_BINARY="$nowhere" WORDINK_BOOTSTRAP_VERIFIED=0000 sh "$inner/install-release.sh" > "$temporary/out" 2>&1 || status=$?
check "a wrong bootstrap hash refuses" contains "$temporary/out" "does not match what the bootstrap verified"
binary_sha=$(sha256sum "$inner/wordink" | cut -c1-64)
printf 'wordink.linux-artifact.v1\n%s\n' "$binary_sha" > "$temporary/message"
openssl pkeyutl -sign -rawin -inkey "$temporary/signing.pem" -in "$temporary/message" -out "$inner/wordink.sig"
status=0
env PATH="$stubs:$PATH" WORDINK_SYSTEM_BINARY="$nowhere" sh "$inner/install-release.sh" > "$temporary/out" 2>&1 || status=$?
check "a signature from a key other than the embedded one refuses" contains "$temporary/out" "does not verify"
check "the failure installed nothing" absent "$support/current"

echo
echo "Linux installer test: $passed checks passed, $failed failed."
[ "$failed" = 0 ]
