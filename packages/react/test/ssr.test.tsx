// @vitest-environment node
import { renderToString } from "react-dom/server";
import { expect, it } from "vitest";

it("imports and server-renders without a DOM", async () => {
  expect(typeof window).toBe("undefined");
  const { WordInkMic, useDictation } = await import("../src/index.js");
  function Field() {
    const { state } = useDictation({ provider: "groq" });
    return (
      <p data-state={state}>
        <WordInkMic htmlFor="message" mode="toggle" endpoint="/api/wordink" />
      </p>
    );
  }
  const html = renderToString(<Field />);
  expect(html).toContain('data-state="idle"');
  expect(html).toContain('<wordink-mic for="message" mode="toggle" endpoint="/api/wordink">');
});
