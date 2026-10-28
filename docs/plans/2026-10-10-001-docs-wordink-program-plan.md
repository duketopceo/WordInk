---
title: WordInk Program Roadmap - Plan
type: docs
date: 2026-10-10
topic: wordink-program
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-plan
execution: code
---

# WordInk Program Roadmap - Plan

## Goal Capsule

- **Objective:** a single program-level view of what WordInk ships next, in Now / Next / Later order, with status, evidence, difficulty, feasibility and a simpler alternative for every unit. It sits above the per-phase plans in `docs/plans/` and does not replace them.
- **Product authority:** Luke (`duketopceo`). One decision below remains marked **owner decision** and this plan does not make it: whether to merge the Argus reviewer PR (U4). The other — standalone desktop apps (U9) — was decided by the owner on Oct 24, 2026: standalone first-party apps on all four OSes.
- **Evidence date:** 2026-10-10, from `master` at `7057304`, GitHub PR/issue state, `gh run list`, `npm view` and the repo's own checks (all green locally, see Verification Contract).
- **Out of scope:** application code. This pass changes docs and brand assets only. Code lands from the units below.

---

## Product Contract

### Summary

WordInk is dictation as a component: a Rust sans-I/O core compiled to WebAssembly (75.8 KB raw / 34.7 KB gzip against a 150 KB budget), a TypeScript browser host, `<wordink-mic>`, a React hook, an in-browser local engine (Moonshine), a fail-closed credential relay and an OpenAI-compatible desktop gateway in `@wordink/server`, plus a Python CLI and MCP server in `sdks/python`.

### Problem Frame

Phases 0, 1, 2 (integration route) and 4 (Python) are merged to `master`. The docs site is live (HTTP 200). Three things still stop it being a product people can adopt:

- **Nothing is on npm.** `npm view @wordink/web version` returns 404. The release workflow is dormant because the `NPM_PUBLISH_ENABLED` repository variable is unset, and the "Version Packages" PR (#32) is open.
- **`master` CI is red.** The `e2e` job failed on the merge of #36 (`apps/docs/e2e/demo.spec.ts`: `#demo-text` stayed empty).
- **The repo contradicts itself.** `ROADMAP.md` says Phase 2 chose integration and Phase 3 is dropped, while issues #13 and #14 still describe a standalone app, issue #8 (U5) is open though its PR merged, and the README says a desktop app comes "later".

### Requirements

- R1. Every unit carries a stable U-ID, a status with evidence, Difficulty, Feasibility and a Simpler alternative.
- R2. Phase 2 desktop options are recorded with difficulty and feasibility (none was selected when written; the owner selected B + B′ extended to iOS on Oct 24, 2026).
- R3. The `WF-WRITE` audit finding is resolved to a named workflow and a verdict, without changing any workflow.
- R4. Landscape claims trace to `docs/research/2026-10-10-landscape.md`.

### Phase 2 desktop options (decided Oct 24, 2026 — see below)

Historical context: when this plan was written, `ROADMAP.md` recorded Option A as chosen and shipped (the gateway and `/v1/models` are on `master` (PR #29), and Voxtype on the owner's machine dictates through it), while the owner's notes treated the wider desktop direction as open. The options are kept below for the record; the choice was made on Oct 24, 2026 (see the owner decision under the table).

| Option | What it is | Difficulty | Feasibility and blockers |
|---|---|---|---|
| A. Integrate (state per ROADMAP: done) | `wordink-gateway` as an OpenAI-compatible endpoint; Voxtype remote mode; TypeWhisper via its OpenAI-compatible engine. | Easy to extend (each new client is a docs page plus a smoke test) | Works today. Limits: depends on third-party apps keeping their OpenAI-compatible engines; TypeWhisper has no Linux build and is GPLv3, so a bundled plugin would carry GPL terms; Voxtype's remote option is only described as "remote Whisper servers" on its site, so the exact protocol needs re-checking per release. |
| A+. Add more clients to A | Handy, OpenWhispr, hyprwhspr, whisrs, Spokenly (all listed in `ROADMAP.md` as supporting local/BYOK or cloud engines) get gateway recipes. | Easy to medium | Needs per-app verification on each OS; some may lack a custom-endpoint setting. |
| B. Standalone Linux app | Native host for the Rust core: portal global shortcuts, virtual-keyboard injection, tray. | Hard | Wayland has no universal global-hotkey or text-injection API (portals, compositor binds, `wtype`/`ydotool` each work on a subset); competes directly with Voxtype (MIT, 8 engines, GPU); needs packaging (AUR, Omarchy). |
| B'. Standalone Windows/macOS app (issue #14) | Same app elsewhere, replacing `legacy/`. | Hard | macOS accessibility permission flow and signed, notarized builds need an Apple Developer account; Windows signing costs money; competes with Wispr Flow, Superwhisper, TypeWhisper. |
| C. Skip desktop | All effort into the SDK, Python, mobile and provider hardening. | Easy (it is a scope decision) | Drops the desktop story; the gateway stays as a supported but frozen surface. |

**Owner decision (U9), made Oct 24, 2026:** standalone first-party apps on all four OSes — options B + B′ extended to iOS (WordInk Remote becomes a standalone product, not BYO-gateway-only). Hard requirement stated with the decision: every app must work on default open-source configuration — BYO provider key or the bundled local engine, no required self-hosted gateway or third-party dictation app. Option A stays shipped and supported as an optional deployment mode.

### `WF-WRITE` audit finding

Workflows with write permissions, and whether each needs it:

| Workflow | Write scope | Needed? |
|---|---|---|
| `.github/workflows/release.yml`, job `version` | `contents: write`, `pull-requests: write` | Yes. `changesets/action` pushes the version branch and opens the "Version Packages" PR. |
| `.github/workflows/release.yml`, job `publish` | `contents: write`, `id-token: write` | Yes. Pushes release tags; `id-token` is the OIDC exchange for npm trusted publishing and provenance. Job is dormant until `NPM_PUBLISH_ENABLED=true`. |
| `.github/workflows/docs.yml`, job `deploy` | `pages: write`, `id-token: write` | Yes. Required by `actions/deploy-pages`. |
| `ci.yml` | `contents: read` only | n/a |
| `argus-mention.yml` (in open PR #35, not on `master`) | `contents: write`, `issues: write`, `pull-requests: write`, `checks: write`, `statuses: write` | Only if the owner wants `@argus persist`, `generate` or `fix`, which commit to an `argus/` branch. For review-only use it can drop to `contents: read`. It triggers on `issue_comment` with a secret present and only checks that the comment starts with `@argus`; it has no author-association check, so on a public repo any commenter could trigger a run that spends the OpenRouter key. |
| `argus-reviewer.yml` (PR #35) | `contents: read`, plus `issues`/`pull-requests`/`checks`/`statuses: write` | Writes are for review comments and checks; `contents` is already read-only. Acceptable. |

The most likely referent of the audit's `WF-WRITE` flag is `argus-mention.yml` (the only workflow whose write scope is optional). The three existing `master` workflows need what they request. Recommendation recorded for the owner under U4; no workflow is changed in this pass.

### Scope Boundaries

- No application code, workflow or lockfile changes in this pass.
- Phase 3 (standalone Linux, Windows and macOS apps) is reopened — U9 decided Oct 24, 2026.

---

## Planning Contract

### Key Technical Decisions

- KTD1. Order by what unblocks adoption: green CI, then npm, then truth in the docs and backlog, then new capability.
- KTD2. Every unit names a simpler alternative so scope can be cut instead of added.
- KTD3. Decisions that change product identity (desktop app, bot review spend) stay with the owner.

### Dependencies / Assumptions

- Operator-only steps (npm org, trusted publishing, repo variable, merging) cannot be done by an agent; units say so.
- Pricing figures in the research note come from vendor pages and aggregators and drift.

---

## Implementation Units

### Now

#### U1. Make `master` CI green again

- **Status:** open. Evidence: `gh run list` shows CI on `master` failing at `7057304` (job `e2e`: `apps/docs/e2e/demo.spec.ts` expects `#demo-text` not empty; the three other jobs pass). The same suite passes on the branch run of #36.
- **Dependencies:** none.
- **Difficulty:** easy to medium. The failure is one test; the cause (flaky fake-mic or wasm timing versus a real regression) is unknown until reproduced.
- **Feasibility:** CI-only risk. Firefox needs the PulseAudio null sink already in `ci.yml`. See `docs/solutions/test-failures/chromium-fake-mic-loops-from-launch.md` for a related past failure.
- **Simpler alternative:** re-run the job and compare; if it is a timing flake, widen the assertion timeout in `apps/docs/e2e/demo.spec.ts` rather than restructuring the demo.
- **Verification:** three consecutive green `master` CI runs; `pnpm test:e2e` locally.

#### U2. Take `@wordink/*` live on npm

- **Status:** not started. Evidence: `npm view @wordink/web version` is 404; `gh variable list` shows no `NPM_PUBLISH_ENABLED`; PR #32 (version packages, core 0.1.0) is open. Procedure is already written in `.github/workflows/release.yml` header and `docs/plans/2026-10-06-1603-feat-phase-1-closeout-plan.md` U6.
- **Dependencies:** U1 (publish job runs `pnpm test`).
- **Difficulty:** medium. Mostly account work; the pipeline exists.
- **Feasibility:** blocked on the owner: the `@wordink` npm scope may be unclaimable (the closeout plan has a fallback to unscoped names), each package must exist before trusted publishing can be attached, and 2FA is required. No agent can complete it.
- **Simpler alternative:** publish a first version by hand to create the packages, then let CI do every later release. The README's CDN quickstart (`cdn.jsdelivr.net/npm/@wordink/web@VERSION`) depends on this unit.
- **Verification:** `npm view @wordink/{core,web,react,local,server} version` all resolve; provenance badge present; a clean `npm install @wordink/web` works.

#### U3. Reconcile backlog and docs with reality

- **Status:** open. Evidence: issues #4-#12 are U1-U9 tracking issues (only #8 still open though PR #19 merged); #14 describes a standalone desktop app — reconciled Oct 24, 2026 when U9 decided standalone (#14 stays open as the tracking issue; #13 stays closed); #15 mixes the built Python work with mobile. `README.md` says a desktop app comes "later"; `AGENTS.md` says "A desktop app comes later" and that `legacy/` stays until "WordInk Desktop" replaces it — now accurate under U13.
- **Dependencies:** none (U9 may change the wording).
- **Difficulty:** easy.
- **Feasibility:** none; needs owner approval to close issues.
- **Simpler alternative:** one tracking issue per roadmap unit and close the rest, instead of keeping phase-shaped issues.
- **Verification:** `gh issue list` matches this plan's units; README and AGENTS.md agree with `ROADMAP.md`.

#### U4. Decide the Argus reviewer PR and its write scope (owner decision)

- **Status:** open. Evidence: PR #35 adds `argus-reviewer.yml`, `argus-mention.yml`, `argus-reviewer.config.ts` and a smoke test; needs an `OPENROUTER_API_KEY` secret; default budget $1/run.
- **Dependencies:** none.
- **Difficulty:** easy.
- **Feasibility:** spend must go through the dedicated OpenRouter key per `AGENTS.md` of the owner's workspace; see the `WF-WRITE` table for the `argus-mention.yml` issues.
- **Simpler alternative:** merge only `argus-reviewer.yml` (read-only contents) and skip `argus-mention.yml`, or add an `author_association` guard and drop `contents: write` first.
- **Verification:** after merge, a test PR gets a review comment; a comment from a non-collaborator does not trigger a run.

### Next

#### U5. Live-verify the OpenAI and Deepgram adapters

- **Status:** deferred in `ROADMAP.md`. Evidence: unit tests pass; the open questions are OpenAI GA session shape, `gpt-live-transcribe`, `ek_` auth, and Deepgram `token` versus `bearer`. OpenAI's current docs list `gpt-transcribe`, `gpt-4o-transcribe`, `gpt-4o-mini-transcribe` and `whisper-1` (see research note), so model names in `packages/server/src/openai.ts` need re-checking.
- **Dependencies:** none; U2 for any release.
- **Difficulty:** medium.
- **Feasibility:** needs provider keys and small spend; use the `orch` wrapper and the dedicated key only.
- **Simpler alternative:** make Groq (the default, `whisper-large-v3-turbo` at $0.04/hour) the only guaranteed provider and mark the others "experimental" in `apps/docs/pages/providers.md` instead of verifying.
- **Verification:** one recorded live run per provider; doc page states verified date.

#### U6. Cross-browser blur and permission handcheck

- **Status:** deferred. Evidence: Playwright covers Chromium, Firefox and WebKit with fake mics; the manual blur/permission pass on real devices (Safari, iOS) is not recorded.
- **Dependencies:** none.
- **Difficulty:** easy.
- **Feasibility:** needs a Mac and an iPhone.
- **Simpler alternative:** document Safari/iOS as "best effort" and publish the handcheck as a checklist in `packages/web/README.md`.
- **Verification:** checklist filled with date and device.

#### U7. Python SDK: CI and distribution

- **Status:** built; CI-gated, distribution open. Evidence: `sdks/python` (`client.py`, `cli.py`, `mcp_server.py`); its plan (KTD5) left CI as a follow-up. CI now has a `python-sdk` job (`uv sync --frozen && uv run pytest`, 36 tests); PyPI distribution is still open.
- **Dependencies:** none.
- **Difficulty:** easy.
- **Feasibility:** PyPI name availability for `wordink` is unchecked.
- **Simpler alternative:** ship via `uvx --from git+https://github.com/duketopceo/WordInk#subdirectory=sdks/python wordink` and skip PyPI.
- **Verification:** a `uv run pytest` job in `.github/workflows/ci.yml` (change to be made in this unit, not now).

#### U8. Retire or freeze `legacy/`

- **Status:** open. Evidence: `legacy/wordink/*.py` is about 2,000 lines of Windows-only Python; `AGENTS.md` keeps it "until WordInk Desktop replaces it"; with U9 decided for standalone, `legacy/` stays until U13 ships the Windows replacement; CI runs a `legacy` job on every PR.
- **Dependencies:** U9 (decided Oct 24, 2026 — B + B′, so keep `legacy/` until the standalone desktop app exists).
- **Difficulty:** easy.
- **Feasibility:** the legacy data directory `~/.groq_flow/` is user data and must not be renamed; the MIT credit to groq_flow must stay in `LICENSE`.
- **Simpler alternative:** move it to a `legacy` branch/tag and delete the directory plus the `legacy` job; or just drop the CI job and mark the directory unmaintained.
- **Verification:** CI green without the job; README layout table updated.

#### U9. Desktop direction (owner decision)

- **Status:** decided Oct 24, 2026 — standalone first-party apps on Linux, Windows, macOS and iOS (B + B′ extended to iOS), each working on default open-source configuration. Evidence: recorded in `ROADMAP.md` Phase 2 note + Phase 3/4 rows; issue #14 stays open as the tracking issue (U3).
- **Dependencies:** U2 (adoption data needs a published package) — unchanged for sequencing, not for the decision itself.
- **Difficulty:** see the table.
- **Feasibility:** see the table.
- **Simpler alternative:** n/a — decided.
- **Verification:** `ROADMAP.md` records the standalone direction; issue #14 rewritten or replaced with per-unit tracking issues when U13 gets its plan.

### Later

#### U10. Hosted-quality cleanup presets

- **Status:** not started. Evidence: `transform` hook exists and is documented (`apps/docs/pages/transform.md`); Wispr Flow, Superwhisper and TypeWhisper all ship LLM cleanup modes (research note).
- **Dependencies:** U2.
- **Difficulty:** medium.
- **Feasibility:** per-request LLM cost and latency; must stay off by default and route through the relay so keys never reach the browser.
- **Simpler alternative:** keep `transform` and publish two ready-made relay recipes in `examples/server-node`.
- **Verification:** demo shows before/after on a messy sample.

#### U11. Stronger local engine

- **Status:** not started. Evidence: `packages/local` wraps Moonshine in a worker; alternatives are Whisper via whisper.cpp WASM or transformers.js.
- **Dependencies:** none.
- **Difficulty:** medium.
- **Feasibility:** model download size, WebGPU support differences across browsers, licence of each model.
- **Simpler alternative:** keep Moonshine; add a documented `provider` hook so adopters bring their own engine.
- **Verification:** word-error-rate comparison on a fixed clip set.

#### U12. Mobile SDK (React Native / native)

- **Status:** deferred by demand (issue #15).
- **Dependencies:** U2 and adoption data.
- **Difficulty:** hard.
- **Feasibility:** the Rust core is sans-I/O so it can be reused, but every platform needs its own audio and text-insertion host.
- **Simpler alternative:** none needed until requested; the web component already works in mobile browsers where `getUserMedia` does.
- **Verification:** a sample app dictating into a text field on iOS and Android.

#### U13. Standalone desktop app (Linux + Windows + macOS)

- **Status:** planned — U9 decided Oct 24, 2026. Needs its own plan before code (issue #13's note: ce-brainstorm/ce-plan once the provider interface is stable — it is, so the plan can start).
- **Dependencies:** none blocking now; `legacy/` stays until the Windows target ships.
- **Difficulty:** hard.
- **Feasibility:** see Option B/B' — Wayland hotkey/injection fragmentation, macOS accessibility + notarized signing, Windows signing; mitigated by the requirement that everything works on default open-source config.
- **Simpler alternative:** none selected — U9 decided.
- **Verification:** each OS app dictates end-to-end on a clean install with only a provider key (or local engine) — no self-hosted gateway required; gateway pairing remains an optional extra.

### Simplifications found

- **`legacy/`** is the biggest removable piece (about 2,000 lines of Python plus a CI job) and is not part of the product.
- **Phase-shaped issues #4-#14** duplicate this plan and can be closed.
- **`wordink-wasm`** exists only to expose the core to JS; there is no evidence it needs simplifying, so it is left alone.

---

## Verification Contract

Run in the program worktree on 2026-10-10 (all exit 0):

| Command | Exit |
|---|---|
| `cargo fmt --all --check` | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| `cargo test --workspace` | 0 |
| `cargo build -p wordink-wasm --target wasm32-unknown-unknown --release` | 0 |
| `pnpm install --frozen-lockfile` | 0 |
| `pnpm build` | 0 |
| `pnpm typecheck` | 0 |
| `pnpm test` | 0 |

`pnpm test:e2e` and the `legacy` job were not run locally; U1 covers the CI e2e failure.

## Definition of Done

- This plan is listed first in `ROADMAP.md` and links to `docs/research/2026-10-10-landscape.md` and `DESIGN.md`.
- The owner has seen the two decisions (U4, U9) and the logo proposal.
