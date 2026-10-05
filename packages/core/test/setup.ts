// Load the real core wasm synchronously so the host's `init()` resolves without a fetch in Node.
import { readFileSync } from "node:fs";
import { afterEach, vi } from "vitest";
import { initSync } from "../wasm/wordink_core.js";

initSync({ module: readFileSync(new URL("../wasm/wordink_core_bg.wasm", import.meta.url)) });

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
