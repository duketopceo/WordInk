# WordInk gateway as a systemd user service

This runs `wordink-gateway serve` on `127.0.0.1:8941`. Desktop dictation apps, or any client of the OpenAI audio API, point at it. Provider keys come from the desktop keyring (libsecret, through `secret-tool`) at start and never touch disk.

```sh
# On Omarchy, ./install-omarchy.sh in this directory does all of this and can wire Voxtype.
# @wordink/server isn't on npm yet; build the CLI from this repo:
pnpm --filter @wordink/server build
chmod +x packages/server/dist/bin/wordink-gateway.js
# symlink, don't copy: the compiled CLI imports sibling modules by relative path
ln -sf "$PWD/packages/server/dist/bin/wordink-gateway.js" ~/.local/bin/wordink-gateway
# (once published: npm install -g @wordink/server)

mkdir -p ~/.config/wordink ~/.local/bin ~/.config/systemd/user
cp gateway.example.json ~/.config/wordink/gateway.json          # edit providers/vocabulary
# store a key in the keyring (prompts for the value), then map it: one ENV_NAME=service/account
# per line, matching the keyEnv names in gateway.json
secret-tool store --label='WordInk Groq key' service groq account default
printf 'GROQ_API_KEY=groq/default\nGROQ_API_KEY_2=groq/backup\n' > ~/.config/wordink/gateway-keys
install -m 755 wordink-gateway-run ~/.local/bin/
cp wordink-gateway.service ~/.config/systemd/user/
wordink-gateway check                     # which providers have keys (never prints them)
systemctl --user daemon-reload && systemctl --user enable --now wordink-gateway
wordink-gateway tokens create laptop      # prints the device token once
wordink-gateway tokens create --admin my-agent  # admin token for /v1/vocabulary (optional)
```

The service sets a minimal `Environment=PATH=…` that includes `~/.local/bin`; if `wordink-gateway` lives elsewhere, set `WORDINK_GATEWAY_BIN` in a service drop-in.

Without a keyring, put the variables in a systemd `EnvironmentFile=` with mode 0600, or export them before `wordink-gateway serve`. Any other keyring helper that prints a secret works too: change the single `secret-tool lookup` line in `wordink-gateway-run`.

See the docs page "Desktop apps" for client setup, fallback behavior and token management.
