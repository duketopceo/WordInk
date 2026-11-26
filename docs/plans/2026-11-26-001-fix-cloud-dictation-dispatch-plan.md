---
title: "Restore Cloud Dictation Dispatch After the Hex v3.4.0 Sync - Plan"
type: fix
date: 2026-11-26
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
---

# Restore Cloud Dictation Dispatch After the Hex v3.4.0 Sync - Plan

## Goal Capsule

- **Objective:** WordInk desktop once again honors the `engine`, `local_cloud_fallback`, `meeting_engine`, and clip-retention settings: dictation routes to a cloud provider when configured, a failed local dictation retries once on the configured fallback, meeting chunks use the meeting engine, and Linux can run dictation with no local model installed when a cloud route exists.
- **Means:** One PR off `master` that re-applies the WordInk-only dispatch the Hex v3.4.0 sync (`7c2775e`, #176) overwrote in upstream-shared files, onto the new upstream structure (parked worker, `padded_whole_clip`).
- **Authority:** Product Contract wins on behavior; a KTD wins on mechanism within it; units override neither. The owner merges; agents never merge.
- **Execution profile:** `code`. Rust in `apps/desktop`. Test-first where the sync deleted the coverage (engine routing, fallback retry). Linux-only code (`#[cfg(target_os = "linux")]`) cannot compile on this Mac — the Linux legs of `desktop.yml` CI are the gate, not a workaround.
- **Stop conditions:** Stop and report if a unit requires live provider calls to verify, or if upstream code has diverged so far that re-applying the pre-sync shape would fight the new structure instead of following it.
- **Finishing:** The implementer opens the PR with green CI; the owner reviews and merges.

---

## Product Contract

### Summary

The Hex v3.4.0 sync overwrote the WordInk dispatch surfaces. It left the entire `transcription/cloud.rs` stack — providers, routes, keys, request log, usage, `run`/`run_meeting`, test hooks — intact but unreachable: `route()` is `dead_code`, `ENGINE_CHOICE` is stuck at `Auto`, `set_local_fallback`/`set_meeting_engine`/`set_clip_retention` have no callers, and no dictation path can reach `cloud::run`. The visible result: picking a cloud engine in onboarding, enabling fallback, or setting a meeting engine silently does nothing.

### Problem Frame

The regression has six broken surfaces, all removed by one commit:

1. **Settings propagation** — `AppSettings::apply_runtime` no longer calls `cloud::set_engine`, `set_local_fallback`, `set_meeting_engine`, or `set_clip_retention`. Every settings-driven cloud static is frozen at its default.
2. **Engine dispatch** — `transcription/engine.rs` lost `cloud_only`, the per-capture `routed` decision, `begin_routed`/`streams_routed`, `finish_routed`/`finish_cloud`, `CloudFallback`, and `PendingTranscript.fallback`. `finish`/`finish_with` always go to the local model.
3. **Linux listener startup** — `linux/dictation.rs` lost `cloud::refresh_key()` (the one blocking keyring warm allowed on the dictation worker), the `cloud_only` engine start, the `ModelSwap::unloaded()` constructor, and the cloud-route gate on `model.want`.
4. **Usage plumbing** — the output paths in `linux/dictation.rs` and `windows/dictation.rs` lost `attribute()` and `draft.usage`, so a cloud-produced `Transcribed.usage`/`served` never reaches History.
5. **Linux model gate** — `linux/app.rs` lost `skips_model_install_check`, so the app demands a local model even when dictation would run entirely on a cloud engine (meetings and captures still legitimately require one).
6. **`local_cloud_fallback` setting** — dead without the `PendingTranscript.fallback` retry arm from surface 2.

Because the dispatch lived in shared `engine.rs`, the same restore re-enables routing on all three OSes exactly as before the sync: `finish` on Linux/Windows, `finish_with` (called by `parakeet.rs`) on macOS.

### Requirements

- R1. `settings.engine` routes the next capture: `Local` → local model; `Auto` → Groq when a cached key resolves; a named engine → that provider. A named engine routes without needing a warm key cache (the run surfaces the missing key itself).
- R2. With `local_cloud_fallback` set, a local dictation that errors or times out retries the same clip once on the configured provider; the result carries `Source::Cloud` and that provider's `UsageRecord`.
- R3. On Linux, when a cloud route exists and no prepared transcriber arrives (no local model installed), the listener starts a model-free engine and the app does not block on "Model required" — unless meetings or capture are enabled. Capture genuinely needs the local model (`foreground()` stays local per KTD4); meetings only need it when `meeting_route()` is `None`, but the restored gate checks `meetings.enabled` regardless — a parity simplification kept deliberately (see Assumptions).
- R4. `meeting_engine` routes meeting chunks to the configured provider again (`meeting/transcript.rs` already calls `meeting_route()`; it must stop reading an empty static).
- R5. `history_retention` propagates to `set_clip_retention`, so failed-clip retention honors the setting.
- R6. Runtime settings changes propagate without a restart: the next capture after an engine change re-decides its route.
- R7. On Linux and Windows, cloud dictation results record usage (`UsageRecord`) so History/Usage stays truthful for dictation, not only meetings. This requires restoring the output-path plumbing (`attribute()`/`draft.usage`) the sync also removed — `finish_cloud` producing `usage` is not sufficient when no caller consumes it. macOS is out of scope: `parakeet.rs::record_history` hardcoded `usage: None` even pre-sync, so dictation usage recording never existed there (see Open Questions).

### Scope Boundaries

- **In scope:** restoring pre-sync behavior on all three OSes via the shared engine; Linux cloud-only startup; the four settings hooks; the Linux/Windows usage→history plumbing; tests covering the restored seams; feature-map and AGENTS.md notes.
- **Out of scope:** the `wordink-dictd` Python daemon (a separate, working path); `foreground()`/local-API host audio (stays local, deliberately); the ~9s Groq-overhead investigation (a separate roadmap unit — with dispatch restored, dictation rows will finally appear in `cloud-requests.jsonl` to measure); any redesign of route semantics; macOS dictation usage recording (never existed pre-sync).
- **Deferred question (non-blocking):** whether Windows/macOS should also warm the key cache at listener start, as Linux does. Pre-sync parity warms via `set_engine → refresh_key_in_background` on settings apply; keep parity unless implementation surfaces a cold-start gap.

### Open Questions

All deferred, none blocking:

- macOS dictation never wrote a `UsageRecord` (`parakeet.rs::record_history` hardcodes `usage: None`, unchanged pre-sync). R7 restores the Linux/Windows plumbing only; a beyond-parity macOS unit plumbing `transcribed.usage` through `InferenceJob → CompletedTranscript → record_history` is future work.
- Windows has no `start_without_model` path and its app still demands a local model at enable — a Windows user with a Groq key and no model stays blocked. Parity asymmetry; extending cloud-only start to Windows is a separate decision.
- On a cloud-only Linux listener, `engine.share()` exposes a model-free engine to the local API — `/transcribe` host audio gets a graceful error, not silence. Whether to hide that surface is deferred.
- `linux/app.rs` evaluates `route()` before the listener's blocking `refresh_key()` runs; on `Auto` a cold key cache can transiently show "Model required" until the background warm lands. Parity behavior.
- The `routed` mutex can strand a stale route if a `begin` is followed by a capture that never reaches `finish` — the next `finish` consumes the old decision. Pre-sync shape; unexamined against the new `start_parked`/`capture_host` flows.

---

## Planning Contract

### Key Technical Decisions

- KTD1. Re-apply the pre-sync dispatch design — route decided once at `begin`, carried in `routed`, consumed by `finish_routed` — rather than designing a new dispatch. (session-settled: user-approved — chosen over redesigning the route decision: the pre-sync shape already solved stream suppression, deadline bounding, and fallback attachment, and its test hooks `ROUTE_OVERRIDE`/`FALLBACK_OVERRIDE` still exist in `cloud.rs`.)
- KTD2. `cloud_only` takes precedence over the new upstream `start_parked` idle-unload path. `start_without_model` produces a worker with `model: None` and `cloud_only: true`, which makes `route_for_engine` force a cloud provider even when the key cache is cold — a parked engine still *can* load a model, so it is not `cloud_only` and must not get that treatment.
- KTD3. Restore routing in the shared engine rather than per-OS shims. `finish` (Linux/Windows) and `finish_with` (all OSes; `parakeet.rs` on macOS) funnel through `finish_routed`, matching the pre-sync surface — a per-OS shim would fork behavior the sync proved fragile.
- KTD4. `foreground()` (local-API and capture host audio) does not route to cloud. Pre-sync it bypassed `finish_routed` and stays that way; routing host audio is a policy change, not a restore.

### High-Level Technical Design

Pre-sync shape (from `7c2775e^:apps/desktop/src/transcription/engine.rs`) re-applied to the current worker:

```text
begin() ──decide()──> routed = route_with(cloud_only)   [once per capture]
                        │
finish/finish_with ──take_route()──┬─ Some(provider) ──> finish_cloud()
                                   │                    (dedicated thread,
                                   │                     Source::Cloud, usage)
                                   └─ None ──> Command::Finish + optional
                                               CloudFallback (clip clone +
                                               provider + run closure)
PendingTranscript::wait ──local Err──> CloudFallback retry once (if set)
```

Surfaces the sync removed and this plan re-adds, all in upstream-shared files:

- `app_settings.rs` `apply_runtime`: four `cloud::set_*` calls.
- `engine.rs`: `cloud_only`/`routed` fields, `decide`/`take_route`, `begin` route storage, `begin_routed`/`streams_routed`, `streams()` consults route, `finish_routed`, `finish_cloud`, `CloudRun`/`CloudRunner` types, `PendingTranscript.fallback` + `wait` retry, `start_with(Option<Model>)` + `#[cfg(linux)] start_without_model`.
- `linux/dictation.rs`: `cloud::refresh_key()` at listener start; `cloud_only` decision choosing `start_without_model` vs `start_parked` vs `start`; `ModelSwap::unloaded()` for the model-free listener (still present, dead-code-gated, at `local_transcriber.rs`); `route_with(cloud_only)` gate on `model.want`; the `attribute()`/`deliver`/`draft.usage` usage plumbing in the output worker.
- `windows/dictation.rs`: the `transcribed.usage → draft.usage` plumbing (pre-sync ~lines 816–868).
- `linux/app.rs`: `skips_model_install_check(route().is_some(), meetings.enabled, capture.enabled)` around the "Model required" gate.

### Assumptions

- `cloud.rs` needs no changes — verified: `route`, `route_with`, `route_for`, `route_for_engine`, `fallback_provider`, `language_hint`, `refresh_key`, `cached_key`, `run`, `Provider::streams`, `ModelSwap::unloaded`, and both test overrides are all present; `Provider::streams()` is `false` for every variant ("cloud engines take the whole clip; none of them stream"), so the `streams_routed` suppression arm is exercised but the streaming arm is currently vacuous.
- `Worker.model` is already `Option<Model>` in the current file, so `start_with(None)` fits the existing worker shape.
- `Source::Cloud`, `Transcribed.usage`, `Transcribed.served` survived the sync and remain the right result carriers.
- macOS gets routing through `parakeet.rs`'s existing `finish_with` call with no macOS-specific changes — the same as pre-sync.

**Accepted parity quirks** (restored verbatim, recorded so review doesn't re-litigate them):

- The fallback retry fires on *any* local `Err`, including "dictation was cancelled" and empty-audio errors — cancellation is not a transcription failure, but the pre-sync retry arm did not distinguish error classes.
- `streams()` answers `streams_routed(decide())`, so a cloud-routed capture suppresses the streaming-capability answer for local-API yielding streams too, even though host audio runs on the local model.
- The model-install gate treats `meetings.enabled` as model-requiring even when `meeting_route()` is cloud — a conservative parity simplification (see R3).

---

## Implementation Units

### U1. Settings → cloud propagation in `apply_runtime`

**Goal:** The four cloud statics track settings again, so every downstream reader (`route`, `meeting_route`, `fallback_provider`, clip retention) sees live values.

**Requirements:** R4, R5, R6

**Dependencies:** none

**Files:**

- `apps/desktop/src/app_settings.rs` (modify `apply_runtime`)

**Approach:**

1. In `apply_runtime`, re-add `crate::transcription::cloud::set_engine(self.engine, self.engine_model.as_deref(), self.gateway_url.as_deref())`, `set_local_fallback(self.local_cloud_fallback)`, `set_meeting_engine(self.meeting_engine, self.meeting_engine_model.as_deref())`, and `set_clip_retention(self.history_retention)` — same call sites as `7c2775e^:apps/desktop/src/app_settings.rs` (~line 2459).

**Patterns to follow:** the surrounding `set_device`/`set_enabled` calls in the same function; the pre-sync hunk verbatim.

**Test scenarios:**

- Applying settings with `engine = Groq` then `route()`/`route_for_choice` resolves the Groq provider (assert through the public route accessors — `engine_choice()` is private; `ROUTE_OVERRIDE` belongs to engine tests, not here).
- Applying `meeting_engine = Some(Openrouter)` then `meeting_route()` returns `Some(Provider::OpenRouter)` (existing `meeting_route` tests show the seam).
- Applying `local_cloud_fallback = Groq` makes `fallback_provider()` return `Some(Provider::Groq)`.
- Applying `history_retention` propagates to clip retention (match the pre-sync coverage if present in `control/settings.rs` tests — `the_local_cloud_fallback_is_off_by_default_and_set_from_the_cli` may already exist; extend rather than duplicate).

**Verification:** `cargo test` on the app_settings/cloud settings modules; `clippy` clean.

---

### U2. Engine dispatch: routed finish + cloud fallback in `engine.rs`

**Goal:** The shared engine routes captures to a cloud provider per settings, suppresses streams for non-streaming providers, runs cloud finishes on a dedicated thread, and retries a failed local dictation once on the configured fallback.

**Requirements:** R1, R2, R6, R7

**Dependencies:** U1

**Files:**

- `apps/desktop/src/transcription/engine.rs` (modify; tests in its test module)

**Approach:**

1. Add `cloud_only: bool` and `routed: Mutex<Option<Option<cloud::Provider>>>` fields; thread through `start_with`, `for_test`, and constructors.
2. Split `start(model)` into `start_with(Option<Model>)`; add `#[cfg(target_os = "linux")] pub fn start_without_model()`.
3. `decide()` → `cloud::route_with(self.cloud_only)`; `take_route()` → consume `routed` else `decide()`.
4. `begin()` computes the route once, stores it in `routed`, calls `begin_routed(route)`; `begin_routed` is today's `begin` body gated by `streams_routed(route)`; `streams()` reports `streams_routed(self.decide())`.
5. `finish()`/`finish_with()` funnel to `finish_routed(stream, clip, offline, cloud::run)`: routed provider → drop stream, `finish_cloud(provider, clip, run)` (new `cloud-transcription` thread, `Source::Cloud`, `usage`, debug `engine.route` log); unrouted → build `CloudFallback` when `fallback_provider()` is `Some` (clip clone + provider + run closure with `language_hint`), attach to `PendingTranscript`.
6. `PendingTranscript.fallback` + `wait()` retry: on local `Err`, run the fallback once, map to `Transcribed { source: Cloud, usage }`.
7. Keep the current `padded_whole_clip`/`skip_pauses` finish body unchanged — only the routing funnel wraps it.

**Patterns to follow:** `7c2775e^:apps/desktop/src/transcription/engine.rs` — the pre-sync `finish_routed`, `finish_cloud`, `PendingTranscript::wait` fallback arm, `CloudFallback`, `CloudRun` are the reference implementation; port them onto the new worker shape rather than redesigning.

**Test scenarios:**

- Forced route (`ROUTE_OVERRIDE`) to a provider → `begin()` returns no stream (every `Provider` reports `!streams()` today); `finish` resolves `Source::Cloud` with a `usage` record (inject `cloud` fn via `finish_routed`'s parameter — the seam exists so tests never make a live call).
- The `streams_routed` streaming arm is vacuous while all providers report `!streams()` — keep the code path for a future streaming provider but do not write a test pretending to exercise it.
- Route `None` → `finish` behaves exactly as today (stream/offline/`Fallback` source paths unchanged).
- `FALLBACK_OVERRIDE` set + local failure (offline returns Err) → `wait()` retries once on the fallback and returns `Source::Cloud` with the fallback's usage.
- `FALLBACK_OVERRIDE` set + local success → fallback never runs; result is the local `Source`.
- Fallback unset + local failure → the local error propagates unchanged.
- `cloud_only` engine (`route_for_engine` with `cloud_only = true`) forces a provider even when the Groq cache is cold.
- Two captures in sequence → the second `begin` re-decides (route stored per capture, not globally).

**Verification:** engine test module green; `clippy -D warnings` clean; no live provider calls in tests.

---

### U3. Linux listener and app wiring

**Goal:** Linux warms the key cache at listener start, starts a model-free engine when cloud-routed with no prepared transcriber, stops pre-loading a local model while cloud-routed, and stops demanding a model when dictation alone would run on cloud.

**Requirements:** R1, R3, R6

**Dependencies:** U2

**Files:**

- `apps/desktop/src/linux/dictation.rs` (modify `run` startup + settings-update loop)
- `apps/desktop/src/linux/app.rs` (modify enable gate; `skips_model_install_check` helper + tests in its test module)

**Approach:**

1. `linux/dictation.rs::run`: call `crate::transcription::cloud::refresh_key()` before reading routes (the one place a blocking keyring read is allowed); compute `cloud_only = transcriber.is_none() && cloud::route().is_some()`.
2. Engine start order: `cloud_only` → `TranscriptionEngine::start_without_model()`; else prepared `Some` → `start`; else `labs::unload_idle` → `start_parked`; else `LocalTranscriber::load` + `start` (current behavior).
3. `ModelSwap`: `cloud_only` → `crate::transcription::local_transcriber::ModelSwap::unloaded()` (still present, dead-code-gated — pre-sync hunk at `7c2775e^:linux/dictation.rs` ~line 507); else `ModelSwap::new(&settings.transcription)` as today. Required for R6: without it, `want()` on a cloud→Local switch early-returns because `loaded == selection`, so the model-free engine never gets a model until restart.
4. Settings-update loop: guard `model.want(&next.transcription)` with `route_with(cloud_only).is_none() || !cloud_only` — a cloud-routed listener does not start loading a model. The first clause is load-bearing, not dead: `route_with(cloud_only)` returns `None` under `EngineChoice::Local`, which is exactly the switch-off-cloud case the `unloaded()` swap must serve.
5. `linux/app.rs`: restore `skips_model_install_check(cloud_route, meetings_enabled, capture_enabled)` — `cloud_route && !(meetings_enabled || capture_enabled)` — around the `Model required` gate, per `7c2775e^:apps/desktop/src/linux/app.rs` (~line 700).

**Patterns to follow:** the pre-sync hunks at `7c2775e^:apps/desktop/src/linux/dictation.rs` (~lines 302–330, ~507, ~563) and `7c2775e^:apps/desktop/src/linux/app.rs` (~line 700).

**Test scenarios:**

- `skips_model_install_check` truth table: route + no meetings/capture → skip; route + meetings on → require; route + capture on → require; no route → require (restore the pre-sync table tests if they existed; otherwise write them).
- Enable path with cloud route and no installed model proceeds past the model gate (existing `Model required` tests show the harness).
- `ModelSwap::unloaded().want(selection)` schedules a load (the swap has no `loaded` to compare against — this is the R6 cloud→Local transition).

**Verification:** unit tests above; **Linux compile is the real gate** — `#[cfg(target_os = "linux")]` code cannot build on this Mac, so the `desktop.yml` Linux legs on the PR must pass; do not weaken cfg gates to force a macOS compile.

---

### U5. Dictation usage plumbing into History (Linux + Windows)

**Goal:** `Transcribed.usage`/`served` produced by a cloud finish (or by a swapped local model) reaches the history draft again, so Usage/History records dictation truthfully — closes R7.

**Requirements:** R7

**Dependencies:** U2 (produces the `usage`/`served` fields)

**Files:**

- `apps/desktop/src/linux/dictation.rs` (output worker: `attribute()` helper, `deliver` `usage` parameter, `draft.usage` assignment)
- `apps/desktop/src/windows/dictation.rs` (same plumbing, pre-sync ~lines 816–868)

**Approach:**

1. Restore `attribute(served, usage, audio_ms, text, waited_ms) -> (inference_ms, Option<UsageRecord>)` (pre-sync `linux/dictation.rs` ~lines 1448–1472): `served` contributes the local model's own inference time and a synthesized `UsageRecord::local`; `usage` passes through for cloud results; neither falls back to wait time.
2. Consume `transcribed.served`/`transcribed.usage` at the `job.transcript.wait()` call site and pass the record into `deliver`/the history draft (`draft.usage = Some(usage.unwrap_or_else(|| UsageRecord::local(...)))` — pre-sync linux ~line 261, windows ~line 868).
3. Mirror the same consumption in `windows/dictation.rs` — pre-sync it read `transcribed.usage` at ~line 824 and wrote `draft.usage` at ~line 868.

**Patterns to follow:** pre-sync `7c2775e^:apps/desktop/src/linux/dictation.rs` (~lines 231–270, 407–450, 1448–1472) and `7c2775e^:apps/desktop/src/windows/dictation.rs` (~lines 816–874).

**Test scenarios:**

- A finish carrying `usage: Some(record)` (a cloud result or fallback retry) produces a history draft whose `usage` is that record, not a synthesized local one — covers R7.
- A finish carrying `served: Some(ServedBy)` produces a `UsageRecord::local` with the served model's id and inference time.
- A finish with neither field produces the wait-time local record — unchanged local behavior.

**Verification:** dictation output-worker tests green on both files' test modules; Linux compile via `desktop.yml` legs.

---

### U4. Feature map and sync-resilience notes

**Goal:** The feature docs describe cloud engines again and future Hex syncs know which shared files carry WordInk deltas.

**Requirements:** R1, R7 (documentation of restored behavior)

**Dependencies:** U1, U2, U3, U5

**Files:**

- `apps/desktop/docs/features/dictation.md` (re-add cloud-engine routing description if the sync dropped it — diff against `7c2775e^:` to confirm)
- `apps/desktop/docs/features/README.md` (index touch-up if the cloud entry was removed)
- `apps/desktop/AGENTS.md` (add a short "WordInk deltas over upstream Hex" list naming `engine.rs` routing, `linux/dictation.rs` cloud startup + usage plumbing, `windows/dictation.rs` usage plumbing, `app_settings.rs` cloud setters, `linux/app.rs` model gate — the tripwire for the next sync)
- Repo-root `ROADMAP.md` (mark the restore landed if it has a line item; otherwise leave)

**Approach:** doc-only; check each file against its `7c2775e^:` version and restore accurate statements, not new claims. One required statement beyond pure restore: the restored dictation doc must say plainly that under `engine = Auto` (the default), any resolvable Groq key — including one set for meetings or the fallback — sends dictation audio to Groq. The consent boundary for off-device audio must be documented, not only implied by the UI label.

**Test scenarios:**

- Test expectation: none — documentation only; accuracy checked by diffing claims against the restored code.

**Verification:** docs render; no stale claims (e.g., a doc must not claim macOS dictation is local-only once shared routing is restored).

---

## Verification Contract

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test` for the desktop package (package `hex`, workspace root `cargo test --workspace` or `cd apps/desktop && cargo test`); target modules: `transcription::engine`, `transcription::cloud`, `app_settings`, `control::settings`, `linux::app` test modules.
- Linux-only code compiles under the `desktop.yml` Linux legs on the PR — the authoritative gate for `linux/dictation.rs`/`linux/app.rs`/`start_without_model`.
- Known pre-existing environmental failure on macOS: `capture_capsule_goldens` (Linux-rendered golden images; identical failure on clean `master`) — do not chase it.

## Definition of Done

- `settings.engine` demonstrably routes dictation: named engine → provider (tests via `ROUTE_OVERRIDE`); `Auto` + cached Groq key → Groq.
- `local_cloud_fallback` retries a failed local dictation once; success path unaffected.
- `meeting_route()` reflects `meeting_engine` after settings apply.
- Linux: cloud-routed listener with no installed model starts model-free; "Model required" no longer blocks a cloud-only dictation setup; `model.want` stays quiet while cloud-routed; a cloud→Local settings switch loads the model via `ModelSwap::unloaded()` (R6 holds across engine transitions).
- A cloud-finished dictation writes its `UsageRecord` into the history draft on Linux and Windows (R7).
- No live provider calls in tests; all injection through the existing `*_OVERRIDE` seams and `finish_routed`'s `cloud` parameter.
- Feature docs match restored behavior; AGENTS.md names the delta surfaces.
- No abandoned-attempt code in the diff; `cargo fmt`/`clippy`/tests green; Linux CI green.
- One PR, opened but not merged by the agent.

## Sources / Research

- The sync commit: `7c2775e` ("Sync desktop to Hex v3.4.0", #176). Pre-sync sources read at `7c2775e^:` — `apps/desktop/src/transcription/engine.rs` (lines ~410–985 carry the whole dispatch), `apps/desktop/src/linux/dictation.rs` (~302–330, ~563), `apps/desktop/src/linux/app.rs` (~700), `apps/desktop/src/app_settings.rs` (~2459–2469).
- Investigation evidence: `cloud::run` callers today are only meetings, `recover.rs`, and `transcribe_file_cli`; `route()`/`route_with`/`route_for_choice` have no non-test callers; `set_engine`/`set_local_fallback`/`set_meeting_engine`/`set_clip_retention` have no callers — all verified by `git grep` on current `master` vs `7c2775e^`.
- Behavioral baselines measured this session: Groq round trip 0.3–0.5s; FLAC encode 18ms/60s — the request path was never the 9s suspect; the unwired dispatch is the regression.
