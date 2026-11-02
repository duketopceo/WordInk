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

**Means:** Copy Hex's MIT source into this repo as the app base. Port Voxtype's text, media and feedback features into it, plus WordInk's providers, gateway and meeting mode. Re-implement TypeWhisper's best ideas from scratch. Clean the repo to open-source standards along the way.

**Product authority:** The owner (repo maintainer) made the decisions in this session, recorded under Key Decisions. This plan supersedes `docs/plans/2026-11-01-0001-feat-wordink-convergence-plan.md` as the active convergence plan.

**Open blockers:**
- macOS notarization and Windows Authenticode need paid accounts. They are deferred by decision (R30), so they don't block.
- The device acceptance pass on Windows (cyberpowa) and on a real Mac needs the owner present.

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
- R9. Pressing the hotkey cancels a recording; pressing Esc twice cancels from the pill.
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
- F3. **Cancel or recover** (covers R9, R11): press the hotkey again mid-recording, or Esc twice, and nothing is inserted. If the provider fails, the clip is kept and History offers retry with another model.

### Acceptance Examples

- AE1. (covers R7, R8) With the default pill, dictating "hello world" with Groq on Omarchy shows the pill through record, transcribe and done, and "hello world" lands in a foot terminal; switching the style to live text shows the words as they stream.
- AE2. (covers R20, R22) On a machine with no API key and Vulkan available, onboarding recommends Parakeet v3, and dictation works offline after the download.
- AE3. (covers R12, R13) With the owner's Voxtype replacement table imported, a spoken replacement term comes out written, and "um" is dropped when filler removal is on.
- AE4. (covers R10) Dictating while a password manager field is focused inserts nothing and shows why.
- AE5. (covers R32) A search of the repo and its full history for the owner's hostnames, email and personal booking link finds nothing.

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
- The Hex window needs Vulkan on Linux and Windows and AVX2 on x86. Machines without them use the cloud path through a minimal fallback or are unsupported. Planning decides which.
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
