// The five-minute check (Success Criteria, U9): follow the plain HTML quickstart's local-engine path
// literally, from a clean directory, using the exact snippets the docs page renders. The CDN script
// tag points at the real jsDelivr URL with its SRI hash; the route serves the local build in its place.
import { execSync } from "node:child_process";
import { mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { release, snippet } from "../site.js";
import { DOCS_DIR, QUICKSTART_DIR, QUICKSTART_PORT, fakeThirdParties, holdToTalk, withoutWebGPU } from "./env.js";

const PAGE = `http://127.0.0.1:${QUICKSTART_PORT}/`;

function cleanDir(): void {
  rmSync(QUICKSTART_DIR, { recursive: true, force: true });
  mkdirSync(QUICKSTART_DIR, { recursive: true });
}

test("the HTML quickstart with the local engine dictates into the textarea in under five minutes", async ({ page, context }) => {
  const t0 = Date.now();
  const rel = await release({ checkCdn: false });
  cleanDir();

  // Step 1, from the docs: install, then bundle the engine's two files into ./wordink-local/.
  const [install, ...commands] = snippet("vendor-local.sh", rel).split("\n");
  expect(install).toMatch(/^npm install @wordink\/local@\S+ esbuild$/);
  // The packages are not on npm yet, so `npm install` is simulated by linking the workspace install.
  symlinkSync(join(DOCS_DIR, "node_modules"), join(QUICKSTART_DIR, "node_modules"), "dir");
  for (const command of commands) execSync(command, { cwd: QUICKSTART_DIR, stdio: "pipe" });

  // Step 2: the page, verbatim.
  writeFileSync(join(QUICKSTART_DIR, "index.html"), snippet("quickstart-local.html", rel));
  const setupMs = Date.now() - t0;

  // Step 3: open it and dictate.
  const served = await fakeThirdParties(context);
  await withoutWebGPU(page);
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(PAGE);
  await expect(page.locator("wordink-mic")).toHaveAttribute("data-state", "idle");
  await holdToTalk(page, 3500);
  await expect(page.locator("#message")).not.toHaveValue("");
  const totalMs = Date.now() - t0;

  const text = await page.locator("#message").inputValue();
  console.log(
    `[quickstart] setup ${(setupMs / 1000).toFixed(1)} s, first dictation ${((totalMs - setupMs) / 1000).toFixed(1)} s, ` +
      `total ${(totalMs / 1000).toFixed(1)} s: ${JSON.stringify(text)}`,
  );
  expect(text).toMatch(/hello/i);
  expect(text).toMatch(/world/i);
  expect(errors).toEqual([]);
  // The element came from "the CDN" (integrity-checked), and its wasm from beside it.
  expect(served.cdn).toEqual(["wordink-web.cdn.js", "wordink_core_bg.wasm"]);
  expect(served.model.some((p) => p.endsWith("encoder_model_quantized.onnx"))).toBe(true);
  expect(totalMs).toBeLessThan(5 * 60_000);
});

test("a wrong integrity hash stops the CDN script from running", async ({ page, context }) => {
  const rel = await release({ checkCdn: false });
  cleanDir();
  writeFileSync(join(QUICKSTART_DIR, "index.html"), snippet("quickstart.html", { ...rel, webSri: `sha384-${"A".repeat(64)}` }));
  await fakeThirdParties(context);
  const blocked = page.waitForEvent("console", (m) => m.type() === "error" && /integrity/i.test(m.text()));
  await page.goto(PAGE);
  await blocked;
  expect(await page.evaluate(() => customElements.get("wordink-mic") === undefined)).toBe(true);
});
