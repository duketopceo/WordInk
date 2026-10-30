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

// What the status line says while the demo is not mid-dictation. A press shows "Listening…" over it,
// and the end of that dictation restores it, so the line never stays stuck on "Listening…".
let restingStatus = status.textContent ?? "";
const rest = (text: string): void => {
  restingStatus = text;
  status.textContent = text;
};

const local = createLocalProvider();
mic.provider = local;

local.on("progress", (percent) => {
  progress.hidden = false;
  progress.value = percent;
  rest(`Downloading the model… ${Math.round(percent)}%`);
});
local.on("ready", ({ device }) => {
  progress.hidden = true;
  rest(`Model ready on ${device === "webgpu" ? "WebGPU" : "WebAssembly"}. Hold the button and speak.`);
});
local.on("error", (err) => {
  progress.hidden = true;
  rest(`The model failed to load: ${err.message}`);
});
mic.addEventListener("wordink-start", () => {
  if (progress.hidden) status.textContent = "Listening… release to transcribe.";
});
// The dictation is over (text delivered, or it failed): put the resting message back.
for (const type of ["wordink-transcript", "wordink-error"] as const) {
  mic.addEventListener(type, () => {
    if (progress.hidden) status.textContent = restingStatus;
  });
}

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
    rest(`Using ${select.selectedOptions[0]?.text ?? select.value} with your dev key.`);
  });
  details.querySelector('[data-action="local"]')?.addEventListener("click", () => {
    mic.devKey = undefined;
    mic.provider = local;
    input.value = "";
    rest("Using the local engine.");
  });
}
