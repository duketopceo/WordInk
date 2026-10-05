// Insertion with native undo (R2, KTD7) needs a real browser: the full element, worklet and wasm core
// run against the fake mic, and only the Groq relay is mocked.
import { expect, test } from "@playwright/test";
import { fakeMic, waitForAudio } from "./fake-mic.js";

test.beforeEach(async ({ page, browserName }) => {
  await fakeMic(page, browserName);
  await page.route("**/relay/groq/transcriptions", (route) =>
    route.fulfill({ status: 200, contentType: "text/plain", body: "there\n" }),
  );
  await page.goto("/packages/web/e2e/insert.html");
  await page.waitForFunction(() => customElements.get("wordink-mic") !== undefined);
});

test("a final transcript lands at the caret and undo restores the original", async ({ page }) => {
  const field = page.locator("#field");
  const mic = page.locator("wordink-mic");
  const button = mic.locator('[part="button"]');
  await field.fill("Hello world");
  await field.evaluate((el: HTMLTextAreaElement) => el.setSelectionRange(6, 6)); // "Hello |world"

  await button.click();
  await expect(mic).toHaveAttribute("data-state", "listening");
  await expect(button).toHaveAttribute("aria-pressed", "true");
  await waitForAudio(page);
  await page.waitForTimeout(1200);
  await button.click();

  await expect(field).toHaveValue("Hello there world");
  await expect(mic).toHaveAttribute("data-state", "idle");
  await expect(field).toBeFocused();
  expect(await field.evaluate((el: HTMLTextAreaElement) => el.selectionStart)).toBe("Hello there ".length);

  await page.keyboard.press("ControlOrMeta+z");
  await expect(field).toHaveValue("Hello world");
});
