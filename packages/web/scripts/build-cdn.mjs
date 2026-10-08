// CDN build (AE1): one ES module with @wordink/core's JS bundled in, plus the core wasm copied next
// to it. The wasm glue loads `new URL("wordink_core_bg.wasm", import.meta.url)`, which esbuild
// leaves as is, so the wasm is fetched from beside the bundle rather than base64-inlined.
// Then enforces the CDN JavaScript budget: 40 KB gzipped.
import { copyFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { build } from "esbuild";

const pkg = dirname(dirname(fileURLToPath(import.meta.url)));
const out = join(pkg, "dist", "wordink-web.cdn.js");
const BUDGET = 40 * 1024;

await build({
  entryPoints: [join(pkg, "src", "index.ts")],
  outfile: out,
  bundle: true,
  format: "esm",
  target: "es2022",
  minify: true,
  legalComments: "none",
  logLevel: "warning",
});

const core = dirname(fileURLToPath(import.meta.resolve("@wordink/core")));
copyFileSync(join(core, "..", "wasm", "wordink_core_bg.wasm"), join(pkg, "dist", "wordink_core_bg.wasm"));

const js = readFileSync(out);
if (!js.includes("wordink_core_bg.wasm")) {
  console.error("CDN bundle does not reference wordink_core_bg.wasm via import.meta.url.");
  process.exit(1);
}
const gz = gzipSync(js, { level: 9 }).length;
const kb = (n) => `${(n / 1024).toFixed(1)} KB`;
console.log(`wordink-web.cdn.js: ${kb(js.length)} raw, ${kb(gz)} gzip (budget ${kb(BUDGET)})`);
if (gz > BUDGET) {
  console.error("CDN bundle is over its 40 KB gzip budget.");
  process.exit(1);
}
