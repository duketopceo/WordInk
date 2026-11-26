# Residual Review Findings — fix/cloud-dictation-dispatch

Source run: ce-code-review `mode:agent` on `master...fix/cloud-dictation-dispatch` (HEAD 6532fe9).
Plan: `docs/plans/2026-11-26-001-fix-cloud-dictation-dispatch-plan.md` (KTD1 pins pre-sync parity shape).
Reviewer coverage: correctness, testing, standards/plan (partial — reviewer died on rate limit; R1–R7 mapped manually), adversarial (timed out — coverage gap).

## Downstream-resolver findings (not applied; pre-sync parity by design)

- minor — `apps/desktop/src/dictation/audio.rs:1098` — A macOS `/dictations` service capture never calls `engine.begin()`, so a stranded `routed` decision from a discarded hotkey capture can be consumed by the next service finish (audio uploaded per a discarded capture's decision). Suggested: clear `routed` on capture cancel/discard, or key the decision to a capture generation. Pre-sync parity; plan Open Questions.
- minor — `apps/desktop/src/linux/dictation.rs:1195` (+ `windows/dictation.rs:903`, `audio.rs:902`) — `begin_stream` retries `engine.begin()` per audio chunk; under a cloud route or non-streaming model `begin()` returns None every chunk, so `routed` is overwritten per chunk and the finish uses the last chunk's decision rather than recording-start's. Drift vs the documented "decided once at begin" invariant; pre-sync retry shape.
- minor — `apps/desktop/src/transcription/engine.rs:541-560` — `PendingTranscript::wait` retries on ANY local `Err`, including abandonment and timeout: a cancelled/timed-out dictation still uploads to the cloud provider on the output thread. Accepted parity (plan).

## Nits (report-only)

- `windows/dictation.rs` `stream_bench` reports "this model cannot stream" under a cloud route (`engine.begin()` → None). Dev-tool-only regression.
- `app_settings.rs:815-830` — `set_engine` warms the key cache before `set_meeting_engine` stores the meeting choice; a same-apply meeting-key change self-heals on the 30 s staleness refresh.
- `history.rs::usage_or_local` — when `served`/`usage` are both None, the synthesized record names the saved selection, which can differ from the actual model mid-swap.
- `parakeet.rs:1194` — macOS dictation discards the cloud `UsageRecord` (`usage: None`). Documented out-of-scope (beyond-parity future work).
- `engine.rs` `finish_cloud` — `thread_cancel` is checked only after `run(&clip)` returns; a wedged HTTPS call holds a detached thread. Soft deadline by design.
- `engine.rs` `local_finish`/fallback arm — `clip.clone()` (~MB) per finish while a fallback is configured.

## Testing residuals

- minor — `finish_routed` fallback+open-stream arm (`Command::Finish { stream: Some }`) and `decide()` under `cloud_only` are untested at engine level; Linux-gated surfaces (`attribute`, `usage_or_local`, `NO_LOCAL_MODEL`, `skips_model_install_check`) compile only under `desktop.yml` Linux legs.
- nit — `Source::Cloud` missing from `source_labels_distinguish_fallbacks` label test.

## Settled-conflict findings

None. No `settled_conflict` stamps; no proceeded-and-flagged `settled_decision_conflicts` from ce-work.
