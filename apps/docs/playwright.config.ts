import { mkdirSync } from "node:fs";
import { defineConfig, devices } from "@playwright/test";
import { DOCS_HOST, QUICKSTART_DIR, QUICKSTART_PORT, SITE_PORT, writeFakeMicClip } from "./e2e/env.js";

mkdirSync(QUICKSTART_DIR, { recursive: true });
writeFakeMicClip();

// Chromium only; the mic is a getUserMedia stub (see fakeMicFromClip). The model is real (Moonshine
// on WASM), so allow minutes. `pnpm test:e2e` first builds the site with base /WordInk/ into e2e/.site.
export default defineConfig({
  testDir: "e2e",
  timeout: 300_000,
  expect: { timeout: 180_000 },
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  webServer: [
    {
      command: `node e2e/server.mjs ${SITE_PORT} e2e/.site /WordInk/`,
      url: `http://127.0.0.1:${SITE_PORT}/__health`,
      reuseExistingServer: !process.env.CI,
    },
    {
      command: `node e2e/server.mjs ${QUICKSTART_PORT} ${QUICKSTART_DIR} /`,
      url: `http://127.0.0.1:${QUICKSTART_PORT}/__health`,
      reuseExistingServer: !process.env.CI,
    },
  ],
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        channel: "chromium",
        launchOptions: {
          args: [
            "--use-fake-ui-for-media-stream",
            "--autoplay-policy=no-user-gesture-required",
            `--host-resolver-rules=MAP ${DOCS_HOST} 127.0.0.1`,
            // The local engine verifies model checksums with WebCrypto, which needs a secure context.
            `--unsafely-treat-insecure-origin-as-secure=http://${DOCS_HOST}:${SITE_PORT}`,
          ],
        },
      },
    },
  ],
});
