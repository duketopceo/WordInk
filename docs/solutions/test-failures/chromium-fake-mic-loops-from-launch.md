---
title: Chromium's file-backed fake mic loops from browser launch, making speech e2e timing-dependent
module: apps/docs e2e
date: 2026-10-05
problem_type: test_failure
component: testing
severity: medium
symptoms:
  - "Playwright docs e2e intermittently fails: textarea stays empty after hold-to-talk (180 s timeout)"
  - "wordink-error NoSpeech after ~2 s of transcribing with the local Moonshine provider"
  - "Failure appeared right after unrelated fixes and looked like a regression, but also failed on the pre-fix base"
root_cause: test_isolation
resolution_type: test_fix
framework_version: "Playwright 1.63 Chromium (new headless), transformers.js 4.3 Moonshine-tiny"
related_components:
  - "@wordink/local"
  - "@wordink/web"
tags:
  - playwright
  - chromium
  - fake-media
  - getusermedia
  - flaky-test
  - moonshine
  - speech-e2e
---

# Chromium's file-backed fake mic loops from browser launch, making speech e2e timing-dependent

## Problem

The docs site's end-to-end tests dictate a real "hello world" clip through Chromium's fake microphone into the local Moonshine engine. They began failing (2 of 4) on PR #26 and passed on a rerun in a different order, so they looked like a regression from that PR's fixes. They were not.

## Symptoms

- After `holdToTalk(page, 3500)`, `#message` / `#demo-text` stayed empty until the 180 s assertion timeout.
- Instrumenting the page showed `listening → transcribing → wordink-error NoSpeech` about 2 s after release: the model returned an empty transcript, which the core reports as `NoSpeech` (`crates/wordink-core/src/session.rs:216`).
- A near-identical diagnostic spec passed while the real spec failed, because the two took different amounts of setup time before pressing.

## What Didn't Work

- **Bisecting the PR's fix commits by suspicion.** The suspects (the window-blur hold release, the transform-press guard, the new watchdogs, the wasm-load retry) were reasonable. But none of them was the cause, and every failing run cost a 3-minute timeout.
- **Comparing against a hand-written diagnostic spec.** It passed, which pointed away from the harness until the real spec was instrumented directly.

## Solution

`--use-file-for-fake-audio-capture=<wav>` makes Chromium **loop the file starting when the browser launches**, not when the page calls `getUserMedia`. Where a 3.5 s hold lands in the loop depends on everything that ran before the press: page load, model fetch, other assertions. A hold that straddles the loop restart (`…world. Hel`) gives Moonshine-tiny fragments it transcribes to nothing.

The fix stubs `getUserMedia` per page so each mic open plays the padded clip **once, from its start** (`apps/docs/e2e/env.ts:125`, `fakeMicFromClip`). Specs call it before navigating (`apps/docs/e2e/demo.spec.ts:10`, `apps/docs/e2e/quickstart.spec.ts:37`):

```ts
await page.addInitScript(({ pcm, rate }) => {
  const samples = new Int16Array(Uint8Array.from(atob(pcm), (c) => c.charCodeAt(0)).buffer);
  navigator.mediaDevices.getUserMedia = async () => {
    const ctx = new AudioContext();
    await ctx.resume();
    const buffer = ctx.createBuffer(1, samples.length, rate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < samples.length; i++) data[i] = samples[i]! / 32768;
    const source = ctx.createBufferSource();
    source.buffer = buffer;
    const dest = ctx.createMediaStreamDestination();
    source.connect(dest);
    source.start(); // once, from sample 0, at the moment the page opens the mic
    return dest.stream;
  };
}, { pcm, rate });
```

The `--use-file-for-fake-audio-capture` flag was removed from the docs Playwright config. After the change the suite passed 4/4 on three consecutive runs, about 8 s each instead of timing out.

## Why This Works

The captured audio is now a function of the press alone. The hold always contains the clip's leading silence, the full utterance, and trailing silence, whatever happened before. The real capture path after `getUserMedia` still runs: AudioWorklet, resampling and wasm. Real-device capture stays covered by the `packages/core` and `packages/web` e2e suites, which use the browser fake device.

## Prevention

- **Content-sensitive audio assertions must not use Chromium's looping file capture.** That means anything that checks what was *said*. Use a per-open `getUserMedia` stub like `fakeMicFromClip` instead. `packages/core` and `packages/web` still use `--use-file-for-fake-audio-capture` (`packages/core/playwright.config.ts:34`, `packages/web/playwright.config.ts:34`). That is safe only because they assert WAV shape and insertion, not transcript content. Switch them before adding transcript assertions there.
- **When a "regression" appears after unrelated changes, run the failing spec on the base branch first** (`git worktree add <dir> <base>`), before bisecting suspects. Here the base failed the same way, which ruled out the PR in one step.
- **Instrument the real failing spec, not a copy.** A `MutationObserver` on `data-state` plus `wordink-*` event logging (via `page.addInitScript`, with console messages forwarded) showed `NoSpeech` straight away. That narrowed the problem to "the model heard nothing usable".
