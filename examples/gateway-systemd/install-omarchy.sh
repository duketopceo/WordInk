#!/bin/sh
# install-omarchy.sh: set up wordink-gateway as a systemd user service on Omarchy and point
# Voxtype at it. Run from a checkout of this repo, after `pnpm install`.
#
#   examples/gateway-systemd/install-omarchy.sh
#
# Idempotent: existing gateway.json, gateway-keys and a Voxtype [whisper] section are left alone.
# The Groq key is never read here; it stays in omaseal as groq/default (see gateway-keys).
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
BIN="$HOME/.local/bin"
VOX="$CFG/voxtype/config.toml"

pnpm --dir "$ROOT" --filter @wordink/server build
CLI="$ROOT/packages/server/dist/bin/wordink-gateway.js"
chmod +x "$CLI"

mkdir -p "$BIN" "$CFG/wordink" "$CFG/systemd/user"
# Symlink, don't copy: the compiled CLI imports its sibling modules by relative path.
ln -sf "$CLI" "$BIN/wordink-gateway"
install -m 755 "$HERE/wordink-gateway-run" "$BIN/"
install -m 644 "$HERE/wordink-gateway.service" "$CFG/systemd/user/"
[ -f "$CFG/wordink/gateway.json" ] || cp "$HERE/gateway.example.json" "$CFG/wordink/gateway.json"
[ -f "$CFG/wordink/gateway-keys" ] || printf 'GROQ_API_KEY=groq/default\n' > "$CFG/wordink/gateway-keys"

if ! omaseal get groq default >/dev/null 2>&1; then
  echo "install-omarchy: no groq/default in omaseal yet. Run: omaseal set groq default" >&2
  echo "install-omarchy: the gateway refuses to start until a provider key resolves." >&2
fi

systemctl --user daemon-reload
systemctl --user enable wordink-gateway
systemctl --user restart wordink-gateway || echo "install-omarchy: gateway not running yet (see above)" >&2

if [ -f "$VOX" ] && ! grep -q '^\[whisper\]' "$VOX"; then
  # The token is printed once; capture it straight into the 0600 Voxtype config, never to the terminal.
  token=$("$BIN/wordink-gateway" tokens create voxtype 2>/dev/null | grep -o 'wdk_[A-Za-z0-9_-]*' | head -n 1)
  if [ -n "$token" ]; then
    umask 077
    cat >> "$VOX" <<EOF

[whisper]
mode = "remote"
remote_endpoint = "http://127.0.0.1:8941"
remote_api_key = "$token"
EOF
    # The file already existed, so umask does not tighten it: the bearer token must not sit at 0644.
    chmod 600 "$VOX"
    echo "install-omarchy: Voxtype now dictates through the gateway ($VOX)"
  else
    echo "install-omarchy: token mint produced nothing; mint one by hand with $BIN/wordink-gateway tokens create voxtype" >&2
  fi
else
  echo "install-omarchy: Voxtype config missing or already has a [whisper] section; wire it by hand (apps/docs/pages/desktop.md, 'Voxtype (Linux)')"
fi
