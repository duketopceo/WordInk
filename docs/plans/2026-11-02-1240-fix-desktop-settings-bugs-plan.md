---
title: WordInk desktop settings bug fixes (GUI test report) - Plan
type: fix
date: 2026-11-02
topic: desktop-settings-bugs
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-plan-bootstrap
execution: code
status: done
pr: [119, 121]
---

# WordInk desktop settings bug fixes (GUI test report) - Plan

## Goal Capsule

**Objective:** Every WordInk setting the GUI and CLI expose behaves correctly and explains itself in plain language, and a test instance with its own data folder can never send audio on the owner's real provider key.

**Means:** Ten targeted fixes and one small UX addition in `apps/desktop`, each with a test. The fixes come from the GUI settings test of 2026-11-02 (results kept privately).

**Product authority:** The owner asked for these fixes ("ce plan those fixes").

**Stop conditions:**
- Stop if a fix would change a settled one-app decision. Those decisions: Groq is the default when a key exists; key resolution order is env, keyring, 0600 file; one indicator style; the hotkey stops and inserts.

**Execution profile:**
- One PR. It runs **after** `feat/desktop-brand` merges, because both regenerate golden PNGs; merge master in first.
- Units are independent. Run them serially, or in parallel waves on disjoint files.

**Who finishes:** The agent implements and merges when green. The `--admin` flag is allowed only when a stale CodeRabbit "changes requested" is the sole blocker.

## Product Contract

### Requirements

- R1. When `HEX_APPLICATION_SUPPORT_DIR` is set, provider keys resolve only from that folder's key files. Env and the OS keyring are used only if `WORDINK_ALLOW_SYSTEM_KEYS=1`. The default install is unchanged.
- R2. No user-facing text tells users to run `cargo`. A missing local model says: "No speech model installed. Install one in Settings › Speech model, or run `wordink model install`."
- R3. The onboarding demo card and Done button are fully visible at the default window size (823 px wide) and larger.
- R4. Setting `gateway_url` rejects at once anything that isn't a valid URL using https, or http to `localhost` / `127.0.0.1`. The error names the rule.
- R5. `engine_model` is validated on set:
  - Groq, OpenAI and Deepgram: only their known models are accepted.
  - OpenRouter: an id missing from the cached catalogue is saved with a warning.
  - An empty value clears it.
- R6. `meetings.retention` can be set when it is currently unset.
- R7. The Sounds pane shows a stored volume at its true value (0.8 shows as 80%), and slider snapping keeps the stored value.
- R8. The Usage empty state mentions History only when History is off. Otherwise it says there are no dictations in the selected period.
- R9. Setting `cancel_hotkey` to a chord any other WordInk shortcut uses (dictation, paste-last, raw dictation, capture/Annotate) is refused with `shortcut-in-use`. The same check applies the other way round.
- R10. `dictation_hotkey none` unbinds the shortcut, if the shortcut model allows an unbound value; otherwise it fails with "use a chord such as alt+space, or unbind it in Settings". "option+none" must never appear in a message.
- R11. General shows the active engine at the top, for example "Engine: Groq (auto)" or "Engine: Parakeet v3 (local)", linking to Advanced › Cloud engine.

### Scope Boundaries

- Out of scope: the `hex paste-last` label (the brand PR fixes it), new settings, and layout redesign.

## Planning Contract

### Key Technical Decisions

- KTD1. R1 is implemented in `src/keys.rs` resolution (`resolve`, `resolve_checked`, `keyring_get`) by checking whether `app_paths` has a custom support dir. The check keys off the env var, which already exists. File-store keys keep living in the support dir, so test instances can still set dummy keys there.
- KTD2. URL and model validation (R4, R5) lives in the settings-set path in `src/control/settings.rs`, beside the existing choice validation, so the GUI and CLI share it. The GUI rows already go through the same apply.
- KTD3. R9 extends the existing `shortcut-in-use` conflict check to include `cancel_hotkey` and `capture.hotkey`, rather than adding a second checker.

### Sequencing

U1-U6 are independent. U7 (goldens) runs last.

## Implementation Units

| U-ID | Title | Key files | Depends on |
|---|---|---|---|
| U1 | Key isolation for custom support dirs (R1) | `src/keys.rs`, `src/app_paths.rs` | none |
| U2 | Plain model-missing message (R2) | `src/transcription/local_transcriber.rs` and any banner text | none |
| U3 | Onboarding fits at 823 px (R3) | `src/rune_ui/onboarding*`, `src/rune_ui/onboarding_kit/*` | none |
| U4 | Settings validation: gateway_url, engine_model, meetings.retention, hotkey none (R4, R5, R6, R10) | `src/control/settings.rs`, `src/app_settings.rs` | none |
| U5 | Cancel/capture conflicts (R9) | `src/control/settings.rs` | U4, same file, so serial |
| U6 | Sounds %, Usage empty state, Engine row (R7, R8, R11) | `src/rune_ui/sounds.rs`, `src/rune_ui/usage.rs`, `src/rune_ui/general.rs` | none |
| U7 | Goldens and full verification | `tests/golden/**` | U1-U6 |

### U1. Key isolation (R1)
- **Approach:** In `resolve` / `resolve_checked`, when a custom support dir is active and `WORDINK_ALLOW_SYSTEM_KEYS` is not `1`, skip the env and keyring sources. `keys status` reports the source used.
- **Tests:**
  - a custom dir plus a keyring or env key resolves to none;
  - a custom dir plus a file key resolves to the file;
  - the opt-in restores the full order;
  - the default dir is unchanged (env, then keyring, then file).

### U2. Model-missing message (R2)
- **Approach:** Replace the dev hint at `local_transcriber.rs:294`, and anywhere else `rg "cargo run"` finds in user-facing strings, with the R2 text. Add the i18n keys.
- **Tests:** a no-model error contains "Settings › Speech model" and not "cargo".

### U3. Onboarding fit (R3)
- **Approach:** Find the fixed sizes or overflow in the onboarding demo card and the footer button layout. Make the card shrink and scroll, and keep Done pinned inside the window.
- **Tests:** an onboarding scene golden at 823 px shows the Done button inside its bounds. Add a hit-box assertion if the harness supports one.

### U4. Settings validation (R4, R5, R6, R10)
- **Approach:**
  - URL parse via the `url` crate, which is already a dependency.
  - A model allowlist per provider, taken from the bundled price/model table in `cloud.rs`; OpenRouter uses the catalogue cache.
  - Fix the `meetings.retention` "is missing" path, so set works from an unset value.
  - For `none`: support unbinding if `HotkeyBinding` allows it; otherwise give the clear error.
- **Tests:** one each for the accept and refuse cases, and the OpenRouter warning path.

### U5. Hotkey conflicts (R9)
- **Tests:**
  - cancel equal to capture is refused;
  - cancel equal to dictation is refused;
  - capture equal to cancel is refused;
  - unrelated chords are accepted.

### U6. Small UI (R7, R8, R11)
- **Approach:**
  - The volume label rounds the stored value (`(v*100).round()`), and the slider steps align with stored values.
  - Usage empty-state copy follows the history setting.
  - The General engine row reads `cloud::chosen_provider()` / `engine` and the local model name.
- **Tests:** pane tests for each case.

### U7. Goldens and verification
- **Approach:** Regenerate affected goldens with `HEX_UPDATE_GOLDENS=1 cargo test goldens`, then spot-check General, Sounds, Usage and Onboarding.

## Verification Contract

- `cd apps/desktop && cargo clippy --locked --bin wordink --tests -- -D warnings && cargo test --locked`.
- The Desktop CI workflow is green on all 4 platforms.
- A manual check with a temp `HEX_APPLICATION_SUPPORT_DIR`: `wordink keys status` shows groq unset, even though the owner's keyring has a key.

## Definition of Done

- R1-R11 hold, each with a test.
- CI is green and the PR is merged.
- The owner's installed app is rebuilt with `apps/desktop/scripts/install-linux.sh`, and its settings are kept.
- No scratch code is left behind.
