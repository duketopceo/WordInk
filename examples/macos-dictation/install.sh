#!/usr/bin/env bash
# Install WordInk dictation (wordink-dictd-mac) on macOS as a login LaunchAgent.
#
#   install.sh                  install the tool, a stable launcher and the LaunchAgent, then start it.
#                               Touches no other app.
#   install.sh --no-start       install everything but do not load the LaunchAgent (it still starts at login)
#   install.sh --switch APP     also quit the dictation app you name (for example "Superwhisper") and write
#                               a one-command rollback, ~/.local/bin/wordink-use-previous
#   install.sh --uninstall      remove the LaunchAgent, launcher, rollback script and tool
#                               (keeps your Keychain key, vocabulary and config)
#   install.sh --dry-run ...    print every action, change nothing
#
# Re-running is safe: every step checks before it writes. Needs macOS, bash and uv (https://docs.astral.sh/uv/).
# This script never reads, prints or stores your Groq key; `wordink-dictd set-key` puts it in the Keychain.
set -euo pipefail

DRY=0 SWITCH="" UNINSTALL=0 START=1
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --no-start) START=0 ;;
    --switch)
      [ "$#" -ge 2 ] || { echo "install.sh: --switch needs the name of the app to quit" >&2; exit 2; }
      SWITCH="$2"; shift ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "install.sh: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done

if [ -n "$SWITCH" ]; then
  [ "$START" = 1 ] || { echo "install.sh: --switch cannot be combined with --no-start" >&2; exit 2; }
  [ "$UNINSTALL" = 0 ] || { echo "install.sh: --switch cannot be combined with --uninstall" >&2; exit 2; }
  case "$SWITCH" in
    *[\"\\]*|*$'\n'*) echo "install.sh: the app name may not contain quotes, backslashes or newlines" >&2; exit 2 ;;
  esac
fi

say() { printf '%s\n' "$*"; }
act() { if [ "$DRY" = 1 ]; then say "[dry-run] $*"; else say "+ $*"; fi; }
run() { act "$*"; [ "$DRY" = 1 ] || "$@"; }
have() { command -v "$1" >/dev/null 2>&1; }
die() { say "!! $*" >&2; exit 1; }

if [ "$(uname -s)" != "Darwin" ]; then die "this installer is for macOS (found $(uname -s)). On Linux use examples/omarchy-dictation; on Windows use examples/windows-dictation."; fi
[ -n "${HOME:-}" ] || die "HOME is not set"
case "$HOME" in
  *[\&\<\>\"]*|*$'\n'*) die "your home folder path contains a character this installer cannot put in a plist safely: $HOME" ;;
esac

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
LABEL="io.wordink.dictd"
BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
LAUNCHER="$BIN/wordink-dictd-mac"
ROLLBACK="$BIN/wordink-use-previous"
SUPPORT="$HOME/Library/Application Support/wordink"
SWITCH_STATE="$SUPPORT/switch-before"
LOG_DIR="$HOME/Library/Logs/wordink"
AGENT_DIR="$HOME/Library/LaunchAgents"
PLIST="$AGENT_DIR/$LABEL.plist"
DOMAIN="gui/$(id -u)"
PY_SRC="wordink[macos] @ git+https://github.com/duketopceo/WordInk#subdirectory=sdks/python"
[ -f "$REPO/sdks/python/pyproject.toml" ] && PY_SRC="wordink[macos] @ $REPO/sdks/python"

write_file() { # write_file <path> <mode> ; content on stdin; leaves identical files alone
  local path="$1" mode="$2" tmp
  tmp="$(mktemp)"; cat >"$tmp"
  if [ -f "$path" ] && [ ! -L "$path" ] && cmp -s "$tmp" "$path"; then rm -f "$tmp"; say "= $path (unchanged)"; return 0; fi
  act "write $path"
  if [ "$DRY" = 0 ]; then
    mkdir -p "$(dirname "$path")"
    [ ! -L "$path" ] || rm -f "$path"  # never write through uv's symlink into the tool's own environment
    install -m "$mode" "$tmp" "$path"
  fi
  rm -f "$tmp"
}
sq() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }  # single-quote a string for sh
agent_loaded() { launchctl print "$DOMAIN/$LABEL" >/dev/null 2>&1; }
agent_stop() { if agent_loaded; then run launchctl bootout "$DOMAIN/$LABEL"; else say "= $LABEL was not loaded"; fi; }

# ---------------------------------------------------------------------------------------------
if [ "$UNINSTALL" = 1 ]; then
  say "Uninstalling WordInk dictation (your Keychain key, vocabulary and dictate.toml are kept)."
  agent_stop
  for f in "$PLIST" "$ROLLBACK" "$SWITCH_STATE"; do
    if [ -e "$f" ]; then run rm -f "$f"; else say "= $f (absent)"; fi
  done
  if have uv; then run uv tool uninstall wordink || true; fi
  if [ -e "$LAUNCHER" ] || [ -L "$LAUNCHER" ]; then run rm -f "$LAUNCHER"; else say "= $LAUNCHER (absent)"; fi
  say "Done. Any app you quit with --switch was not reopened: run $ROLLBACK first next time if you want it back."
  exit 0
fi

# ---------------------------------------------------------------------------------------------
say "1/5 tools"
have launchctl || die "launchctl not found: is this macOS?"
if ! have uv; then
  die "uv is required and was not found. Install it first:  curl -LsSf https://astral.sh/uv/install.sh | sh   (or: brew install uv)"
fi

say "2/5 wordink-dictd"
run uv tool install --force "$PY_SRC"
TOOL_DIR="$(uv tool dir 2>/dev/null || true)"
[ -n "$TOOL_DIR" ] || TOOL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/uv/tools"
TARGET="$TOOL_DIR/wordink/bin/wordink-dictd-mac"
if [ "$DRY" = 0 ] && [ ! -x "$TARGET" ]; then die "uv did not create $TARGET"; fi

say "3/5 stable launcher (macOS ties Microphone and Accessibility grants to this path)"
write_file "$LAUNCHER" 755 <<LAUNCHEOF
#!/bin/sh
# Generated by examples/macos-dictation/install.sh. Starts the uv-installed WordInk dictation app.
exec $(sq "$TARGET") "\$@"
LAUNCHEOF

say "4/5 LaunchAgent"
[ -f "$HERE/$LABEL.plist.tmpl" ] || die "missing $HERE/$LABEL.plist.tmpl"
TMPL="$(cat "$HERE/$LABEL.plist.tmpl")"
RENDERED="${TMPL//__LAUNCHER__/$LAUNCHER}"
RENDERED="${RENDERED//__LOG_DIR__/$LOG_DIR}"
CHANGED=0
if [ ! -f "$PLIST" ] || [ "$(cat "$PLIST")" != "$RENDERED" ]; then CHANGED=1; fi
if [ "$DRY" = 0 ]; then mkdir -p "$LOG_DIR" "$SUPPORT"; fi
printf '%s\n' "$RENDERED" | write_file "$PLIST" 644

say "5/5 start WordInk dictation"
if [ -n "$SWITCH" ]; then
  running=no
  if pgrep -x "$SWITCH" >/dev/null 2>&1; then running=yes; fi
  if [ -f "$SWITCH_STATE" ]; then
    say "= $SWITCH_STATE (kept; a switch is already recorded)"
  else
    act "remember that $SWITCH was running=$running in $SWITCH_STATE"
    if [ "$DRY" = 0 ]; then printf 'app=%s\nrunning=%s\n' "$SWITCH" "$running" >"$SWITCH_STATE"; fi
  fi
  write_file "$ROLLBACK" 755 <<RBEOF
#!/bin/sh
# One command back to $SWITCH. Generated by examples/macos-dictation/install.sh --switch.
launchctl bootout $DOMAIN/$LABEL 2>/dev/null || true
if grep -qx 'running=yes' $(sq "$SWITCH_STATE") 2>/dev/null; then open -a $(sq "$SWITCH") || true; fi
rm -f $(sq "$SWITCH_STATE")
printf '%s\n' $(sq "WordInk dictation is stopped and $SWITCH is back. Re-run install.sh --switch to return.")
RBEOF
  if [ "$running" = yes ]; then
    run osascript -e "tell application \"$SWITCH\" to quit"
  else
    say "= $SWITCH is not running"
  fi
fi

if [ "$START" = 0 ]; then
  say "Not loading the LaunchAgent (--no-start). It starts at your next login, or run: launchctl bootstrap $DOMAIN $PLIST"
elif agent_loaded; then
  if [ "$CHANGED" = 1 ]; then
    run launchctl bootout "$DOMAIN/$LABEL"
    run launchctl bootstrap "$DOMAIN" "$PLIST"
  else
    say "= $LABEL is already loaded and up to date"
  fi
else
  run launchctl bootstrap "$DOMAIN" "$PLIST"
fi

if [ "$DRY" = 0 ] && [ -x "$BIN/wordink-dictd" ]; then "$BIN/wordink-dictd" doctor || true; fi
cat <<NEXT

Next:
  * store your Groq key in the Keychain:  $BIN/wordink-dictd set-key
  * when macOS asks, allow Microphone and Accessibility for the launcher ($LAUNCHER)
  * check anytime (read-only):  $BIN/wordink-dictd doctor
  * log: $LOG_DIR/dictd.log     remove: $0 --uninstall
NEXT
