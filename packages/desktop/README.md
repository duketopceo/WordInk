# @wordink/desktop

A small typed Node client for the WordInk desktop app's [local API](../../apps/desktop/docs/local-api.md). It finds the running app, checks the API version, and lets a script transcribe audio, start a dictation, or run the same commands as the `hex` CLI. No runtime dependencies beyond Node 20+.

The app must be running. Everything stays on your computer: the API listens on `127.0.0.1` only.

```sh
pnpm add @wordink/desktop
```

```ts
import { connect, NotRunningError } from "@wordink/desktop";
import { readFile } from "node:fs/promises";

try {
  const wordink = await connect();
  console.log(await wordink.transcribe(await readFile("clip.wav")));
} catch (error) {
  if (error instanceof NotRunningError) console.error("Start WordInk first.");
  else throw error;
}
```

## API

- `connect(options?)` reads `local-api.json`, refuses an `apiVersion` it does not know, and proves the app answers. Options: `discoveryPath` (a full path), `timeoutMs`. The file is found through `WORDINK_LOCAL_API_FILE`, then `HEX_APPLICATION_SUPPORT_DIR/local-api.json`, then the per-OS data folder (`$XDG_DATA_HOME/wordink/` on Linux, `~/Library/Application Support/wordink/` on macOS, `%APPDATA%\wordink\` on Windows).
- `client.health()` and `client.capabilities()`.
- `client.transcribe(audio, { mime?, model?, language? })` sends a WAV and returns the text. Without `model` it uses the first installed model.
- `client.dictate({ source? })` starts a dictation the app records itself and returns a handle with `finish()` (returns `{ text, durationMs }`), `cancel()` and `heartbeat()`. It needs `capabilities().serviceCapture` (macOS today); elsewhere it throws `ApiError` with code `service-capture-unavailable`.
- `client.control(command, args?)` runs a control command, for example `control("status")` or `control("dictation", { action: "toggle" })`. See the [CLI reference](../../apps/desktop/docs/cli.md).

## Errors

- `NotRunningError`: no discovery file, or the app is gone and the file is stale.
- `UnauthorizedError`: the app refused the token (it changes each time the app starts; reconnect).
- `UnsupportedVersionError`: the app speaks an `apiVersion` this client does not know; update the package.
- `ApiError`: any other error answer, with `status` and the app's `code`.

All extend `WordInkDesktopError`.

## Security

Any process running as your user can read the discovery file and use the API. See the [local API doc](../../apps/desktop/docs/local-api.md#security).

License: MIT.
