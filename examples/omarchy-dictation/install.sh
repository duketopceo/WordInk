#!/usr/bin/env bash
# Install WordInk dictation (wordink-dictd) on Omarchy / Hyprland.
#
#   install.sh                 install files, unit and binds; does NOT touch Voxtype
#   install.sh --switch        also stop and disable Voxtype, start WordInk, and write the
#                              one-command rollback ~/.local/bin/wordink-use-voxtype
#   install.sh --uninstall     remove everything this script added (keeps your key and vocabulary)
#   install.sh --dry-run ...   print every action, change nothing
#   install.sh --install-packages   also run `sudo pacman -S --needed` for missing tools
#
# Re-running is safe: every step checks before it writes.
set -euo pipefail

DRY=0 SWITCH=0 UNINSTALL=0 PACKAGES=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY=1 ;;
    --switch) SWITCH=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --install-packages) PACKAGES=1 ;;
    -h|--help) sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "install.sh: unknown option $arg" >&2; exit 2 ;;
  esac
done

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
BIN="$HOME/.local/bin"
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}/wordink"
UNIT_DIR="$CFG/systemd/user"
UNIT="wordink-dictd.service"
HYPR_CONF="$CFG/hypr/hyprland.conf"
BINDS_CONF="$CFG/hypr/wordink-dictd.conf"
ENV_FILE="$CFG/wordink/dictd.env"
ROLLBACK="$BIN/wordink-use-voxtype"
MARK_BEGIN="# >>> wordink-dictd (managed by examples/omarchy-dictation/install.sh) >>>"
MARK_END="# <<< wordink-dictd <<<"
# Hyprland 0.55+ can run a Lua config (Omarchy's hyprland.lua). When it does, hyprland.conf is
# not read at all, so the binds go through Omarchy's `o.bind` instead.
LUA_MAIN="$CFG/hypr/hyprland.lua"
LUA_BINDS="$CFG/hypr/wordink-dictd.lua"
LUA_BEGIN="-- >>> wordink-dictd (managed by examples/omarchy-dictation/install.sh) >>>"
LUA_END="-- <<< wordink-dictd <<<"
PY_SRC="git+https://github.com/duketopceo/WordInk#subdirectory=sdks/python"
[ -f "$REPO/sdks/python/pyproject.toml" ] && PY_SRC="$REPO/sdks/python"

strip_block() { # strip_block <file> <begin> <end>: drop the managed block, keep everything else
  local file="$1" tmp
  [ -f "$file" ] && grep -qF -- "$2" "$file" || return 0
  act "remove the managed block from $file"
  if [ "$DRY" = 0 ]; then
    tmp="$(mktemp)"
    awk -v b="$2" -v e="$3" '$0==b{skip=1} !skip{print} $0==e{skip=0}' "$file" >"$tmp"
    cat "$tmp" >"$file"; rm -f "$tmp"
  fi
}
lua_host() { if [ -f "$CFG/hypr/bindings.lua" ]; then say "$CFG/hypr/bindings.lua"; else say "$LUA_MAIN"; fi; }

say() { printf '%s\n' "$*"; }
act() { if [ "$DRY" = 1 ]; then say "[dry-run] $*"; else say "+ $*"; fi; }
run() { act "$*"; [ "$DRY" = 1 ] || "$@"; }
have() { command -v "$1" >/dev/null 2>&1; }
user_ctl() { systemctl --user "$@"; }

# Find Voxtype's user unit without guessing at a name that may not exist.
voxtype_unit() {
  if [ -n "${VOXTYPE_UNIT:-}" ]; then say "$VOXTYPE_UNIT"; return 0; fi
  user_ctl list-unit-files --no-legend 'voxtype*.service' 2>/dev/null | awk 'NR==1{print $1}'
}

write_file() { # write_file <path> <mode> ; content on stdin; leaves identical files alone
  local path="$1" mode="$2" tmp
  tmp="$(mktemp)"; cat >"$tmp"
  if [ -f "$path" ] && cmp -s "$tmp" "$path"; then rm -f "$tmp"; say "= $path (unchanged)"; return 0; fi
  act "write $path"
  if [ "$DRY" = 0 ]; then mkdir -p "$(dirname "$path")"; install -m "$mode" "$tmp" "$path"; fi
  rm -f "$tmp"
}

# ---------------------------------------------------------------------------------------------
if [ "$UNINSTALL" = 1 ]; then
  say "Uninstalling WordInk dictation (your $ENV_FILE, vocabulary and dictate.toml are kept)."
  if [ "$DRY" = 1 ]; then act "systemctl --user disable --now $UNIT"
  elif user_ctl disable --now "$UNIT" 2>/dev/null; then say "+ disabled $UNIT"
  else say "= $UNIT was not enabled"; fi
  for f in "$UNIT_DIR/$UNIT" "$BINDS_CONF" "$LUA_BINDS" "$ROLLBACK"; do
    if [ -e "$f" ]; then run rm -f "$f"; else say "= $f (absent)"; fi
  done
  strip_block "$HYPR_CONF" "$MARK_BEGIN" "$MARK_END"
  for f in "$CFG/hypr/bindings.lua" "$LUA_MAIN"; do strip_block "$f" "$LUA_BEGIN" "$LUA_END"; done
  [ "$DRY" = 1 ] || user_ctl daemon-reload 2>/dev/null || true
  if have uv; then run uv tool uninstall wordink || true; fi
  have hyprctl && [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] && run hyprctl reload || true
  say "Done. Voxtype was not touched by this script unless you ran --switch; run $ROLLBACK first if you did."
  exit 0
fi

# ---------------------------------------------------------------------------------------------
say "1/6 tools"
need=()
have pw-record   || need+=(pipewire)
have wtype       || need+=(wtype)
have wl-copy     || need+=(wl-clipboard)
have notify-send || need+=(libnotify)
have hyprctl     || { say "!! hyprctl not found: this installer is for Hyprland (Omarchy)."; [ "$DRY" = 1 ] || exit 1; }
have uv          || need+=(uv)
if [ "${#need[@]}" -gt 0 ]; then
  if [ "$PACKAGES" = 1 ]; then run sudo pacman -S --needed --noconfirm "${need[@]}"
  else say "Missing: ${need[*]}. Install with: sudo pacman -S --needed ${need[*]}   (or re-run with --install-packages)"
       [ "$DRY" = 1 ] || have uv || exit 1
  fi
fi

say "2/6 wordink-dictd"
run uv tool install --force "$PY_SRC"

say "3/6 key file and service"
if [ -f "$ENV_FILE" ]; then say "= $ENV_FILE (kept)"
else write_file "$ENV_FILE" 600 <<'ENVEOF'
# Put the key here, one KEY=value per line. This file is read by the systemd user service only.
# GROQ_API_KEY=
ENVEOF
fi
[ "$DRY" = 1 ] || chmod 600 "$ENV_FILE" 2>/dev/null || true
write_file "$UNIT_DIR/$UNIT" 644 <"$HERE/$UNIT"
[ "$DRY" = 1 ] || user_ctl daemon-reload

say "4/6 Hyprland binds (checked for clashes first)"
if have wordink-dictd && [ "$DRY" = 0 ]; then
  clash="$(wordink-dictd doctor --json 2>/dev/null | python3 -c '
import json, sys
try: checks = json.load(sys.stdin)
except ValueError: sys.exit(0)
for c in checks:
    if c["name"] in ("hold bind", "toggle bind") and c["status"] == "fail":
        print(c["detail"])' || true)"
  if [ -n "$clash" ]; then
    say "!! A configured key is already bound, so no binds were added:"; say "$clash"
    say "   Change [hotkey] in $CFG/wordink/dictate.toml, then re-run."
    exit 1
  fi
fi
if [ -f "$LUA_MAIN" ]; then
  say "   (Hyprland Lua config found at $LUA_MAIN)"
  host="$(lua_host)"
  if [ "$DRY" = 1 ]; then act "write $LUA_BINDS (from: wordink-dictd binds --format lua)"
  else wordink-dictd binds --format lua --exe "$BIN/wordink-dictd" | write_file "$LUA_BINDS" 644; fi
  if [ -f "$host" ] && grep -qF -- "$LUA_BEGIN" "$host"; then say "= $host (already loads the binds)"
  else
    act "append a guarded dofile of $LUA_BINDS to $host"
    if [ "$DRY" = 0 ]; then
      [ ! -s "$host" ] || [ -z "$(tail -c1 "$host")" ] || printf '\n' >>"$host"
      printf '%s\ndo local p = (os.getenv("XDG_CONFIG_HOME") or (os.getenv("HOME") .. "/.config")) .. "/hypr/wordink-dictd.lua"; local f = io.open(p, "r"); if f then f:close(); dofile(p) end end\n%s\n' "$LUA_BEGIN" "$LUA_END" >>"$host"
      sleep 1  # some setups (a config-sync guard) quietly restore the file; say so instead of leaving dead binds
      if ! grep -qF -- "$LUA_BEGIN" "$host"; then
        say "!! $host was reverted right after the edit (a sync guard restores it). The binds are NOT persistent."
        say "   Load them for this session:  hyprctl eval 'dofile(os.getenv(\"HOME\") .. \"/.config/hypr/wordink-dictd.lua\")'"
        say "   To keep them, add the same guarded dofile block to the file your guard restores from."
      fi
    fi
  fi
else
  if [ "$DRY" = 1 ]; then act "write $BINDS_CONF (from: wordink-dictd binds)"
  else wordink-dictd binds | write_file "$BINDS_CONF" 644; fi
  if [ -f "$HYPR_CONF" ] && grep -qF "$MARK_BEGIN" "$HYPR_CONF"; then say "= $HYPR_CONF (already sources the binds)"
  else
    act "append a source line for $BINDS_CONF to $HYPR_CONF"
    if [ "$DRY" = 0 ]; then
      mkdir -p "$(dirname "$HYPR_CONF")"
      [ ! -s "$HYPR_CONF" ] || [ -z "$(tail -c1 "$HYPR_CONF")" ] || printf '\n' >>"$HYPR_CONF"
      printf '%s\nsource = %s\n%s\n' "$MARK_BEGIN" "$BINDS_CONF" "$MARK_END" >>"$HYPR_CONF"
    fi
  fi
fi
have hyprctl && [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] && run hyprctl reload || true

say "5/6 start WordInk dictation"
if [ "$SWITCH" = 1 ]; then
  vt="$(voxtype_unit || true)"
  if [ -z "$vt" ]; then say "!! No Voxtype user unit found (set VOXTYPE_UNIT=name.service). Not switching."; exit 1; fi
  act "remember Voxtype's state in $STATE"
  if [ "$DRY" = 0 ]; then
    mkdir -p "$STATE"
    { printf 'unit=%s\n' "$vt"; printf 'enabled=%s\n' "$(user_ctl is-enabled "$vt" 2>/dev/null || echo unknown)"
      printf 'active=%s\n' "$(user_ctl is-active "$vt" 2>/dev/null || echo unknown)"; } >"$STATE/voxtype-before"
  fi
  write_file "$ROLLBACK" 755 <<RBEOF
#!/bin/sh
# One command back to Voxtype. Generated by examples/omarchy-dictation/install.sh --switch.
systemctl --user disable --now $UNIT || true
systemctl --user enable --now $vt
echo "Voxtype ($vt) is back. WordInk dictation is stopped; re-run install.sh --switch to return."
RBEOF
  run systemctl --user disable --now "$vt"
  run systemctl --user enable --now "$UNIT"
else
  say "Voxtype is untouched. WordInk is installed but not started, so the two never fight over the mic."
  say "Try it without switching: systemctl --user start $UNIT   (stop with: systemctl --user stop $UNIT)"
fi

say "6/6 checks"
if [ "$DRY" = 0 ] && have wordink-dictd; then wordink-dictd doctor || true; fi
cat <<NEXT

Next:
  * put GROQ_API_KEY in $ENV_FILE
  * waybar: merge $HERE/waybar-module.jsonc into your waybar config and append waybar-style.css
  * switch for good:   $0 --switch        (undo anytime with: wordink-use-voxtype)
NEXT
