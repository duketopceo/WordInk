#!/bin/sh
# Builds WordInk from this checkout and installs it for the current user.
#
#   apps/desktop/scripts/install-linux.sh [install|uninstall]
#
# Layout (user-local, no root):
#   ~/.local/share/wordink/current/wordink   the binary (+ source-build marker)
#   ~/.local/bin/wordink                     symlink to it
#   ~/.local/share/applications/io.github.duketopceo.wordink.desktop
#   ~/.local/share/icons/hicolor/*/apps/io.github.duketopceo.wordink.png
#   ~/.config/systemd/user/wordink.service   (enabled, started)
#
# A source build never updates itself; pull and run this again to update.
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
bin_dir=${WORDINK_BIN_DIR:-"$HOME/.local/bin"}
app_dir="$data_home/wordink/current"
applications="$data_home/applications"
icons="$data_home/icons/hicolor"
units="$config_home/systemd/user"
app_id=io.github.duketopceo.wordink

# The bin dir is substituted into sed replacements below, where a space, `&`,
# `\`, `#` or `%` would corrupt the .desktop and unit files.
case "$bin_dir" in
  *[[:space:]\&\\#%]*)
    echo "WORDINK_BIN_DIR ($bin_dir) can't contain a space or any of & \\ # %; pick another directory." >&2
    exit 1
    ;;
esac

if [ "$(id -u)" = 0 ]; then
  echo "Run this as your desktop user, not root." >&2
  exit 1
fi

uninstall() {
  systemctl --user disable --now wordink.service 2>/dev/null || true
  rm -f "$units/wordink.service" "$applications/$app_id.desktop"
  if [ -L "$bin_dir/wordink" ]; then rm -f "$bin_dir/wordink"; fi
  rm -rf "$app_dir"
  find "$icons" -name "$app_id.png" -delete 2>/dev/null || true
  systemctl --user daemon-reload 2>/dev/null || true
  echo "WordInk uninstalled. Settings and history stay in $data_home/wordink."
}

case "${1:-install}" in
  install) ;;
  uninstall) uninstall; exit 0 ;;
  *) echo "Usage: $0 [install|uninstall]" >&2; exit 2 ;;
esac

target_dir=${CARGO_TARGET_DIR:-"$root/target"}
HEX_LOCAL_BUILD=1 cargo build --locked --release --manifest-path "$root/Cargo.toml"
binary="$target_dir/release/wordink"
[ -x "$binary" ] || { echo "Build did not produce $binary" >&2; exit 1; }

# A non-symlink ~/.local/bin/wordink belongs to something else; refuse.
if [ -e "$bin_dir/wordink" ] && [ ! -L "$bin_dir/wordink" ]; then
  echo "Refusing to replace $bin_dir/wordink: it is not a symlink." >&2
  exit 1
fi

mkdir -p "$app_dir" "$bin_dir" "$applications" "$units"
install -m 755 "$binary" "$app_dir/wordink.new"
mv -f "$app_dir/wordink.new" "$app_dir/wordink"
"$app_dir/wordink" --version | awk 'NF { v=$NF } END { print v }' > "$app_dir/source-build"
ln -sfn "$app_dir/wordink" "$bin_dir/wordink"

sed "s#@HEX_BIN@#$bin_dir/wordink#g" "$root/packaging/$app_id.desktop" > "$applications/$app_id.desktop"
for png in "$root"/packaging/linux/icons/hicolor/*/"$app_id.png"; do
  [ -f "$png" ] || continue
  size=$(basename "$(dirname "$png")")
  mkdir -p "$icons/$size/apps"
  install -m 644 "$png" "$icons/$size/apps/$app_id.png"
done

sed "s#^ExecStart=/usr/bin/wordink#ExecStart=$bin_dir/wordink#" "$root/packaging/wordink.service" > "$units/wordink.service"
systemctl --user daemon-reload
systemctl --user enable wordink.service >/dev/null
systemctl --user restart wordink.service

echo "WordInk $(cat "$app_dir/source-build") installed. Open the window with: wordink app"
