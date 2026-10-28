import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Both spawned-process suites (cli/verify) need dist/ built exactly once —
    // per-file beforeAll builds raced in CI (a child read router.js mid-emit).
    globalSetup: "./test/global-setup.ts",
  },
});
