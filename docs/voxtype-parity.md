# Voxtype parity

Voxtype features a user relies on, mapped to the WordInk desktop app
(`apps/desktop`). Checked by reading the code and running its tests (U6 of
`docs/plans/2026-11-02-0136-feat-wordink-one-app-plan.md`; R9, R12-R15).

Status: **covered** (works today), **filled** (a small gap closed by U6),
**gap** (not built; size S under a day, M 1-3 days, L more).

| Voxtype feature | WordInk setting / CLI | Status |
|---|---|---|
| `[text.replacements]` phrase map, case-insensitive | Global mode replacements (Settings, `dictation_processing.default_mode.replacements`); `wordink import voxtype` brings a Voxtype table across. Word-bounded, longest match wins, empty output deletes | covered |
| Replacement import | `wordink import voxtype [--config PATH] [--dry-run]` | filled (new) |
| `filter_fillers` (uh, um, er, ah, eh, hmm, hm, mm, mhm) | Labs `filler_words`; list widened to Voxtype's set in U6 | filled |
| Filler collisions per language (Portuguese/German "um" survives) | none: the filler list is not language-aware | gap, S |
| `spoken_punctuation`: new line, new paragraph, quotes | Labs `spoken_formatting` | covered |
| `spoken_punctuation`: comma, colon, semicolon, hyphen, underscore, slash, hashtag, at sign | Labs `spoken_symbols` | covered |
| `spoken_punctuation`: period, question mark, exclamation mark/point, open/close paren(thesis), bracket, brace, dollar sign, percent sign, backslash | Labs `spoken_symbols`, added in U6 | filled |
| `spoken_punctuation`: ambiguous bare words (plus, equals, pipe, tab, dash, hash, percent, tilde, backtick, ampersand, asterisk) | not built, because they are ordinary words and the join style is a design choice. A replacement such as "plus sign" to "+" does the same today | gap, S |
| `[vad]` silence rejection | always on: Silero VAD (`transcription/speech_detector.rs`) with a level-only fallback (`is_near_silent`) before transcription; `pause_segmenter.rs` splits long audio. No on/off or threshold setting | covered (no tuning knobs) |
| `audio.pause_media` (MPRIS pause and resume) | `recording_audio_behavior = pause_media` (`linux/audio_control.rs`, also Windows and macOS paths); import maps it | covered |
| `audio.duck_media` + `duck_media_volume_percent` | `recording_audio_behavior = duck`, `recording_duck_volume` (a fraction); import maps both | covered |
| `pause_media_ignored_players`, `duck_media_fade_ms` | none; all playing players pause, the ramp is fixed | gap, S |
| `[audio.feedback]` sounds and themes | `sound_effects`, `sound_effect_volume`, `sound_theme` (7 sets: Signature, Classic, Click, Soft, Wood, Analog, Thock) | covered |
| Custom feedback sound directory | none | gap, S |
| Cancel a recording (`record cancel`, cancel key) | `cancel_hotkey` setting (rebindable, `wordink settings set cancel_hotkey ...`) and pressing Esc twice within 600 ms while recording; pressing the dictation hotkey again stops and inserts. `wordink dictation cancel` is removed by design (#786) | covered on Linux; Windows (keyboard hook) and macOS (event tap) wired to the same Esc-twice state machine, pending a real-hardware check |
| `output.auto_submit`, `smart_auto_submit` ("submit" presses Enter), `append_text`, `shift_enter_newlines` | none in WordInk | gap, M (needs an Enter inject on three OSes; not in the U6 take-list) |
| Meeting mode, engines, OSD, per-phase notifications, hooks | outside U6 (other units) | not assessed here |

## `wordink import voxtype`

```
wordink import voxtype [--config PATH] [--dry-run]
```

Reads a Voxtype `config.toml` (default `$XDG_CONFIG_HOME/voxtype/config.toml`,
else `~/.config/voxtype/config.toml`) and merges it into WordInk's
`settings.json`. Available wherever the CLI is (Linux, macOS, Windows).

| Voxtype key | Lands in |
|---|---|
| `text.replacements` | Global mode replacements |
| `text.filter_fillers = true` | `labs.filler_words` on |
| `text.spoken_punctuation = true` | `labs.spoken_formatting` and `labs.spoken_symbols` on |
| `audio.pause_media = true` | `recording_audio_behavior = pause_media` |
| `audio.duck_media = true` (+ `duck_media_volume_percent`) | `recording_audio_behavior = duck`, `recording_duck_volume` |

Rules:

- It only adds. A toggle Voxtype has off leaves WordInk's alone. A phrase
  WordInk already has (compared case-insensitively) is kept, and counted as
  already present or conflicting. A media behaviour you already chose is kept.
  Running it twice changes nothing; the second run does not rewrite the file.
- Pause wins over duck when Voxtype has both on, since WordInk has one
  behaviour at a time.
- The summary is counts and key names. Replacement phrases and outputs are
  never printed, and the `text.replacements` and `profiles` tables are not
  opened when listing skipped keys.
- Keys with no WordInk setting are listed by dotted name and skipped. A mapped
  key with the wrong type is reported as invalid and skipped.
- `--dry-run` reads both files and writes nothing. A real run says to restart
  WordInk if it is running, since the running app holds its own copy.
- Exit 0 on success, 1 when the config is missing, is not TOML, or settings
  cannot be written.

Logic is WordInk's own; only Voxtype's documented config key names are used.
Voxtype is MIT (https://github.com/peteonrails/voxtype).
