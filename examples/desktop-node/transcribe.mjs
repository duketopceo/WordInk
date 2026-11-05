// Transcribe a WAV file with the running WordInk desktop app.
//   node transcribe.mjs clip.wav [model-id] [language]
// Needs the app running and @wordink/desktop built (pnpm --filter @wordink/desktop build).
import { readFile } from "node:fs/promises";
import { NotRunningError, UnsupportedVersionError, connect } from "@wordink/desktop";

const [file, model, language] = process.argv.slice(2);
if (!file) {
  console.error("usage: node transcribe.mjs <clip.wav> [model-id] [language]");
  process.exit(2);
}

try {
  const client = await connect();
  const options = {};
  if (model) options.model = model;
  if (language) options.language = language;
  console.log(await client.transcribe(await readFile(file), options));
} catch (error) {
  if (error instanceof NotRunningError) console.error("WordInk is not running. Start it and try again.");
  else if (error instanceof UnsupportedVersionError) console.error(error.message);
  else console.error(error);
  process.exit(1);
}
