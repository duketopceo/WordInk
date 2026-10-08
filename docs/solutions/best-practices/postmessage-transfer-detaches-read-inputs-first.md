---
title: Worker postMessage transfers detach the sender's buffer — read what you need before posting, and fakes must emulate it
date: 2026-10-06
category: best-practices
module: "@wordink/local"
problem_type: best_practice
component: test_infrastructure
severity: medium
applies_when:
  - "Calling worker.postMessage(msg, [buffer]) with a transfer list"
  - "Reading anything off a message or its buffers after postMessage"
  - "Writing a fake Worker/Channel for tests that must behave like the platform"
related_components:
  - "@wordink/local transcribe worker"
  - "FakeWorker in local.test.ts"
tags:
  - structured-clone
  - transferable
  - worker
  - test-fakes
  - semantic-emulation
---

# postMessage transfer detaches — read inputs first, and fakes must emulate it

## Context

`w.postMessage(msg, [audio.buffer])` with a transfer list **detaches** the sender's `ArrayBuffer`: on the next line, `audio.length` is `0` and iterating the view throws. The receiving side gets a live copy; the sender's is dead.

A timeout-scaling fix in `@wordink/local` computed `inferenceTimeoutMs(audio.length)` **after** the `postMessage` line. Production always read `0` — the change was dead code that kept the flat floor it was meant to fix. The unit suite could not see it: `FakeWorker.postMessage` just pushed `msg` to an array, so the buffer stayed alive and the test passed. Review caught it before merge.

## Guideline

1. **Capture everything you need off a message before `postMessage` transfers it.** Sizes, deadlines, checksums — read them first; the transfer list is a moving-out boundary.
2. **A fake must emulate the platform semantics the code under test relies on.** For a Worker fake: the worker receives an intact copy (structured clone), the caller's transferred buffers detach. Emulate both halves or production-vs-test behavior silently diverges — the failure mode is a passing test over dead code.

## Example

```ts
// Wrong: reads 0 in production — the buffer detached on the line above.
w.postMessage(msg, [audio.buffer]);
const timeoutMs = inferenceTimeoutMs(audio.length);

// Right: read before the transfer boundary.
const timeoutMs = inferenceTimeoutMs(audio.length);
w.postMessage(msg, [audio.buffer]);
```

Fake worker emulation (`ArrayBuffer.prototype.transfer`, ES2024/Node 20+):

```ts
postMessage(msg: ToWorker, transfer?: Transferable[]) {
  // Worker side gets an intact copy; the caller's transferred buffers detach.
  const received = msg.type === "transcribe" ? { ...msg, audio: msg.audio.slice() } : msg;
  for (const t of transfer ?? []) (t as ArrayBuffer & { transfer?(): ArrayBuffer }).transfer?.();
  this.sent.push(received);
}
```

The same rule applies to any fake standing in for a transfer boundary: `MessageChannel`, `BroadcastChannel`, `Atomics`-backed buffers, stream transfers. Emulate what the platform does to the *sender's* object, not just what the receiver sees.
