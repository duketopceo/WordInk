---
"@wordink/core": patch
---

OpenAI realtime now sends the GA transcription `session.update` (matching the GA client secrets the relay mints) and defaults to `gpt-live-transcribe`. A transcript that completes while the button is still held no longer leaves the session stuck in Transcribing: OpenAI keeps the socket open and the commit on release produces the final, and a streaming host provider that reports its final early completes on release. Recordings now stop on their own at 60 s and transcribe what was captured, keeping Groq uploads under the relay's 2 MB body cap.
