// The live demo: <wordink-mic> with the local Moonshine engine, so the hosted docs need no key and
// no relay. When the docs run on localhost, a dev-key form lets you try the cloud providers (KTD3:
// dev keys work only on localhost, so the form is not rendered anywhere else).
import { type CloudProvider, isLocalHostname } from "@wordink/core";
import { createLocalProvider } from "@wordink/local";
import "@wordink/web";
import type { WordInkMic } from "@wordink/web";

const demo = document.getElementById("demo") as HTMLElement;
const mic = demo.querySelector("wordink-mic") as WordInkMic;
const status = demo.querySelector("#demo-status") as HTMLElement;
const progress = demo.querySelector("#demo-progress") as HTMLProgressElement;

const local = createLocalProvider();
mic.provider = local;

local.on("progress", (percent) => {
  progress.hidden = false;
  progress.value = percent;
  status.textContent = `Downloading the model… ${Math.round(percent)}%`;
});
local.on("ready", ({ device }) => {
  progress.hidden = true;
  status.textContent = `Model ready on ${device === "webgpu" ? "WebGPU" : "WebAssembly"}. Hold the button and speak.`;
});
local.on("error", (err) => {
  progress.hidden = true;
  status.textContent = `The model failed to load: ${err.message}`;
});
mic.addEventListener("wordink-start", () => {
  if (progress.hidden) status.textContent = "Listening… release to transcribe.";
});

if (isLocalHostname(location.hostname)) renderDevKeyForm();

function renderDevKeyForm(): void {
  const host = demo.querySelector("#demo-devkey") as HTMLElement;
  const details = document.createElement("details");
  details.className = "devkey";
  details.innerHTML = `
    <summary>Try a cloud provider with a dev key (localhost only)</summary>
    <p class="warn">The key stays in this tab and is sent straight to the provider. Anyone with access to a
    page holding a key can read it, which is why this form only appears on localhost.
    See <a href="dev-keys.html">Dev keys</a>.</p>
    <div class="devkey-row">
      <select aria-label="Provider">
        <option value="groq">Groq</option>
        <option value="openai">OpenAI</option>
        <option value="deepgram">Deepgram</option>
      </select>
      <input type="password" autocomplete="off" spellcheck="false" placeholder="Paste a provider key" aria-label="Provider key" />
      <button type="button" data-action="use">Use key</button>
      <button type="button" data-action="local">Back to local</button>
    </div>`;
  host.append(details);

  const select = details.querySelector("select") as HTMLSelectElement;
  const input = details.querySelector("input") as HTMLInputElement;
  details.querySelector('[data-action="use"]')?.addEventListener("click", () => {
    const key = input.value.trim();
    if (!key) return;
    mic.devKey = key;
    mic.provider = select.value as CloudProvider;
    status.textContent = `Using ${select.selectedOptions[0]?.text ?? select.value} with your dev key.`;
  });
  details.querySelector('[data-action="local"]')?.addEventListener("click", () => {
    mic.devKey = undefined;
    mic.provider = local;
    input.value = "";
    status.textContent = "Using the local engine.";
  });
}
