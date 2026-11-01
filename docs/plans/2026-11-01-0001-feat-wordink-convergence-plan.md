# WordInk convergence plan: pull Voxtype + Hex into wordink-dictd

Owner decision (Nov 1, 2026): converge the two proven donor implementations into WordInk
rather than integrate beside them. `wordink-dictd` becomes the single dictation client
on every OS; Voxtype and Hex become reference/donor code (both MIT), not dependencies.
After the pull, this doc's feature inventory feeds a `ce-plan` roadmap recap.

## Why

- Voxtype has the production-proven Linux capture/paste paths; its user-facing feature
  set is exactly what `wordink-dictd` lacks today.
- Hex (anomalyco/hex, MIT, full source) proves the category: one Rust process, local
  models, polished settings app, per-app modes. Its `transcribe.cpp` engine and
  Parakeet vocabulary patch are directly adoptable.
- U9 requires "works on default open-source configuration" — a bundled local engine
  is the bar, and both donors show it done.

## Pull from Voxtype (voxtype 1.1.1 surface)

| Feature | State in dictd | Pull |
|---|---|---|
| `audio.device` source pin | `[record] device` added (PR #106) | done |
| `--paste-keys` / `output.paste_keys` | dictd already pastes terminals correctly | verify parity |
| `[text.replacements]` spoken->written map | absent | P2 |
| `filter_fillers` / spoken punctuation | absent | P2 |
| pause/duck media while recording | absent | P2 |
| VAD (reject silence-only clips) | `min_seconds` only | P2 |
| audio feedback tones | absent | P2 |
| notification controls per phase | partial | P2 |
| meeting mode | `meeting.py` exists | verify parity |
| model picker / multiple local engines | absent | P3 (superseded by hex engine) |

## Pull from Hex (v3.1.2)

| Feature | Pull |
|---|---|
| `transcribe.cpp` + ggml Vulkan patch + `hex/parakeet-vocabulary` rescoring branch | P3 — the bundled local engine; adopt, don't rebuild |
| Model catalog (parakeet-unified-en ~163x RT, whisper_large_v3_turbo, qwen3_asr, underdog_bark_3) | P3 — model downloader + picker |
| Per-app **Modes** (corrections + rewrite profiles) | P4 |
| Vocabulary biasing | P4 — dictd already has `vocab.py`; wire into local engine rescoring |
| `paste_with_shift` terminal paste | done — dictd inject path already handles terminals |
| History (`hex history`, retention) | P4 — dictd has `failed_dir`/state; needs retention + CLI |
| `live-text` provisional words | P5 — needs streaming engine; defer |
| One-process model + CLI surface (`dictation/status/history/mcp`) | already mirrored |
| Idle model unload (`unload_idle`) | P3 |
| **Settings GUI** (window, panes, model picker, mode editor) | P5 — rebuild as the WordInk settings app; scope TBD (native vs TUI vs web) |
| Signed updater | P5 |

## Architecture after the pull

```
wordink-dictd (one process per user)
├── capture: pw-record/parecord (+ device pin) — voxtype's proven path  [done]
├── engines:
│   ├── local: transcribe.cpp + Parakeet GGUF (hex pull)  [P3]
│   ├── BYO provider key (Groq/OpenAI/Deepgram)           [exists]
│   └── optional gateway endpoint                          [exists]
├── text: replacements, fillers, spoken punctuation        [P2]
├── modes: per-app correction/rewrite profiles             [P4]
├── output: wtype/ydotool + clipboard+paste fallback       [exists]
├── history + vocabulary                                   [P4]
└── settings app (hex-class GUI)                           [P5]
```

Voxtype and gateway integrations stay supported as optional deployments — the
converged app is the product, integrations are not required.

## Phases

- **P1 (done/landed):** `[record] device` (PR #106), terminal paste, compositor binds, systemd unit.
- **P2 — voxtype parity:** replacements, fillers, VAD, pause-media, feedback, notifications.
- **P3 — local engine:** transcribe.cpp binding, model downloader + picker, vocab rescoring, idle unload. The U9 "open-source config" gate lives here.
- **P4 — modes/vocab/history:** per-app modes, vocabulary biasing, retained history + CLI.
- **P5 — settings app + updater:** the hex-class GUI rebuild and update channel.

## Verification

Each phase lands with the repo's contract (`uv run pytest` in `sdks/python`, CI on
Linux+Windows+macOS) plus a device acceptance pass on omarchy-max and cyberpowa:
dictate into a foot terminal, a GUI app, and a password field per OS.
