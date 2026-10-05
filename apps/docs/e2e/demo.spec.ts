// The built docs site (base /WordInk/, as on GitHub Pages): the live demo dictates with the local
// engine and no key, its wasm and worker assets resolve under the base path, and the dev-key form
// exists only on localhost (KTD3).
import { expect, test } from "@playwright/test";
import { DOCS_HOST, SITE_PORT, fakeThirdParties, holdToTalk, withoutWebGPU } from "./env.js";

test("on a public hostname the demo dictates with the local engine and offers no dev-key form", async ({ page, context }) => {
  const served = await fakeThirdParties(context);
  await withoutWebGPU(page);
  const failed: string[] = [];
  page.on("response", (r) => {
    if (r.status() >= 400) failed.push(`${r.status()} ${r.url()}`);
  });
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));

  await page.goto(`http://${DOCS_HOST}:${SITE_PORT}/WordInk/`);
  await expect(page.locator("#demo wordink-mic")).toHaveAttribute("data-state", "idle");
  await expect(page.locator("#demo-devkey")).toBeEmpty();
  expect(await page.locator("#demo-devkey input, .devkey").count()).toBe(0);

  await holdToTalk(page, 3500);
  await expect(page.locator("#demo-text")).not.toHaveValue("");
  await expect(page.locator("#demo-status")).toContainText("WebAssembly");
  const text = await page.locator("#demo-text").inputValue();
  expect(text).toMatch(/hello/i);
  expect(text).toMatch(/world/i);

  expect(failed).toEqual([]);
  expect(errors).toEqual([]);
  expect(served.model.some((p) => p.endsWith("decoder_model_merged_quantized.onnx"))).toBe(true);
});

test("on localhost the demo renders the dev-key form, and every docs page loads", async ({ page }) => {
  const failed: string[] = [];
  page.on("response", (r) => {
    if (r.status() >= 400) failed.push(`${r.status()} ${r.url()}`);
  });
  await page.goto(`http://127.0.0.1:${SITE_PORT}/WordInk/`);
  await expect(page.locator("#demo-devkey details.devkey")).toHaveCount(1);
  await expect(page.locator('#demo-devkey input[type="password"]')).toHaveCount(1);

  const links = await page.locator("nav.side a").evaluateAll((as) => as.map((a) => (a as HTMLAnchorElement).href));
  expect(links).toHaveLength(9);
  for (const href of links) {
    await page.goto(href);
    await expect(page.locator("main.content h1")).toHaveCount(1);
  }
  expect(failed).toEqual([]);
});
