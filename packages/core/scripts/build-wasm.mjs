// Builds the core wasm and its JS glue into packages/core/wasm/:
// cargo (wasm32, release) -> wasm-bindgen --target web -> wasm-opt -Oz.
// Requires the wasm32-unknown-unknown target, wasm-bindgen-cli matching the
// crate's pinned wasm-bindgen version, and binaryen's wasm-opt on PATH.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const pkg = dirname(dirname(fileURLToPath(import.meta.url)));
const root = join(pkg, "..", "..");
const out = join(pkg, "wasm");

const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit" });

const manifest = readFileSync(join(root, "crates/wordink-wasm/Cargo.toml"), "utf8");
const pinned = manifest.match(/wasm-bindgen = "=([\d.]+)"/)?.[1];
const cli = execFileSync("wasm-bindgen", ["--version"], { encoding: "utf8" }).trim().split(" ")[1];
if (pinned !== cli) {
  console.error(`wasm-bindgen CLI ${cli} does not match the crate's pinned ${pinned}.`);
  console.error(`Install it with: cargo install wasm-bindgen-cli --version ${pinned} --locked`);
  process.exit(1);
}

run("cargo", ["build", "-p", "wordink-wasm", "--target", "wasm32-unknown-unknown", "--release"]);
run("wasm-bindgen", [
  "--target",
  "web",
  "--out-dir",
  out,
  "--out-name",
  "wordink_core",
  join(root, "target/wasm32-unknown-unknown/release/wordink_wasm.wasm"),
]);
const wasm = join(out, "wordink_core_bg.wasm");
run("wasm-opt", ["-Oz", "--enable-bulk-memory", "--enable-nontrapping-float-to-int", "-o", wasm, wasm]);
