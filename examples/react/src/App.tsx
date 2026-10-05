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
    <main style={{ fontFamily: "system-ui, sans-serif", maxWidth: 560, margin: "3rem auto", padding: "0 1rem" }}>
      <h1>WordInk + React</h1>
      <label htmlFor="message">Message</label>
      <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
        <input
          id="message"
          value={text}
          onChange={(e) => setText(e.target.value)}
          style={{ flex: 1, padding: 8, fontSize: 16 }}
        />
        <button
          type="button"
          aria-pressed={state === "listening"}
          onPointerDown={() => void press()}
          onPointerUp={() => void release()}
          onPointerLeave={() => state === "listening" && void release()}
        >
          {state === "listening" ? "Release to stop" : "Hold to talk"}
        </button>
      </div>
      <meter min={0} max={1} value={level} style={{ width: "100%", marginTop: 8 }} />
      <p role="status">{state === "error" && error ? `${error.message} ${error.hint ?? ""}` : interim || STATUS[state]}</p>
    </main>
  );
}
