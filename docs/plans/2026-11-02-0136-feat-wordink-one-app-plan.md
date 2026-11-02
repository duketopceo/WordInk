---
title: WordInk one app (Hex base, Voxtype parity, open-source ready) - Plan
type: feat
date: 2026-11-02
topic: wordink-one-app
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-brainstorm
execution: code
---

# WordInk one app (Hex base, Voxtype parity, open-source ready) - Plan

## Goal Capsule

**Objective:** Anyone on Linux, Windows or macOS can install one WordInk desktop app, press a hotkey, see a recording indicator while they speak, and get their words typed into the focused app. It works through Groq by default, or with no API key at all through a downloaded local model. The owner can uninstall Voxtype and Hex because WordInk does everything he used them for.

**Means:** Copy Hex v3.2.0's MIT source into `apps/desktop/` as the app base (KTD1). Hex already ships most Voxtype parity. Add:
- cloud engines with Groq as the default, plus cost tracking;
- a Voxtype audio-bar indicator style;
- WordInk's providers, gateway and meeting mode.

Re-implement TypeWhisper ideas from scratch. Clean the repo to open-source standards along the way.

**Product authority:** The owner (repo maintainer) made the decisions in this session, recorded under Key Decisions. This plan supersedes `docs/plans/2026-11-01-0001-feat-wordink-convergence-plan.md` as the active convergence plan.

**Stop conditions:**
- Stop and report if Hex v3.2.0 cannot be built or run on Linux aarch64 (U1). Every later unit depends on it.
- Stop and report if a settled decision proves infeasible.
- Stop and report if any step would touch the owner's running `wordink-dictd.service` before U5's switch-over checks pass.
- Stop and report if the history rewrite (U18) cannot first produce a verified backup.

**Execution profile:** Run phases in order. Execute one phase at a time with `ce-work`, opening one PR per unit (or a small set per phase) against `master`. Merge when green under KTD9. Phase 1 (U1-U5) is the first execution scope.

**Who finishes and ships:** The agent implements, reviews, merges under KTD9, and installs for the owner. The owner does the Windows and macOS device passes and any action auto mode blocks. Those are listed in `~/Downloads/claude/wordink-overnight-report.md`, outside the repo.

**Open blockers:**
- macOS notarization and Windows Authenticode need paid accounts. They are deferred by decision (R30), so they don't block.
- The device acceptance pass on the owner's Windows PC and on a real Mac needs the owner present.

## Product Contract

### Summary

WordInk becomes one cross-platform desktop app that replaces Voxtype and Hex:
- Hex's interface, engine and updater form the base.
- Voxtype's text clean-up is added.
- One recording indicator in a choice of three styles (Hex pill, Voxtype audio bar, TypeWhisper-style live text) and TypeWhisper-style usage stats are added.
- Groq is the default and Parakeet v3 is the no-key default.

The repo is cleaned of personal details, legacy code and drift, and meets an open-source bar.

### Problem Frame

The owner does not use WordInk day to day because it has no Hex-style pill and no Voxtype-style audio popup. The Python daemon has no GUI. It is Groq-only. It lacks the replacements, filler removal and media pause he had in Voxtype. Its Linux path works only on Hyprland. The repo carries a superseded `legacy/` app, contradictory plans, and personal details in a public repo, and lacks the basics a stranger needs to trust and install it.

### Requirements

**App base**
- R1. The desktop app is built from Hex's source, copied into this repo with its MIT notice kept and the source commit recorded. It is not a GitHub fork.
- R2. A documented, repeatable way exists to pull a later Hex change into the copy on purpose.
- R3. One app per user runs the window, pill, engine and hotkey on Linux (any Wayland desktop), Windows and macOS (Apple silicon).
- R4. The Python daemon `wordink-dictd` is retired once the app matches it on each OS. Until then it stays installable and is not broken.

**Dictation surface**
- R5. The default dictation hotkey is Option+Space on Mac keyboards and Alt+Space on PC keyboards, and it can be changed in settings.
- R6. The app detects the keyboard type and labels the hotkey to match.
- R7. While dictating, the app shows one recording indicator. The user picks its style in settings, and only one shows at a time:
  - (a) Hex's pill;
  - (b) Voxtype's audio-bar popup, which moves with the voice;
  - (c) TypeWhisper-style live text.

  Hex's pill is the default. Synth feedback sounds for start, stop and error are a separate, optional setting.
- R8. Live text (style c) streams words as they are recognised for models that can stream. With other models it, like the other styles, shows the phases: ready, loading model, recording, transcribing, done, copied, error.
- R9. A dictation can be cancelled with a dedicated, rebindable Cancel shortcut, or by pressing Esc twice while the indicator shows. Pressing the dictation hotkey again stops and inserts.
- R10. Text is inserted into the focused app with terminal-safe paste and clipboard restore, and is never typed into a detected password field.
- R11. A dictation that fails is kept and can be recovered from history.

**Text processing (Voxtype parity)**
- R12. Spoken-to-written replacements, including importing an existing Voxtype replacement table.
- R13. Filler-word removal and spoken punctuation, each switchable.
- R14. Silence-only clips are rejected before transcription.
- R15. Media playing on the machine pauses or ducks while recording and resumes after.
- R16. Per-app modes: correction and rewrite profiles chosen by the focused app.
- R17. A vocabulary of terms with "sounds like" spellings, used for biasing where the engine supports it.

**Engines and models**
- R18. Groq is the default cloud engine. A Groq key is set in onboarding.
- R19. Every OpenRouter model that accepts audio input is offered. The list is read from OpenRouter's model catalogue, not hard-coded.
- R20. Local engines run through Hex's transcribe.cpp. Parakeet TDT 0.6B v3 (CC-BY-4.0) is the default local model, downloaded on first use. Whisper large-v3-turbo and Qwen3-ASR are also offered.
- R21. The model picker shows each model's license. Models under non-open licenses can be chosen but are never the default.
- R22. The app detects the hardware (GPU API such as Vulkan or Metal, memory, CPU features) and recommends local or cloud and a model to match.
- R23. The existing gateway and the bring-your-own OpenAI and Deepgram providers keep working as options.
- R24. Meeting mode, with mic plus system audio and source-based speaker labels, keeps working.

**Usage and history**
- R25. A usage page shows words dictated, time saved, sessions, and estimated cost per provider and model, TypeWhisper-style.
- R26. History is searchable, has a retention setting, and can re-transcribe an entry with another model.

**Settings and onboarding**
- R27. A sidebar settings window has panes for General, Hotkey, Models, Modes, Vocabulary, Replacements, Sounds, History, Usage, Privacy and About.
- R28. First run walks through permissions, choosing cloud or local, a key or a model download, and a test dictation.

**Distribution**
- R29. Releases are built in CI for all three OSes from tags, with checksums, an SBOM and build attestations.
- R30. Linux builds are signed (ed25519, Hex's scheme) and the updater verifies them. macOS notarization and Windows Authenticode are deferred, and the docs say so.
- R31. Linux installs work on any Wayland desktop. Omarchy also gets its bar module and binds.

**Repo clean-up and open-source standards**
- R32. All personal and machine-specific details are removed from current files and from git history. That covers owner hostnames, home paths, personal email, name and booking links in fixtures, private tool hooks, and private tracker ticket ids.
- R33. `legacy/` is deleted when nothing depends on it. Superseded plans move to `docs/archive/`. README, ROADMAP, AGENTS, CONCEPTS and DESIGN describe the one-app product truthfully.
- R34. The repo has:
  - THIRD_PARTY_NOTICES covering code (Hex, Voxtype, transcribe.cpp, ggml) and models (Parakeet v3 and others);
  - SECURITY with a private reporting channel;
  - CONTRIBUTING with DCO sign-off;
  - a code of conduct and issue templates;
  - CodeQL and secret scanning;
  - Dependabot for every package root;
  - correct CODEOWNERS.
- R35. TypeWhisper (GPL-3.0) is used for ideas only. No code, strings or assets are copied, and a clean-room note records that.
- R36. The web SDK, the gateway and the iOS app remain separate products in the repo. They are cleaned but not merged into the app.

**Tracking**
- R37. When the work lands, the public ROADMAP, the GitHub issues and the owner's private tracker are updated to match. The private tracker keeps ticket ids out of the public repo.

### Key Decisions

- Copy Hex's code into this repo as the app base (session-settled: user-directed — chosen over keeping the Python daemon with a new Tauri or Qt GUI, and over a GitHub fork: Hex takes no PRs, so a copy gives one repo, one CI and the full Hex GUI on day one).
- Parakeet TDT 0.6B v3 is the default local model (session-settled: user-approved — chosen over Hex's default Parakeet Unified under NVIDIA's custom license and over Whisper turbo: every default stays under a standard open license).
- Linux means any Wayland desktop, with Omarchy extras (session-settled: user-approved — chosen over Omarchy-only and over full X11 parity: most Linux users can run it without a second input path).
- Sign Linux now and defer paid macOS/Windows signing (session-settled: user-directed — chosen over paying for signing now and over no signing: Linux gets a verified updater at no cost).
- Rewrite git history to remove personal details (session-settled: user-directed — chosen over cleaning current files only: the repo is public and history is readable). It is one force-push after a backup, at the end of the run.
- Clean-up scope is all four audit items: delete `legacy/`, archive plans and fix docs, scrub personal details, add the open-source basics (session-settled: user-directed — chosen from the audit's recommendation list).
- Groq is the baseline engine; Option+Space is the default hotkey, changeable, with keyboard auto-detect (session-settled: user-directed — chosen over Hex's local-first default and over keeping the owner's current binds as the default).
- One indicator with three selectable styles, not all at once; Hex's pill is the default (session-settled: user-directed — chosen over showing pill, audio bar and live text together). The indicator ships first, because its absence is what stops the owner using WordInk.
- The PR process is merge-when-green: the agent may squash-merge its own PRs when CI passes and review has no open blocking finding, and may use `--admin` only when a stale CodeRabbit change request is the sole block (session-settled: user-directed — chosen over stacking for the owner to merge).

### Key Flows

- F1. **First run** (covers R18, R20, R22, R28): install, grant permissions, see the hardware recommendation, choose Groq (paste a key) or local (download Parakeet v3 with progress), set or accept the hotkey, then do a test dictation.
- F2. **Dictate** (covers R5, R7, R8, R10, R12-R15): press Option/Alt+Space. Media pauses and the chosen indicator appears (live text streams words where the model supports it). Release or press again: the text is processed and inserted into the focused app, media resumes, and the pill shows done.
- F3. **Cancel or recover** (covers R9, R11): press the Cancel shortcut, or Esc twice while the indicator shows, and nothing is inserted. If the provider fails, the clip is kept and History offers retry with another model.

### Acceptance Examples

- AE1. (covers R7, R8) With the default pill, dictating "hello world" with Groq on Omarchy shows the pill through record, transcribe and done, and "hello world" lands in a foot terminal; with the live-text style, Groq shows the phase labels, and a streaming-capable local model on a GPU shows the words as they stream.
- AE2. (covers R20, R22) On a machine with no API key and Vulkan available, onboarding recommends Parakeet v3, and dictation works offline after the download.
- AE3. (covers R12, R13) With the owner's Voxtype replacement table imported, a spoken replacement term comes out written, and "um" is dropped when filler removal is on.
- AE4. (covers R10) Dictating while a password manager field is focused inserts nothing and shows why.
- AE5. (covers R32) A search of `master` and its full history for the owner's hostnames, email and personal booking link finds nothing.

### Success Criteria

- The owner uses WordInk daily on Omarchy and has uninstalled Voxtype (including its post-update reinstall hook) and Hex.
- A stranger installs from the README on GNOME, Windows or an Apple-silicon Mac and dictates within five minutes, with a Groq key or with no key.
- CI is green on all three OSes. A tagged release produces signed Linux artifacts and unsigned macOS/Windows artifacts with checksums and attestations.

### Scope Boundaries

- Deferred for later:
  - speed and accuracy benchmarks of local models;
  - NVIDIA and Microsoft niche models;
  - multi-speaker meeting transcription (ML diarization);
  - Hex's macOS-only Meetings and Labs;
  - macOS notarization and Windows Authenticode;
  - iOS feature parity.
- Out of scope:
  - TypeWhisper's plugin marketplace, sync and device management;
  - Voxtype's other nine engines, its evdev hotkey reader, its TUI and its updater;
  - X11 parity;
  - Intel Macs, unless a cheap fallback appears.

### Dependencies / Assumptions

- Hex v3.2.0 builds on Asahi aarch64 with Vulkan (Honeykrisp). This is unverified, so verify it first.
- The Hex window needs a Vulkan driver on Linux and Windows (a software driver such as Mesa lavapipe counts) and AVX2 on x86. Without a GPU, the window runs on the software driver and onboarding recommends Groq (KTD8); without any Vulkan driver or AVX2 the app exits with a message.
- OpenRouter audio-input models are discoverable through its model API.

### Outstanding Questions

- Where the Python daemon's Windows and macOS adapters are better than Hex's (the audit names the Windows insert policy), port that behaviour into the app.
- Exact look of each indicator style: start from Hex's pill, Voxtype's bar and TypeWhisper's overlay states; design settles details during the first unit.

### Sources / Research

Private research notes (not committed; they contain machine-specific detail) informed this plan:
- a grounding dossier;
- a Hex GUI teardown;
- a TypeWhisper idea list, recorded as clean-room notes;
- a Voxtype take-list;
- an open-source standards checklist;
- a repo audit.

Public sources:
- github.com/anomalyco/hex (MIT, v3.2.0);
- Voxtype 1.1.1 (MIT);
- github.com/TypeWhisper (GPL-3.0, ideas only);
- the Hugging Face model cards for Parakeet TDT 0.6B v3 (CC-BY-4.0), Whisper large-v3-turbo and Qwen3-ASR (Apache-2.0).

## Planning Contract

### Key Technical Decisions

- KTD1. Import Hex v3.2.0 (tag commit `00610c581a0501e02631b7d915d504dec3729b03`) into `apps/desktop/` as its own Cargo workspace. Leave out `site/`, `ios/`, `prototypes/`, `skills/`, `release-notes/`, `flake.*` and `nix/`. Record the source commit and the excluded paths in `apps/desktop/UPSTREAM.md`. The root `Cargo.toml` workspace must not list or glob `apps/desktop`. (session-settled: user-directed — chosen over a GitHub fork and over a Python + Tauri/Qt GUI: Hex takes no PRs, and a copy gives one repo, one CI and the full Hex GUI on day one.)
- KTD2. `scripts/hex-sync.sh <rev>` pulls a later Hex change into a review branch on purpose. It does this by diffing Hex `v3.2.0..<rev>` restricted to the imported paths and applying that diff under `apps/desktop/`. Conflicts are left for review. (Covers R2.)
- KTD3. Rebrand at the product layer only:
  - Rename the binary, app id, window and tray titles, data and config directories (`wordink`, never `hex`), the update URLs and the icons.
  - Leave internal module names alone, so later Hex syncs stay small.
  - Keep "Based on Hex by Anomaly (MIT)" in About and the notices.
  - The renamed app must coexist with an installed Hex and with `wordink-dictd`.
- KTD4. Cloud transcription is a new engine family beside transcribe.cpp. It uses an OpenAI-compatible `/audio/transcriptions` client over Hex's existing `ureq`/native-tls `net` layer. Providers:
  - Groq: the default model is `whisper-large-v3-turbo`.
  - OpenRouter: audio-input models are discovered from `GET https://openrouter.ai/api/v1/models`, filtered on `architecture.input_modalities` containing `audio`, and called through chat completions with `input_audio`.
  - OpenAI.
  - Deepgram.
  - The WordInk gateway.

  Keys come from the OS keyring (Secret Service, Keychain, Credential Manager). Where Linux has no Secret Service, keys go in a user-only (0600) file in the WordInk config directory, separate from settings, and onboarding says where the key is stored. Environment variables (`GROQ_API_KEY`, `OPENROUTER_API_KEY`) override both. Keys are never written to the settings file or logs.
- KTD5. Every transcription records provider, model, audio seconds, words and estimated cost into history. Prices come from a bundled table for Groq, OpenAI and Deepgram, and from the OpenRouter catalogue `pricing` field, cached daily. The Usage pane aggregates these records. (Covers R25.)
- KTD6. The indicator style is one setting: `pill` (Hex's HUD, the default), `audio_bar` or `live_text`.
  - `audio_bar` is a new Rune HUD view that draws a level meter from the existing capture levels, in the shape of Voxtype's popup. It is re-implemented, not copied.
  - `live_text` reuses Hex's `live_text` path. Where the engine cannot stream, it falls back to showing the phases.
  - Feedback sounds stay Hex's sound sets. A "synth" set may be added later.
- KTD7. The defaults change to:
  - Groq as the engine when a Groq key exists. Otherwise, Parakeet TDT 0.6B v3, local.
  - Option+Space on macOS and Alt+Space elsewhere.

  The keyboard type is detected from Hex's `keyboard_input_sources` and layout data, so labels read Option or Alt. GNOME binds Alt+Space to its window menu, so GNOME sessions default to Ctrl+Alt+Space, and the label says so. Windows keeps Alt+Space unless the owner's Windows device pass finds a conflict. Hex's Parakeet Unified stays in the catalogue, labelled with its license, and is never the default. (Covers R5, R6, R18, R20, R21.)
- KTD8. Hardware recommendation reuses Hex's Auto device logic (the GPU/CPU probe, and remembered hangs) and adds the RAM and CPU-feature probe. Onboarding shows one recommended choice and one alternative. Machines without Vulkan or AVX2 are recommended Groq. "No Vulkan" here means no GPU Vulkan device: the window still runs on a software Vulkan driver (Mesa lavapipe), which the docs list as the fallback. With no Vulkan driver at all, or no AVX2, the app exits with a message naming the fix. A no-window CLI mode is deferred. (Resolves the open area on fallback.)
- KTD9. Merge when green. The agent may squash-merge its own PRs when CI passes and review has no open blocking finding, and may use `--admin` only when a stale CodeRabbit CHANGES_REQUESTED is the sole block. (session-settled: user-directed — chosen over stacking every PR for the owner to merge: overnight progress.)
- KTD10. The owner switches over on Linux in U5:
  1. Install the app beside `wordink-dictd`.
  2. Smoke-test it on a spare hotkey.
  3. Move Alt+Space, plus the owner's existing dictation binds, to `wordink`.
  4. Stop and disable `wordink-dictd.service` only after a successful dictation with the app.

  Roll back by re-enabling the unit and restoring the binds from the Omarchy stash. `wordink-dictd` stays packaged for Windows and macOS until those platforms reach parity. (Resolves the open area on dictd.)
- KTD11. Meeting mode: Hex's Meetings UI is marketed macOS-only, but its source has a Linux and Windows meetings runtime. Enable and exercise that runtime first. Then port only the proven gaps from `wordink-dictd` (source-based speaker labels, markdown export) into Hex's meeting modules, not a parallel module. ML diarization stays out. (Covers R24.)
- KTD12. Personal details are scrubbed in two passes:
  - in files, in U15;
  - in history, once, in U18, using `git filter-repo --replace-text` with a private expressions file kept outside the repo. Before that rewrite, take a verified `git bundle` backup and confirm it with `git bundle verify`.

  (session-settled: user-directed — chosen over cleaning current files only: the repo is public.)
- KTD13. Default local model is Parakeet TDT 0.6B v3 (session-settled: user-approved — chosen over Hex's Parakeet Unified and over Whisper turbo: open license by default). Linux targets any Wayland desktop via Hex's portal paths (session-settled: user-approved — chosen over Omarchy-only and X11 parity). Linux releases are ed25519-signed now, macOS and Windows signing deferred (session-settled: user-directed — chosen over paid signing now and over none).

### High-Level Technical Design

```
apps/desktop/  (Hex v3.2.0 copy, renamed WordInk)
├── capture → VAD → engine router ─┬─ transcribe.cpp (Parakeet v3 default local, Whisper turbo, Qwen3-ASR)
│                                  └─ cloud (Groq default | OpenRouter audio | OpenAI | Deepgram | gateway)   [new, KTD4]
├── text: fillers, spoken formatting, replacements (+ Voxtype import), modes, vocabulary   [Hex; import new]
├── output: paste (wtype / RemoteDesktop portal / platform) + clipboard restore            [Hex]
├── indicator: pill | audio_bar [new] | live_text        sounds: Hex sound sets           [KTD6]
├── history + usage/cost pane [new]   meeting mode (port from dictd) [new, KTD11]
└── settings window + onboarding + tray + signed updater (WordInk endpoints)              [Hex, rebranded]
sdks/python (wordink-dictd)  → kept until per-OS parity, then retired (R4)
packages/* (web SDK, gateway) and apps/ios → separate products, cleaned (R36)
```

### Assumptions

- Hex v3.2.0 builds on Linux aarch64. Hex's Linux beta lists ARM64 and Hyprland, but it is unverified on Asahi with Honeykrisp Vulkan. U1 verifies it.
- The Rust toolchain on the build host is new enough for Hex's `edition = "2024"`. The host has cargo 1.98.
- OpenRouter exposes `input_modalities` in its models API. U7 falls back to a bundled list if it does not.

### Sequencing

| Phase | Units | Ships |
|---|---|---|
| 1 | U1-U5 | Hex imported, renamed, Groq engine, indicator styles, installed for the owner on Linux |
| 2 | U6 | Voxtype parity verified + replacements import |
| 3 | U7-U11 | providers, models, hardware, hotkey, meeting mode |
| 4 | U12-U13 | usage/history/recovery, settings + onboarding |
| 5 | U14 | CI/CD 3 OSes + signed Linux releases |
| 6 | U15-U17 | cleanup, OSS basics, dictd retirement on Linux |
| 7 | U18-U19 | history rewrite, roadmap/issues/tracker |

## Implementation Units

| U-ID | Title | Key files | Depends on |
|---|---|---|---|
| U1 | Import Hex v3.2.0 | `apps/desktop/**`, `apps/desktop/UPSTREAM.md`, `Cargo.toml` | — |
| U2 | Rebrand to WordInk | `apps/desktop/src/app_paths.rs`, `apps/desktop/Cargo.toml`, `apps/desktop/src/desktop/*`, `apps/desktop/resources/**` | U1 |
| U3 | Cloud engine + Groq default | `apps/desktop/src/transcription/*`, `apps/desktop/src/net*`, `apps/desktop/src/app_settings.rs` | U2 |
| U4 | Indicator styles | `apps/desktop/src/dictation/indicator.rs`, `apps/desktop/src/linux/rune_hud.rs`, `apps/desktop/src/rune_ui/*` | U2 |
| U5 | Owner install + Linux switch-over | `apps/desktop/scripts/install-linux.sh`, `examples/omarchy-desktop/` | U3, U4 |
| U6 | Voxtype parity + replacements import | `apps/desktop/src/dictation/text_replacements.rs`, `apps/desktop/src/main.rs` | U5 |
| U7 | OpenRouter, OpenAI, Deepgram, gateway providers | `apps/desktop/src/transcription/cloud/*` | U3 |
| U8 | Model catalogue defaults + license display | `apps/desktop/src/transcription/models.rs`, `apps/desktop/src/rune_ui/models.rs` | U2 |
| U9 | Hardware recommendation | `apps/desktop/src/desktop/onboarding.rs`, `apps/desktop/src/transcription/engine.rs` | U8 |
| U10 | Option/Alt+Space default + keyboard detect | `apps/desktop/src/keyboard.rs`, platform input files | U2 |
| U11 | Meeting mode on Linux/Windows | `apps/desktop/src/meeting*` | U3 |
| U12 | Usage/cost, history re-transcribe, recovery | `apps/desktop/src/history*`, `apps/desktop/src/rune_ui/history.rs`, new usage pane | U3, U7 |
| U13 | Settings panes + onboarding | `apps/desktop/src/rune_ui/settings.rs`, `apps/desktop/src/rune_ui/onboarding*` | U9, U12 |
| U14 | CI/CD + signed Linux releases | `.github/workflows/desktop.yml`, `.github/workflows/release.yml`, `apps/desktop/packaging/**` | U2 |
| U15 | Repo cleanup + scrub files | `legacy/`, `docs/archive/`, `README.md`, `ROADMAP.md`, `AGENTS.md`, `CONCEPTS.md`, `DESIGN.md`, fixtures | U5 |
| U16 | Open-source basics | `THIRD_PARTY_NOTICES.md`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `.github/**` | U1 |
| U17 | Retire dictd on Linux | `sdks/python/**` docs, `examples/omarchy-dictation/` | U5, U6 |
| U18 | History rewrite | (git history; private expressions file outside repo) | U15-U17 |
| U19 | Roadmap, issues, tracker | `ROADMAP.md`, GitHub issues, private tracker | U18 |

### U1. Import Hex v3.2.0
- **Goal:** Hex's source builds and runs from `apps/desktop/` in this repo.
- **Requirements:** R1, R2, R3.
- **Files:** `scripts/hex-sync.sh`, `apps/desktop/**` (copied tree), `apps/desktop/UPSTREAM.md`, `apps/desktop/LICENSE` (Hex MIT, unchanged), and root `Cargo.toml` (a comment only, if needed, to keep `apps/desktop` out of the root workspace).
- **Approach:**
  1. Copy the Hex tree at tag commit `00610c5` from a scratch clone, without `.git` and with the KTD1 exclusions, including `.cargo/config.toml`.
  2. Verify that `cargo build --release` in `apps/desktop` succeeds on Linux aarch64 with Vulkan, and that the binary opens its window under Hyprland. This is the prerequisite gate; stop per the Stop conditions if it fails.
  3. Commit the import as a single commit whose message names the source commit.
  4. Add `scripts/hex-sync.sh` per KTD2 (covers R2), with a dry-run test against a known Hex tag.
- **Test scenarios:**
  - `cargo test` (Hex's default set) passes, or its failures are pre-existing upstream failures and are listed.
  - The root `cargo build -p wordink-core` is unaffected.
- **Verification:** the build log, a window screenshot saved privately, and `UPSTREAM.md` listing the source commit and exclusions.

### U2. Rebrand to WordInk
- **Goal:** The app presents as WordInk and never collides with an installed Hex or with `wordink-dictd`.
- **Requirements:** R1, R3.
- **Files:**
  - `apps/desktop/Cargo.toml` (`[[bin]]` name `wordink`, package name);
  - `apps/desktop/src/app_paths.rs`;
  - user-visible strings in `apps/desktop/src/desktop/**`, `apps/desktop/src/rune_ui/**` and the i18n tables;
  - icons and desktop entries under `apps/desktop/resources/**` and `apps/desktop/packaging/**`;
  - the update endpoint constants.
- **Approach:**
  - Rename at the product layer only, per KTD3.
  - Point the data and config directories at `wordink`.
  - Point the updater at GitHub Releases of this repo (wired in U14).
  - Add an About line crediting Hex.
  - Inventory every outbound host in the imported tree. Disable or compile out features that phone Anomaly or third parties, or that capture the screen by default: `hex_capture`/screen recording, the opencode.ai router, and release-service check-ins. Keep QuickJS mode scripts, but without network access unless the user enables it. Record the inventory in `apps/desktop/UPSTREAM.md`.
  - Use WordInk's brand assets where they exist; otherwise a neutral placeholder icon.
- **Test scenarios:**
  - The app's data dir is `…/wordink`, not `…/hex`.
  - The single-instance socket name differs from Hex's.
  - Hex's own tests still pass after the renames.
- **Verification:** `rg -n '"hex"' apps/desktop/src/app_paths.rs` finds no product-path uses. The app and an installed `hex` can run at the same time.

### U3. Cloud engine + Groq default
- **Goal:** Dictation works through Groq with no local model, and Groq is the default when a key exists.
- **Requirements:** R18, R23 (partly), R8.
- **Files:**
  - a new `apps/desktop/src/transcription/cloud.rs` (or a `cloud/` dir);
  - `apps/desktop/src/transcription/engine.rs`;
  - `apps/desktop/src/app_settings.rs`;
  - key storage via the OS keyring.
- **Approach:**
  - Add the OpenAI-compatible multipart client and the Groq provider per KTD4.
  - Register the provider as an engine choice in settings and in the model picker.
  - Record usage per KTD5. The pane comes in U12.
  - Give errors (401, 429, 5xx, no network) clear pill and notification text, and keep the clip for recovery (R11).
- **Test scenarios:**
  - Use a mock HTTP server, never a live call.
  - A multipart request carries the model and audio.
  - 401 shows "Groq key rejected".
  - 429 shows a retry hint.
  - The clip is kept on failure.
  - The default engine resolves to Groq only when a key exists.
- **Verification:** unit tests pass. One live smoke dictation on the owner's machine uses the owner's Groq key from his secret store, injected into the environment and never echoed (minimal spend).

### U4. Indicator styles
- **Goal:** Exactly one indicator shows while dictating, in the style the user picked.
- **Requirements:** R7, R8, R9.
- **Files:**
  - `apps/desktop/src/dictation/indicator.rs`;
  - `apps/desktop/src/linux/rune_hud.rs`, plus the Windows and macOS HUD hosts;
  - `apps/desktop/src/rune_ui/hud_*.rs`;
  - `apps/desktop/src/app_settings.rs`.
- **Approach:**
  - Add the `indicator_style` setting per KTD6.
  - Implement the `audio_bar` view from capture levels.
  - Wire `live_text` with the phase fallback.
  - Keep Hex's double-Esc cancel and hotkey cancel.
- **Test scenarios:**
  - Each style renders through Hex's HUD story/snapshot tests.
  - Only one HUD surface exists at a time.
  - Live text falls back to phase labels for a non-streaming engine.
- **Verification:** HUD stories pass. On the owner's machine each style shows during a dictation, with screenshots saved privately.

### U5. Owner install + Linux switch-over
- **Goal:** The owner dictates daily with the WordInk app on Omarchy.
- **Requirements:** R4, R5, R31.
- **Files:** the install script adapted from Hex's `scripts/install-linux.sh` (local-build mode), and `examples/omarchy-desktop/` (generic Hyprland binds and Waybar notes, no personal content).
- **Approach:**
  - Follow KTD10 exactly. Until U10 changes the in-app default, Alt+Space is provided as a compositor bind that runs the app's dictation command.
  - The Omarchy bar module (R31) is the app's StatusNotifierItem tray in Waybar. The example documents it, plus the optional Waybar mic indicator.
  - Edit the Omarchy bindings only through the owner's stash-canonical config: edit the stash, then copy.
  - Remove the Voxtype reinstall hook only in U17.
  - Port `wordink-dictd`'s password-field protection (R10): a focused-app deny list, plus Hex's AT-SPI focus data where the toolkit exposes a password role. Hex's paste path has no such check, so this is new work and its tests cover AE4.
- **Test scenarios:**
  - After install, Alt+Space toggles a recording into a foot terminal.
  - Disabling the app and re-enabling `wordink-dictd` restores the old behaviour.
- **Verification:**
  - A recorded smoke dictation succeeds.
  - `systemctl --user is-active wordink-dictd` reports inactive only after that success.

### U6. Voxtype parity + replacements import
- **Goal:** Everything the owner used in Voxtype works in the app.
- **Requirements:** R12, R13, R14, R15, R9.
- **Files:** `apps/desktop/src/dictation/text_replacements.rs` and a new `wordink import voxtype [--config PATH]` command in `apps/desktop/src/main.rs`.
- **Approach:**
  - Map each Voxtype feature to Hex's equivalent: fillers → `filler_words`, spoken punctuation → `spoken_formatting`/`spoken_symbols`, VAD → Hex's VAD, media → `audio_control`, sounds → sound sets, cancel → Esc and the hotkey.
  - Fill any gap.
  - The import reads Voxtype's `[text.replacements]` TOML and merges it into WordInk replacements, so it can be run more than once safely.
- **Test scenarios:**
  - Importing a fixture Voxtype config with replacements and filler settings yields equal WordInk settings.
  - Re-import adds no duplicates.
- **Verification:** tests pass. The owner's real Voxtype config imports on his machine; it is never committed.

### U7. More cloud providers
- **Goal:** OpenRouter audio models, OpenAI, Deepgram and the WordInk gateway are selectable.
- **Requirements:** R19, R23.
- **Files:** `apps/desktop/src/transcription/cloud/*` and the picker in `apps/desktop/src/rune_ui/models.rs`.
- **Approach:**
  - Implement the providers per KTD4.
  - Cache the OpenRouter catalogue daily.
  - Fall back to a bundled list when the catalogue can't be fetched.
- **Test scenarios:** with mocks only:
  - catalogue filtering keeps audio-input models;
  - the `input_audio` request shape is correct;
  - the Deepgram request shape is correct;
  - the gateway honours its bearer token and refuses plain http unless the host is localhost.
- **Verification:** unit tests pass.

### U8. Model catalogue defaults + licenses
- **Goal:** Parakeet v3 is the default local model, and every model shows its license.
- **Requirements:** R20, R21.
- **Files:** `apps/desktop/src/transcription/models.rs`, `apps/desktop/src/windows.rs` (`DEFAULT_MODEL`), `apps/desktop/src/parakeet.rs`, and `apps/desktop/src/rune_ui/models.rs`.
- **Approach:**
  - Make sure the catalogue contains Parakeet TDT 0.6B v3, Whisper large-v3-turbo and Qwen3-ASR.
  - Set the default per KTD7.
  - Add a license field and badge.
  - A guard test asserts the default model's license is in an allowlist (MIT, Apache-2.0, CC-BY-4.0).
- **Test scenarios:** the default resolves to Parakeet v3 on all OSes; the license-allowlist guard holds; every catalogue entry has a license.
- **Verification:** tests pass.

### U9. Hardware recommendation
- **Goal:** Onboarding recommends cloud or local and a model that fits the machine.
- **Requirements:** R22.
- **Files:** `apps/desktop/src/desktop/onboarding.rs`, `apps/desktop/src/transcription/engine.rs`, and a new `apps/desktop/src/hardware.rs`.
- **Approach:** Per KTD8. The probe returns `{gpu_api, vram_or_unified, ram, cpu_features}`, and a pure function maps it to a recommendation.
- **Test scenarios:** table-driven:
  - no Vulkan → Groq;
  - Vulkan with ≥8 GB → Parakeet v3;
  - x86 without AVX2 → Groq.
- **Verification:** tests pass. The owner's machine gets a sensible recommendation.

### U10. Default hotkey + keyboard detection
- **Goal:** Option+Space on Mac keyboards and Alt+Space elsewhere, labelled correctly, and changeable.
- **Requirements:** R5, R6.
- **Files:** `apps/desktop/src/keyboard.rs`, `apps/desktop/src/windows/input.rs`, the Linux shortcut paths and the macOS input host.
- **Approach:**
  - Change the defaults per KTD7.
  - Detect Apple keyboards from layout and vendor info where available, falling back to the OS.
  - Keep the user's chosen shortcut on upgrade.
- **Test scenarios:** the default per OS; the label follows the detected keyboard; an existing custom shortcut is preserved.
- **Verification:** tests pass.

### U11. Meeting mode port
- **Goal:** Meeting recording with mic plus system audio, source labels and export works on Linux and Windows.
- **Requirements:** R24.
- **Files:**
  - Hex's `apps/desktop/src/meeting*` modules;
  - reference: `sdks/python/src/wordink/desktop/meeting.py`.
- **Approach:**
  - Port the dictd behaviour per KTD11.
  - Use the chosen engine for transcription.
  - Export to markdown.
- **Test scenarios:** two-source capture is labelled by source; export writes the expected markdown from a fixture.
- **Verification:** tests pass.

### U12. Usage, history re-transcribe, recovery
- **Goal:** A usage page with words, time saved, sessions and cost; history re-transcribe; recovery of failed dictations.
- **Requirements:** R11, R25, R26.
- **Files:** `apps/desktop/src/history*`, `apps/desktop/src/rune_ui/history.rs`, and a new `apps/desktop/src/rune_ui/usage.rs`.
- **Approach:**
  - Aggregate the KTD5 records.
  - Time saved = words ÷ 40 wpm typing − audio seconds, with the formula shown in the pane.
  - Re-transcribe sends the stored clip to another engine. Sending a clip recorded with a local model to a cloud provider needs an explicit confirmation.
  - Clips follow the history retention setting (default 7 days), are stored user-only (0600), and are deleted with their history entry.
- **Test scenarios:**
  - Aggregation over fixture records.
  - The cost total per provider.
  - Re-transcribe uses the stored clip.
  - A failed dictation appears with a retry action.
- **Verification:** tests pass. The pane renders in its story test.

### U13. Settings panes + onboarding
- **Goal:** The sidebar settings cover R27, and the first run follows F1.
- **Requirements:** R27, R28.
- **Files:** `apps/desktop/src/rune_ui/settings.rs`, `apps/desktop/src/rune_ui/onboarding*` and `apps/desktop/src/desktop/onboarding.rs`.
- **Approach:**
  - Add the Usage, Replacements, Sounds and Privacy panes where Hex lacks them.
  - Onboarding offers cloud (Groq key) or local (Parakeet v3 download), using the U9 recommendation.
  - TypeWhisper ideas are re-implemented only (R35).
- **Test scenarios:** pane snapshot tests; onboarding state-machine tests for the cloud and local paths.
- **Verification:** tests pass.

### U14. CI/CD + signed Linux releases
- **Goal:** CI builds and tests the app on Linux (x86_64, aarch64), Windows and macOS. Tags produce releases with checksums, an SBOM, attestations, and ed25519-signed Linux artifacts that the updater verifies.
- **Requirements:** R29, R30.
- **Files:**
  - `.github/workflows/desktop.yml`;
  - `.github/workflows/release.yml`;
  - `apps/desktop/packaging/**`;
  - the updater public-key constant.
- **Approach:**
  - Adapt Hex's `check-*.yml` and `release.yml` to `apps/desktop`. Remove the Anomaly release-service, R2, Azure, Apple, AUR and Homebrew jobs.
  - The release job writes a schema-1 update manifest as a release asset on the same host the app checks: stable channel, `linux-x64` and `linux-arm64` targets, sha256, size, and the `-sig` entries from Hex's `sign-linux.sh`.
  - Only Linux uses the direct, ed25519-verified update path. On macOS and Windows the in-app updater is download-only: it links to the GitHub release, as Hex's `DownloadOnly` method does, until signing exists.
  - Generate the signing keypair locally. Store the private key only as a secret on a protected `release` environment that requires the owner's approval to deploy, used only by a separate signing job. CODEOWNERS covers `.github/workflows/**` and the updater key constant. Rotation steps are documented in `SECURITY.md`. Setting the secret and the environment is an owner action if auto mode blocks it.
  - Ship macOS and Windows unsigned, with that stated in the README.
- **Test scenarios:** the workflows pass actionlint; a PR run builds on all OSes.
- **Verification:** a green CI run on the PR. A dry-run release job produces artifacts with checksums.

### U15. Repo cleanup + scrub files
- **Goal:** The current tree has no personal or machine details and no legacy app, and the docs tell the truth.
- **Requirements:** R32 (files), R33, R36.
- **Files:**
  - delete `legacy/`;
  - move superseded `docs/plans/*` to `docs/archive/`;
  - rewrite `README.md`, `ROADMAP.md`, `AGENTS.md`, `CONCEPTS.md` and `DESIGN.md`;
  - fixtures (`tests/fixtures/flow-contract.json`, iOS samples, `flow.ts` strings), `examples/**`, and `sdks/python/**/binds.py` (private tool hook).
- **Approach:**
  - Build the private expression list from the repo audit; it is never committed.
  - Replace each hit with a generic value.
  - Remove the CI step that compiles `legacy/`.
  - Remove the Dependabot entry for `legacy/`.
  - Keep `packages/*` and `apps/ios` as separate products, with their docs fixed.
- **Test scenarios:**
  - The full test suites still pass, so fixtures still match contracts after the generic values go in.
  - A scrub-check script exits 0 with no hits.
- **Verification:** the scrub check, run against the private expressions file, finds nothing in the tree.

### U16. Open-source basics
- **Goal:** The repo meets the R34 checklist.
- **Requirements:** R34, R35.
- **Files:**
  - `THIRD_PARTY_NOTICES.md`: Hex, Voxtype, transcribe.cpp, ggml, and the model licenses with Parakeet v3 CC-BY attribution;
  - `SECURITY.md`, using GitHub private vulnerability reporting;
  - `CONTRIBUTING.md` with DCO, and `CODE_OF_CONDUCT.md`;
  - `.github/ISSUE_TEMPLATE/*`;
  - `.github/workflows/codeql.yml`;
  - `.github/dependabot.yml`, covering every package root including `sdks/python`, `apps/desktop` (cargo) and actions;
  - `.github/CODEOWNERS`;
  - `docs/clean-room.md`, for TypeWhisper.
- **Approach:** Keep the original copyright lines. Enable secret scanning and push protection via `gh api` if permitted; otherwise record the command for the owner.
- **Test scenarios:** actionlint passes; the notices list matches the imported code.
- **Verification:** files present; CI green.

### U17. Retire `wordink-dictd` on Linux
- **Goal:** Linux users and the owner run the app only. `wordink-dictd` remains for Windows and macOS until parity.
- **Requirements:** R4.
- **Files:** `examples/omarchy-dictation/` (points to the app), and the `sdks/python` README (deprecation note for Linux).
- **Approach:**
  - Mark Linux dictd deprecated, without deleting it.
  - Remove the Voxtype post-update reinstall hook on the owner's machine (outside the repo).
  - Leave Voxtype uninstallable by the owner; uninstalling a package needs sudo and is the owner's call, so it is listed in the report.
- **Test scenarios:** the dictd test suite still passes.
- **Verification:** the owner's machine runs only the app for dictation.

### U18. History rewrite
- **Goal:** No personal or machine details remain in any commit of `master`.
- **Requirements:** R32 (history).
- **Files:** none in the tree; a private expressions file outside the repo.
- **Approach:**
  1. Take a fresh mirror clone. Run a credential scan (gitleaks) over the full history first; a found credential is rotated by the owner, not just rewritten.
  2. Make a `git bundle` backup into `~/Downloads/claude/` and verify it.
  3. Run `git filter-repo --replace-text` with the private expressions.
  4. Verify with `git log master -p | rg` over the expressions: no hits.
  5. Force-push `master` once. Leave other branches alone except those that must follow; close or rebase open PRs only if needed and record which.
  6. If the push is blocked by branch protection or auto mode, record the exact commands for the owner.
  7. List remaining refs that still contain the details (other remote branches, tags, and GitHub's read-only `refs/pull/*`, which a force-push cannot reach) in the morning report. Deleting branches, or asking GitHub support to purge cached refs, is the owner's call.
- **Test scenarios:** none (an operational step).
- **Verification:** the post-push mirror search finds no hits, and CI is green on the rewritten master.

### U19. Roadmap, issues, tracker
- **Goal:** Public and private tracking match what shipped.
- **Requirements:** R37.
- **Files:**
  - `ROADMAP.md`: now/next/later themes plus the platform and model matrix;
  - GitHub issues: close the delivered ones, open the deferred ones;
  - the owner's private tracker, via its API, with the credential from the owner's secret store and never echoed.
- **Approach:**
  - Sync the missing PRs and fix stale entries in the private tracker.
  - Keep private ticket ids out of the repo.
- **Test scenarios:** none.
- **Verification:** the issue list and the tracker match the morning report.

## Verification Contract

- Desktop app:
  - `cd apps/desktop && cargo build --release && cargo test`;
  - `cargo clippy --all-targets -- -D warnings` where Hex's CI runs it;
  - Hex's HUD/story tests.
- Python: `cd sdks/python && uv run pytest`. The known local failure `test_dictd_installer.py::test_missing_packages_are_listed_not_installed_unless_asked` is environment-specific; note it, don't fix it here.
- JS packages: `pnpm -r test` and `pnpm -r typecheck`.
- Rust core: `cargo test -p wordink-core`.
- Workflows: actionlint, via the existing `workflow-lint.yml`.
- No live model calls in tests. The single live Groq smoke test in U3/U5 runs on the owner's machine with minimal spend.
- Scrub check (U15/U18): a script reads a private expressions file and searches the tree, or history, and must report zero hits.

## Definition of Done

- Every unit's Verification holds, and CI is green on `master` on all three OSes.
- The owner's Linux machine dictates with the WordInk app on Alt+Space through Groq, and `wordink-dictd` is disabled there.
- AE1-AE5 pass. AE5 runs after U18.
- `master` history is rewritten once, with a verified backup bundle in `~/Downloads/claude/`.
- ROADMAP, issues and the private tracker are updated.
- No abandoned-attempt code, scratch files or experiment branches are left in merged PRs. Scratch worktrees are removed.
- `~/Downloads/claude/wordink-overnight-report.md` lists what merged (PR links), what's installed, what's blocked and the exact commands for the owner.
