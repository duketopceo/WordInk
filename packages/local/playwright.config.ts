import { buildSync } from "esbuild";
import { fileURLToPath } from "node:url";
import { defineConfig, devices } from "@playwright/test";

const PORT = 4318;

// The page has no bundler: bundle the provider and its worker (with transformers.js) for the browser.
// `new URL("./worker.js", import.meta.url)` in index.js then finds the bundled worker next to it.
buildSync({
  entryPoints: { index: "src/index.ts", worker: "src/worker.ts" },
  absWorkingDir: fileURLToPath(new URL(".", import.meta.url)),
  outdir: "e2e/.build",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "error",
});

// Model loads are real (WASM inference on CPU): allow minutes, not seconds.
export default defineConfig({
  testDir: "e2e",
  timeout: 180_000,
  expect: { timeout: 120_000 },
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    command: `node e2e/server.mjs ${PORT}`,
    url: `http://127.0.0.1:${PORT}/e2e/index.html`,
    reuseExistingServer: !process.env.CI,
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "firefox", use: { ...devices["Desktop Firefox"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
});
