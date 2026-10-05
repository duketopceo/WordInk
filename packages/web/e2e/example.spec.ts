// AE1: the plain HTML example, served as static files with the CDN bundle loaded by a relative path
// (no build step on the page), dictates into its textarea. Fake mic; the relay is mocked.
import { expect, test } from "@playwright/test";
import { fakeMic, waitForAudio } from "./fake-mic.js";

test("examples/html dictates into its textarea with push-to-talk", async ({ page, browserName }) => {
  await fakeMic(page, browserName);
  let relayed = 0;
  await page.route("**/api/wordink/groq/transcriptions", (route) => {
    relayed++;
    return route.fulfill({ status: 200, contentType: "text/plain", body: "hello from the relay\n" });
  });
  const wasm: string[] = [];
  page.on("request", (r) => {
    if (r.url().endsWith(".wasm")) wasm.push(new URL(r.url()).pathname);
  });

  await page.goto("/examples/html/index.html");
  const mic = page.locator("wordink-mic");
  await expect(mic).toHaveAttribute("data-state", "idle");

  // Hold to talk: press, speak, release.
  await mic.locator('[part="button"]').hover();
  await page.mouse.down();
  await expect(mic).toHaveAttribute("data-state", "listening");
  await waitForAudio(page);
  await page.waitForTimeout(1200);
  await page.mouse.up();

  await expect(page.locator("#message")).toHaveValue("hello from the relay");
  await expect(mic).toHaveAttribute("data-state", "idle");
  expect(relayed).toBe(1);
  // The core wasm came from beside the CDN bundle (import.meta.url), not inlined.
  expect(wasm).toEqual(["/packages/web/dist/wordink_core_bg.wasm"]);
});
