import { useDictation } from "@wordink/react";
import { useState } from "react";

const STATUS = {
  idle: "Hold the button and speak",
  "requesting-mic": "Starting microphone…",
  listening: "Listening…",
  transcribing: "Transcribing…",
  error: "",
} as const;

export function App() {
  const [text, setText] = useState("");
  const { state, level, interim, error, press, release } = useDictation({
    provider: "groq",
    // Localhost only: the key is sent from the page. Use `endpoint` and @wordink/server in production.
    devKey: import.meta.env.VITE_GROQ_KEY,
    onTranscript: (t) => setText((prev) => (prev && !/\s$/.test(prev) ? `${prev} ${t}` : prev + t)),
  });

  return (
    <main className="demo-shell">
      <h1>WordInk + React</h1>
      <label htmlFor="message">Message</label>
      <textarea
        id="message"
        className="demo-field"
        rows={5}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="Hold the mic and speak…"
      />
      <div className="demo-row">
        <button
          type="button"
          className="demo-mic"
          aria-pressed={state === "listening" || state === "requesting-mic"}
          onPointerDown={() => void press()}
          onPointerUp={() => void release()}
          onPointerLeave={() => state === "listening" && void release()}
        >
          {state === "listening" ? "Release to stop" : "Hold to talk"}
        </button>
        <span className="demo-hint">Uses the `useDictation` hook with Pen and Signal demo styles.</span>
      </div>
      <meter className="demo-meter" min={0} max={1} value={level} />
      <p role="status" className="demo-status">
        {state === "error" && error ? `${error.message} ${error.hint ?? ""}` : interim || STATUS[state]}
      </p>
    </main>
  );
}
