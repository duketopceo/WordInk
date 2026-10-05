// KTD1: the core wasm must stay within 150 KB gzipped.
import { readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";

const BUDGET = 150 * 1024;
const file = new URL("../wasm/wordink_core_bg.wasm", import.meta.url);
const raw = readFileSync(file);
const gz = gzipSync(raw, { level: 9 }).length;
const kb = (n) => `${(n / 1024).toFixed(1)} KB`;
console.log(`wordink_core_bg.wasm: ${kb(raw.length)} raw, ${kb(gz)} gzip (budget ${kb(BUDGET)})`);
if (gz > BUDGET) {
  console.error("Core wasm is over the KTD1 gzip budget.");
  process.exit(1);
}
