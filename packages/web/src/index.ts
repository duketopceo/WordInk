import { WordInkMic } from "./wordink-mic.js";

export {
  WordInkMic,
  type WordInkErrorDetail,
  type WordInkInterimDetail,
  type WordInkMicEventMap,
  type WordInkTranscriptDetail,
} from "./wordink-mic.js";
export { insertText, saveSelection, type SavedSelection } from "./insert.js";

// Importing the package registers the element (in browsers; a no-op where there is no DOM).
if (typeof customElements !== "undefined" && !customElements.get("wordink-mic")) {
  customElements.define("wordink-mic", WordInkMic);
}
