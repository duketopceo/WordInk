import { WordInkMic } from "@wordink/react";
import { useState } from "react";

export function App() {
  const [text, setText] = useState("");

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
        <WordInkMic
          htmlFor="message"
          provider="groq"
          devKey={import.meta.env.VITE_GROQ_KEY}
          onTranscript={(t) => setText((prev) => (prev && !/\s$/.test(prev) ? `${prev} ${t}` : prev + t))}
        />
        <span className="demo-hint">Hold the mic to dictate into the field.</span>
      </div>
    </main>
  );
}
