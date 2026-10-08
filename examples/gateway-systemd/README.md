# WordInk gateway as a systemd user service

This runs `wordink-gateway serve` on `127.0.0.1:8941`. Desktop dictation apps such as Voxtype, or any client of the OpenAI audio API, point at it. Provider keys come from omaseal at start and never touch disk.

```sh
# @wordink/server isn't on npm yet; build the CLI from this repo:
pnpm --filter @wordink/server build
install -Dm755 packages/server/dist/bin/wordink-gateway.js ~/.local/bin/wordink-gateway
# (once published: npm install -g @wordink/server)

mkdir -p ~/.config/wordink ~/.local/bin ~/.config/systemd/user
cp gateway.example.json ~/.config/wordink/gateway.json          # edit providers/vocabulary
# key map: one ENV_NAME=service/account per line, matching the keyEnv names in gateway.json
printf 'GROQ_API_KEY=groq/default\nGROQ_API_KEY_2=Groq-Hermes-API\n' > ~/.config/wordink/gateway-keys
install -m 755 wordink-gateway-run ~/.local/bin/
cp wordink-gateway.service ~/.config/systemd/user/
wordink-gateway check                     # which providers have keys (never prints them)
systemctl --user daemon-reload && systemctl --user enable --now wordink-gateway
wordink-gateway tokens create laptop      # prints the device token once
```

The service sets a minimal `Environment=PATH=…` that includes `~/.local/bin`; if `wordink-gateway` lives elsewhere, set `WORDINK_GATEWAY_BIN` in a service drop-in.

Without omaseal, put the variables in a systemd `EnvironmentFile=` with mode 0600, or export them before `wordink-gateway serve`.

See the docs page "Desktop apps" for client setup, fallback behavior and token management.
