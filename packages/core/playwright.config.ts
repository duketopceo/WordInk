import { mkdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";
import { clipWav } from "./e2e/clip.js";

const PORT = 4317;

// The recorded clip Chromium's fake device plays (looped).
const fixtures = fileURLToPath(new URL("./e2e/.fixtures/", import.meta.url));
mkdirSync(fixtures, { recursive: true });
const CLIP = `${fixtures}clip.wav`;
writeFileSync(CLIP, clipWav());

// Chromium and Firefox capture from the browser's fake media device; WebKit has none, so the
// spec injects a getUserMedia stub (see e2e/dictation.spec.ts).
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    command: `node e2e/server.mjs ${PORT}`,
    url: `http://127.0.0.1:${PORT}/e2e/index.html`,
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        launchOptions: {
          args: ["--use-fake-ui-for-media-stream", "--use-fake-device-for-media-stream",
            `--use-file-for-fake-audio-capture=${CLIP}`,
            "--autoplay-policy=no-user-gesture-required"],
        },
      },
    },
    {
      name: "firefox",
      use: {
        ...devices["Desktop Firefox"],
        launchOptions: {
          firefoxUserPrefs: {
            "media.navigator.streams.fake": true,
            "media.navigator.permission.disabled": true,
            "media.autoplay.default": 0,
          },
        },
      },
    },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
});
