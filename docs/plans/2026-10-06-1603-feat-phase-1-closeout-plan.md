---
title: Phase 1 Closeout - Plan
type: feat
date: 2026-10-06
topic: phase-1-closeout
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-brainstorm
execution: code
---

# Phase 1 Closeout - Plan

## Goal Capsule

- **Objective:** Phase 1 is actually shipped — the `@wordink` packages exist on npm with trusted publishing and provenance, the docs site is live on GitHub Pages, and the closeable PR #26 residuals are fixed. After this work, anyone can `npm install @wordink/web` and open `https://duketopceo.github.io/WordInk/`.
- **Product authority:** Luke (duketopceo). This plan owns the Phase 1 closeout: landing the PR chain, npm publish go-live, GitHub Pages, and the feasible review residuals. A TypeWhisper plugin, LLM cleanup, usage logging, and a Python SDK are not active scope.
- **Open blockers:** whether the `wordink` npm org name is claimable (OQ1); merging the PR chain is the operator's call throughout.

---

## Product Contract

### Summary

Ship Phase 1 for real: land the stacked PR chain on `master`, take `@wordink/*` live on npm through the already-built trusted-publishing pipeline, turn on GitHub Pages, fix the residuals that need no live provider keys, and refresh the roadmap.

### Problem Frame

Phase 1 is built and reviewed — eleven stacked PRs ending at #26, plus the Phase 2 gateway at #27 — but nothing has reached `master`, npm, or the public docs site. The release machinery (`release.yml`, `docs.yml`) was built inside the chain and is waiting for one-time account-side setup it cannot perform itself. Until the scope is claimed, trusted publishing is configured, and the chain merges, every downstream surface — the gateway's own install docs, a TypeWhisper plugin, any adopter — routes around the missing package instead of using it.

### Key Decisions

- **Closeout scope is land + publish + Pages + feasible residuals.** (session-settled: user-approved — chosen over publish-only and drop-residuals: the residuals left are small, and shipping with them open repeats the review debt.) Governs R9, R10, R11.
- **OpenAI/Deepgram live-API checks are deferred, not closed.** (session-settled: user-directed — chosen over adding or creating provider keys now.) Governs R13.
- **Unscoped package names are the carried contingency.** `release.yml` already prescribes the `wordink-*` rename if the `@wordink` scope is unavailable; this plan adopts that contingency instead of re-deciding it. Governs R3.

### Requirements

**Landing**

- R1. The open PR chain (#18 through #27) lands on `master` oldest-first; each PR merges only after its parent has merged and its base has retargeted.
- R2. Each merge waits for that PR's checks to be green; no PR merges over failing or pending checks.

**npm publishing**

- R3. The `wordink` npm organization is created under the `duketopceo` account so the `@wordink` scope exists; if the scope cannot be had, the five packages are renamed to `wordink-core`, `wordink-web`, `wordink-react`, `wordink-local`, `wordink-server` and every scoped reference in code, docs, and READMEs is updated.
- R4. Each package exists on npm before trusted publishing is configured, via a manual first publish or npm's placeholder flow.
- R5. Trusted publishing is configured per package for owner `duketopceo`, repository `WordInk`, workflow `release.yml`, no environment.
- R6. Each package's publishing access requires two-factor authentication and disallows tokens, so only the OIDC workflow can publish.
- R7. After setup, merging a "Version Packages" PR publishes every pending version to npm with provenance; versions already on the registry are skipped, so re-runs are safe.

**Docs site**

- R8. Repository Pages source is set to "GitHub Actions", after which `docs.yml` deploys the built site to `https://duketopceo.github.io/WordInk/` on pushes to `master`.

**Residuals**

- R9. A WebSocket that stalls before opening is not reported as `AuthFailed`; the surfaced error reflects a connection or provider problem instead.
- R10. The `INFERENCE_TIMEOUT_MS` (30 s) versus 60 s utterance cap interaction is measured on a slow inference path and the finding is recorded: either the collision is proven impossible or a fix lands.
- R11. Tests cover the streaming path through the 60 s cap and blur while a press waits on wasm load or token mint.

**Repository docs**

- R12. `ROADMAP.md` reflects the shipped Phase 2 decision (integrate, delivered through PR #27), the closeout state of Phase 1, and the revised Phase 3/4 framing.
- R13. The OpenAI/Deepgram live-API checks remain a recorded open residual naming what is blocked (no keys) and exactly what is deferred: OpenAI GA session shape, `gpt-live-transcribe`, `ek_` subprotocol auth; Deepgram `token` vs `bearer` subprotocol.

<!-- ce-section: work-relationships -->
### How This Work Fits Together

This plan covers the Phase 1 closeout area only; the surrounding breakdown is the current understanding, not a committed roadmap.

- **PR chain #18–#27** — Depends on: everything in this plan happens on or after these merges.
- **TypeWhisper plugin** — Depends on: a published `@wordink/server` (or the same install workaround the gateway docs carry pre-publish). Tentative next area per `docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md`.
- **LLM cleanup and usage logging** — Depends on: the gateway request path shipped in PR #27. Can proceed independently of this closeout.
- **Python SDK** — Can proceed independently of this closeout; still exploratory per `ROADMAP.md`.

### Actors

- A1. **Operator (Luke):** runs the npm-side setup, flips the Pages source, merges each PR, performs the manual blur handcheck.
- A2. **Release automation:** `release.yml`'s version and publish jobs on `master` pushes.
- A3. **CI and branch protection:** gates every merge on green checks.
- A4. **npm registry:** hosts the packages and enforces the trusted-publishing configuration.

### Key Flows

- F1. Land the chain
  - **Trigger:** This plan is underway and the operator is ready to merge.
  - **Actors:** A1, A3
  - **Steps:** Merge the oldest open PR → its dependents retarget to `master` → wait for the next PR's checks → merge → repeat through #27.
  - **Outcome:** `master` holds Phase 1 plus the gateway; the release workflow becomes live.
- F2. npm publish go-live
  - **Trigger:** The chain has landed and the operator is logged into npm (`Npmjs / duketopceo@gmail.com` in omaseal).
  - **Actors:** A1, A2, A4
  - **Steps:** Create the `wordink` org (or execute the R3 fallback) → ensure each package exists → configure trusted publishing per R5 → require 2FA per R6 → merge a "Version Packages" PR → verify all five packages on npm with provenance.
  - **Outcome:** `@wordink/*` installs from the public registry with no stored npm token.
- F3. Enable Pages
  - **Trigger:** `docs.yml` is on `master`.
  - **Actors:** A1, A2
  - **Steps:** Set Pages source to "GitHub Actions" → the next `master` push deploys → verify the site and its live demo load at `https://duketopceo.github.io/WordInk/`.
  - **Outcome:** The public docs site is live.

### Acceptance Examples

- AE1. **Covers R3.** Given the `wordink` org cannot be created, when publishing is configured, then the packages publish under the unscoped `wordink-*` names and no code, doc, or README still points at `@wordink/`.
- AE2. **Covers R7.** Given a package's computed version already exists on npm, when the publish job runs, then that version is skipped and the job still succeeds.
- AE3. **Covers R9.** Given a WebSocket never reaches the open event, when the stall is observed, then the surfaced error is not `AuthFailed`.
- AE4. **Covers R1, R2.** Given #18 merges first, when the merge lands, then #19's base retargets without manual branch surgery and #19 cannot merge red.

### Scope Boundaries

**Deferred for later**

- OpenAI/Deepgram live-API checks (R13 records them as the open residual).
- The cross-browser blur-permission handcheck — an operator verification step with fresh mic permissions in Chrome, Firefox, and Safari, not a plan unit.
- TypeWhisper plugin, LLM cleanup, usage logging, Python SDK — separate areas in *How This Work Fits Together*.

**Outside this product's identity**

- A WordInk-branded standalone desktop app — settled in the gateway plan.

### Dependencies / Assumptions

- npm login for `duketopceo` exists in omaseal as `Npmjs / duketopceo@gmail.com`; account-side setup needs it.
- Merges stay with the operator; `master` protection (PR + checks) already enforces R2 mechanically.
- `release.yml` and `docs.yml` are in the repo on the current branch and reach `master` with the chain; their header comments are the authoritative one-time setup recipes.
- The `@wordink` scope's availability is unknown until checked while logged in; the public probe was inconclusive.

### Outstanding Questions

- OQ1. Is the `wordink` npm org name claimable? **Deferred to Planning** — resolved at the first operator step; the fallback is already decided by R3.
- OQ2. What version does the first "Version Packages" PR produce from the pending changesets? **Deferred to Planning** — visible when the version PR opens; no decision needed now.

### Sources / Research

- `docs/plans/2026-10-05-1131-feat-wordink-desktop-gateway-plan.md` — parent plan; Phase 2 decision and gateway delivery this closeout builds on.
- `.github/workflows/release.yml` header comment — the authoritative npm org, trusted-publishing, and 2FA setup recipe this plan sequences.
- `.github/workflows/docs.yml` header comment — the Pages source toggle.
- PR #26 body — the residual list this plan's R9–R11 and the deferred items come from.
- `ROADMAP.md` — the stale Phase 2 status R12 corrects.

---

## Planning Contract

**Product Contract preservation:** unchanged.

### Key Technical Decisions

- KTD1. **A private close code separates stall from rejection — carried on `Rejected`.** `openSocket` in `packages/core/src/host.ts` arms a connect-timeout timer whose `failSocket` path reports `ws_closed(id, 1006)`, indistinguishable from a real close-before-open — which is how browsers surface a 401 handshake, and why the core deliberately maps it to `AuthFailed`. The mapping site is `SocketEvent::Rejected` (`providers/mod.rs:232,243`), not `ws_close_error` — that function only serves sockets that had opened (`Closed`), so `Rejected` gains a code payload (`Rejected(u16)`) and `failure()` maps the private stall code to `ProviderDown`, everything else to `AuthFailed` as today. The timeout path in `failSocket` feeds a WordInk-private close code (4000–4999); the constructor-throw and natural-close paths keep 1006. The constant's owner is the core (export it via the wasm package if the bindings allow; otherwise duplicate it in `host.ts` with cross-referencing comments). Real auth rejections are unchanged. Governs R9, AE3.
- KTD2. **Inference timeout scales with utterance length.** `INFERENCE_TIMEOUT_MS` (30 s) wraps the local batch transcribe while `max_utterance_ms` (60 s) bounds the input; a long utterance on a slow WASM device can out-run the flat timeout. U2 first measures real-time factor on a ~60 s clip, then — only if the collision is real — scales the timeout by captured duration with the 30 s floor kept for short clips. Governs R10.
- KTD3. **Closeout code lands on a stacked branch.** `feat/p1-closeout` branches from `feat/p2-gateway` and its PR targets `feat/p2-gateway`, so the diff shows only this plan's changes — the same stacking rule as PR #27 on #26.
- KTD4. **The squash cascade needs no rebases.** Merging oldest→newest with delete-branch-on-merge lets GitHub auto-retarget each next PR to `master`; the three-dot file diff stays correct even while the commit list still shows the absorbed parent's commits. Merging stays with the operator — this plan sequences the merges and verifies retargets, it does not merge. Governs R1, R2, AE4.
- KTD5. **The unscoped rename runs only on scope failure.** If a logged-in npm check shows `wordink` cannot be claimed, the rename to `wordink-core`, `wordink-web`, `wordink-react`, `wordink-local`, `wordink-server` is its own commit before any publish step: the five `name` fields, workspace dependency specifiers, docs, and READMEs. Governs R3, AE1.

### High-Level Technical Design

Two kinds of work run under one gate. The code slice is small and bounded: one core/host error-mapping change (KTD1), one conditional local-engine timeout fix (KTD2), two test additions, and a roadmap edit — all on the stacked branch in KTD3. The operator slice is sequenced account-side work: the merge cascade (KTD4), the npm go-live per `release.yml`'s own recipe (org → first publish → trusted publishing → 2FA), and the Pages toggle. The merge cascade is the pivot: every operator step downstream of it requires `release.yml`/`docs.yml` on `master`, and the first publish should include the residual fixes, so code lands before the cascade completes.

### Output Structure

- `feat/p1-closeout` PR: Rust core mapping + host stall signal + tests, local-engine timeout, residual tests, `ROADMAP.md`, and a changeset if a published package changes.
- Operator runbook executed live: merge order, npm steps, Pages steps — recorded in this plan's units, not a new doc.

### Risks & Dependencies

- **`@wordink` scope unavailable** → KTD5 rename path; blocked publish until resolved.
- **Squash-cascade retarget lag** → a PR momentarily shows the parent's absorbed commits; the file diff stays correct and the next merge still gates on green checks.
- **npm org creation and trusted-publishing config are web-UI steps** → the agent can verify and drive npm/gh CLIs but cannot click npmjs.com; those steps are operator-owned with agent-run verification.
- **R10's measurement may show no collision** → then no code changes; the finding is recorded in the PR.

## Implementation Units

### U1. WS-stall error surfacing

**Goal:** A WebSocket connect that times out surfaces a connection/provider error, not `AuthFailed`.
**Requirements:** R9, AE3, KTD1.
**Dependencies:** None.
**Files:** `crates/wordink-core/src/providers/mod.rs`, `packages/core/src/host.ts`, `crates/wordink-core/tests/providers.rs`, `packages/core/test/host.test.ts`.
**Approach:**
1. Pick a private close code in 4000–4999 (e.g., `WS_CONNECT_STALL = 4408`), owned by the core; export or duplicate it into `host.ts` per KTD1.
2. `SocketEvent::Rejected` becomes `Rejected(u16)` carrying the close code; `failure()` maps the stall code to `ErrorCode::ProviderDown`, all other codes to `AuthFailed` — the 401-hides-as-1006 semantics stay exact.
3. `failSocket`'s connect-timeout path in `host.ts` feeds `ws_closed(id, WS_CONNECT_STALL)`; the natural-close and constructor-throw paths keep 1006. Update existing `Rejected` tests to the payload form (`tests/providers.rs:460+`).
**Test scenarios:**
- Rust: a session whose last candidate closes with the stall code ends in `ProviderDown`, not `AuthFailed`; `Rejected(1006)` and mid-chain candidate retries behave exactly as today (existing tests updated, not weakened).
- Host: a socket that never opens before `WS_CONNECT_TIMEOUT_MS` produces the private code event; a socket that closes with 1006 before open still maps as today.
**Verification:** `cargo test -p wordink-core` and `pnpm --filter @wordink/core test`; wasm gzip stays under 150 KB.

### U2. Local inference timeout versus utterance cap

**Goal:** Prove or remove the `INFERENCE_TIMEOUT_MS` (30 s) / `max_utterance_ms` (60 s) collision.
**Requirements:** R10, KTD2.
**Dependencies:** None.
**Files:** `packages/local/src/index.ts`, `packages/local/test/` (existing suite), measurement recorded in the PR.
**Approach:**
1. Measure real-time factor of the bundled model on a ~60 s clip on this machine (and note the margin for slower devices — the e2e suite already runs the model under WASM).
2. If 60 s of audio can exceed 30 s of inference within a realistic margin, scale the timeout by captured audio duration (30 s floor for short clips, e.g., `max(30_000, capturedMs × factor)`); the factor comes from the measurement, not a guess.
3. If the measurement shows the collision cannot happen, make no code change and record the numbers.
**Test scenarios:**
- A transcription whose audio exceeds the flat timeout succeeds under the scaled timeout.
- The 30 s floor still applies to short audio (a fake-timer test over the scaling function).
**Verification:** `pnpm --filter @wordink/local test`; measurement numbers in the commit/PR.

### U3. Residual test coverage

**Goal:** Close the two testable PR #26 residual gaps.
**Requirements:** R11.
**Dependencies:** None.
**Files:** `crates/wordink-core/tests/` (streaming cap test — the file the session tests live in), `packages/web/test/wordink-mic.test.ts`.
**Approach:**
1. Streaming provider + `max_utterance_ms` reached: the session stops capture and transcribes what it has rather than dropping or hanging — pin the emitted effects.
2. `wordink-mic`: a press whose engine is still loading (wasm/token mint pending) then a `blur` fires — the hold resolves to a defined state (released + no stuck mic-open) per `onFocusLost`'s contract in `wordink-mic.ts:298`. Extend the existing blur test group.
**Test scenarios:**
- Streaming path hits the cap mid-utterance: `StopMic` + transcribe emitted once.
- Blur during pending start: no second release, no stuck hold; state is not `listening`.
**Verification:** `cargo test --workspace`, `pnpm --filter @wordink/web test`.

### U4. Roadmap and residual record

**Goal:** `ROADMAP.md` tells the truth about where the project is.
**Requirements:** R12, R13.
**Dependencies:** U1–U3 (records their landing).
**Files:** `ROADMAP.md`.
**Approach:**
1. Phase 2 row: decision made (integrate), shipped through the gateway plan and PR #27.
2. Phase 1 row: closeout state — published/pending as of this branch.
3. Phase 3/4 rows: TypeWhisper plugin as the tentative next area; Phase 3's standalone-app premise noted as dropped by the Phase 2 decision.
4. R13's deferred list verbatim: OpenAI GA session shape, `gpt-live-transcribe`, `ek_` auth; Deepgram `token` vs `bearer`.
**Verification:** Docs-only; review diff.

### U5. Closeout PR and chain landing

**Goal:** This plan's changes reach `master` via the stacked chain.
**Requirements:** R1, R2, AE4, KTD3, KTD4.
**Dependencies:** U1–U4.
**Files:** none (PR and merges).
**Approach:**
1. Commit U1–U4 to `feat/p1-closeout`; open PR targeting `feat/p2-gateway`; watch CI green.
2. Operator merges the chain oldest→newest (#18 … #26, #27, this PR). After each merge, verify the next PR retargeted to `master` (`gh pr view N --json baseRefName`) and its checks re-run green before the next merge.
3. Any merge that stalls (conflict after retarget, red check) pauses the cascade and reports — it does not get resolved by force.
**Verification:** All PRs `MERGED` via `gh pr view`; `master` contains the work; no open PR left from the chain.

### U6. npm publish go-live

**Goal:** `@wordink/*` (or the KTD5 fallback names) installs from npm with trusted publishing and provenance.
**Requirements:** R3–R7, AE1, AE2, KTD5.
**Dependencies:** U5 (the release workflow must be on `master`).
**Files:** none unless the KTD5 rename runs (then the rename commit's files).
**Approach (operator steps, agent verifies each):**
1. Logged-in npm check on the `wordink` scope; if unclaimable, run the KTD5 rename commit and re-open the publish path.
2. Create the `wordink` org; first-publish each of the five packages by hand (`pnpm build && npm publish --access public` from each package dir) or via npm's placeholder flow.
3. Per package: Settings → Trusted publishing → GitHub Actions publisher `duketopceo`/`WordInk`/`release.yml`, no environment; then Publishing access → require 2FA, disallow tokens.
4. Merge the "Version Packages" PR `release.yml` opens; watch the publish job; `npm view @wordink/<pkg> version` for each, and confirm the provenance attestation on npmjs.com.
**Verification:** `npm view` returns a version for all five; `npm install @wordink/web` resolves from a clean project; provenance badge present.

### U7. GitHub Pages go-live

**Goal:** The docs site serves publicly.
**Requirements:** R8, F3.
**Dependencies:** U5 (`docs.yml` on `master`).
**Files:** none (repo setting).
**Approach:**
1. Set Pages source to GitHub Actions (`gh api repos/duketopceo/WordInk/pages -X POST -f build_type=workflow` or the Settings UI if the API path needs admin scope).
2. Trigger/observe the `docs.yml` deploy on the next `master` push; check the run.
3. `curl -s -o /dev/null -w "%{http_code}" https://duketopceo.github.io/WordInk/` → 200; open the live demo page and confirm the demo mounts.
**Verification:** Deploy job green; site 200; demo mounts (quickstart page loads with no console errors).

## Verification Contract

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — U1, U3 touch the core.
- `pnpm typecheck && pnpm test` — U1–U3.
- Wasm gzip < 150 KB (`cargo build -p wordink-wasm --target wasm32-unknown-unknown --release` + gzip the output) — U1 changes the core.
- `pnpm --filter @wordink/docs test:e2e` — only if `apps/docs/` changes in U4's sweep (it should not).
- U5: every merged PR's checks were green at merge; each dependent PR's `baseRefName` is `master` after retarget.
- U6: `npm view @wordink/{core,web,react,local,server} version` returns versions; provenance visible on npmjs.com; a clean `npm install @wordink/web` resolves.
- U7: `docs.yml` deploy run green; `https://duketopceo.github.io/WordInk/` returns 200; demo page mounts.

## Definition of Done

- R1–R2: the full chain is merged on `master`, oldest-first, all merges green.
- R3–R7: all five packages publish from `release.yml` via OIDC trusted publishing with provenance; no stored npm token anywhere.
- R8: the docs site serves at `https://duketopceo.github.io/WordInk/` with the demo working.
- R9–R11: tests green pin the stall error, the timeout behavior, and both residual gaps.
- R10: the measurement is recorded in the PR whether or not a fix was needed.
- R12–R13: `ROADMAP.md` reflects reality; the deferred live-API list is recorded.
- Operator-owned steps (merges, npm web UI, Pages toggle) are surfaced to Luke as they come due; deferred items (OpenAI/Deepgram live checks, blur handcheck) are recorded, not closed.
- A changeset exists for any published-package change; the closeout PR is open, stacked correctly, and its CI is green.
