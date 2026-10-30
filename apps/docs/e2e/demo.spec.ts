// The built docs site (base /WordInk/, as on GitHub Pages): the live demo dictates with the local
// engine and no key, its wasm and worker assets resolve under the base path, and the dev-key form
// exists only on localhost (KTD3).
import { expect, test } from "@playwright/test";
import { DOCS_HOST, SITE_PORT, dictate, fakeThirdParties, fakeMicFromClip, holdToTalk, withoutWebGPU } from "./env.js";

test("on a public hostname the demo dictates with the local engine and offers no dev-key form", async ({ page, context }) => {
  const served = await fakeThirdParties(context);
  await withoutWebGPU(page);
  await fakeMicFromClip(page);
  const failed: string[] = [];
  page.on("response", (r) => {
    if (r.status() >= 400) failed.push(`${r.status()} ${r.url()}`);
  });
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const consoleErrors: string[] = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });

  await page.goto(`http://${DOCS_HOST}:${SITE_PORT}/WordInk/`);
  await expect(page.locator("#demo wordink-mic")).toHaveAttribute("data-state", "idle");
  await expect(page.locator("#demo-devkey")).toBeEmpty();
  expect(await page.locator("#demo-devkey input, .devkey").count()).toBe(0);

  const r = await dictate(page, "#demo-text", (t) => /hello/i.test(t) && /world/i.test(t));
  if (!r.text.trim()) {
    // If the capture ever flakes again (#40), the page state says where: holdPeak ~0 means the
    // worklet emitted silence, a peak with an error status means the model rejected the audio.
    const micState = await page.locator("#demo wordink-mic").getAttribute("data-state");
    const statusText = await page.locator("#demo-status").textContent();
    const micStatus = await page
      .locator("#demo wordink-mic")
      .locator('[part="status"]')
      .textContent()
      .catch(() => null);
    console.log(`[demo-flake-debug] micState=${micState} status=${JSON.stringify(statusText)} ` +
      `micStatus=${JSON.stringify(micStatus)} holdPeak=${r.peak} dictationState=${r.state} ` +
      `pageerrors=${JSON.stringify(errors)} console=${JSON.stringify(consoleErrors)}`);
  }
  expect(r.text.trim()).not.toBe("");
  await expect(page.locator("#demo-status")).toContainText("WebAssembly");
  expect(r.text).toMatch(/hello/i);
  expect(r.text).toMatch(/world/i);

  expect(failed).toEqual([]);
  expect(errors).toEqual([]);
  expect(served.model.some((p) => p.endsWith("decoder_model_merged_quantized.onnx"))).toBe(true);
});

// Regression: the status line used to be left on "Listening… release to transcribe." once a press
// happened after the model was ready (any second dictation, e.g. when the first try came back
// garbled on a slow runner), so the "Model ready on WebAssembly" message the first spec asserts
// was lost and master CI went red intermittently.
test("after each dictation the demo status returns to the model-ready message", async ({ page, context }) => {
  await fakeThirdParties(context);
  await withoutWebGPU(page);
  await fakeMicFromClip(page);
  await page.goto(`http://${DOCS_HOST}:${SITE_PORT}/WordInk/`);
  await expect(page.locator("#demo wordink-mic")).toHaveAttribute("data-state", "idle");
  const settled = () =>
    page.waitForFunction(
      () => {
        const s = document.querySelector("wordink-mic")?.getAttribute("data-state");
        return s === "idle" || s === "error";
      },
      undefined,
      { timeout: 60_000 },
    );
  for (let press = 0; press < 2; press++) {
    await holdToTalk(page, 3500);
    await settled();
    await expect(page.locator("#demo-status")).toContainText("WebAssembly");
    await expect(page.locator("#demo-status")).not.toContainText("Listening");
  }
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
  expect(links).toHaveLength(11);
  for (const href of links) {
    await page.goto(href);
    await expect(page.locator("main.content h1")).toHaveCount(1);
  }
  expect(failed).toEqual([]);
});
