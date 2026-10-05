// U8 end-to-end: the real Moonshine model in a real browser, through the HostProvider contract.
// Each test gets a fresh browser context, so its Cache API starts empty (a first visit).
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { type Page, expect, test } from "@playwright/test";

type Result = { type: "final"; text: string } | { type: "error"; code: string };
type Events = { progress: number[]; ready: { device: string }[]; error: string[] };

declare global {
  interface Window {
    local: { events: Events; load(): Promise<void>; dictate(): Promise<Result>; hasGpu(): boolean };
  }
}

async function open(page: Page, query = "") {
  await page.goto(`/e2e/index.html${query}`);
  await page.waitForFunction(() => window.local !== undefined);
}

const load = (page: Page) => page.evaluate(() => window.local.load());
const events = (page: Page) => page.evaluate(() => window.local.events);
const dictate = (page: Page) => page.evaluate(() => window.local.dictate());

function expectHelloWorld(result: Result) {
  expect(result.type).toBe("final");
  const text = result.type === "final" ? result.text : "";
  expect(text).toMatch(/hello/i);
  expect(text).toMatch(/world/i);
}

const withoutWebGPU = (page: Page) =>
  page.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
  });

test("without WebGPU it initializes on WASM and transcribes a hello world clip", async ({ page }) => {
  await withoutWebGPU(page);
  await open(page);
  expect(await page.evaluate(() => window.local.hasGpu())).toBe(false);

  const t0 = Date.now();
  await load(page);
  const loadMs = Date.now() - t0;
  expect((await events(page)).ready).toEqual([{ device: "wasm" }]);

  const t1 = Date.now();
  const result = await dictate(page);
  console.log(`[${test.info().project.name}] load ${loadMs} ms, transcribe ${Date.now() - t1} ms:`, result);
  expectHelloWorld(result);
});

test("first load reports progress 0 to 100; a second load is served from cache with no download", async ({
  playwright,
  browserName,
  baseURL,
}) => {
  // A fresh persistent context (still a first visit): Playwright's WebKit drops Cache Storage across a
  // reload in its default ephemeral context, which would fail the second-load check for a reason
  // unrelated to the provider.
  const context = await playwright[browserName].launchPersistentContext(mkdtempSync(join(tmpdir(), "wordink-cache-")), {
    ...(baseURL ? { baseURL } : {}),
  });
  const page = await context.newPage();
  await open(page);
  await load(page);
  const first = await events(page);
  expect(first.error).toEqual([]);
  expect(first.progress[0]).toBe(0);
  expect(first.progress.at(-1)).toBe(100);
  expect(first.progress.length).toBeGreaterThan(2);
  for (let i = 1; i < first.progress.length; i++) expect(first.progress[i]!).toBeGreaterThan(first.progress[i - 1]!);

  // Count model requests that reach the network on the second visit.
  const modelRequests: string[] = [];
  page.context().on("request", (r) => {
    if (r.url().includes("/hf/")) modelRequests.push(r.url());
  });
  await page.reload();
  await page.waitForFunction(() => window.local !== undefined);
  await load(page);
  const second = await events(page);
  expect(second.ready).toHaveLength(1);
  expect(second.progress).toEqual([]);
  expect(modelRequests).toEqual([]);
  await context.close();
});

test("AE4: after the model loads, dictation works with the network disconnected", async ({ page, context }) => {
  await open(page);
  await load(page);
  await context.setOffline(true);
  const online = await page.evaluate(() =>
    fetch("/e2e/index.html", { cache: "no-store" }).then(
      () => true,
      () => false,
    ),
  );
  expect(online).toBe(false);
  expectHelloWorld(await dictate(page));
});

test("a model file with the wrong checksum fails with ProviderDown and is not cached", async ({ page }) => {
  await withoutWebGPU(page);
  await open(page, "?badsum");
  const loadError = await page.evaluate(() => window.local.load().then(() => "", (e: Error) => e.message));
  expect(loadError).toMatch(/checksum mismatch for onnx\/encoder_model_quantized\.onnx/);
  expect((await events(page)).error[0]).toMatch(/checksum/);
  expect(await dictate(page)).toEqual({ type: "error", code: "ProviderDown" });
  const cached = await page.evaluate(async () =>
    (await (await caches.open("transformers-cache")).keys()).map((r) => r.url),
  );
  expect(cached.some((u) => u.includes("encoder_model"))).toBe(false);
});

test("the defaults load the pinned revision from huggingface.co", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "one real Hub download per run is enough");
  await withoutWebGPU(page);
  await open(page, "?hub");
  await load(page);
  expect((await events(page)).ready).toEqual([{ device: "wasm" }]);
  expectHelloWorld(await dictate(page));
});
