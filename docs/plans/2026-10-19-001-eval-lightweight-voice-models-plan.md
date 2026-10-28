---
artifact_contract: ce-unified-plan/v1
product_contract_source: ce-plan user-directed (kill-filter applied: lightweight + strong only)
external_sources:
  - QwenLM/Qwen3-ASR (github.com/QwenLM/Qwen3-ASR) — 3.7k★, Apache-2.0, 0.6B/1.7B models
  - microsoft/VibeVoice (github.com/microsoft/VibeVoice) — 54.9k★, MIT — Realtime-0.5B + ASR-BitNet engine only
---

# Lightweight voice eval — ASR/TTS candidates vs the WordInk relay

## Goal Capsule

Bench the genuinely lightweight open voice models against the current WordInk gateway relay (api-ink.shippedit.dev → omarchy-max :8941). **Kill criteria per the owner's filter: lightweight AND strong — a mid-speed/mid-quality model is a reject, not a compromise.** Replace the relay only on a clear win.

## Product Contract

### The candidate list (pre-filtered — heavyweight models excluded on arrival)

| Candidate | Size | Claim | Fits filter? |
|---|---|---|---|
| **Qwen3-ASR-0.6B** | 0.6B | 52 langs+dialects, streaming+offline unified, 2000× throughput @ conc-128, near-SOTA | Yes — flagship candidate |
| **VibeVoice-ASR-BitNet** | ~1.58GB quant artifact (parameter count and base revision recorded in the verdict table before benchmarking; the 7B exclusion applies to parameters, not artifact size) | realtime ASR on 3+ CPU threads, no GPU, heterogeneous I8_S+I2_S quant | Yes — built for CPU-only boxes like ours |
| **VibeVoice-Realtime-0.5B** | 0.5B | real-time TTS, ~300ms first-audio latency | Yes (TTS side) |
| Qwen3-ASR-1.7B | 1.7B | SOTA open ASR | Borderline — include only if 0.6B meets the pre-registered inclusion gate below (set before any benchmark runs) |
| ~~VibeVoice-ASR-Streaming-7B~~ | 7B | streaming+diarization | **No — fails lightweight filter** |
| ~~VibeVoice-TTS-1.5B~~ | 1.5B | podcast-grade long TTS | **No — fails filter** |

### Pre-registered measurement contract

Every number below is **final as of 2026-10-28** (owner-approved), fixed before any benchmark runs. Changing a threshold after seeing results invalidates the run; record any change here first, with the date.

- **Data boundary.** Benchmark audio and reference transcripts are synthetic (read from a written script) or explicitly approved by the owner. Real Voxtype captures are used only after approval, and never leave the machine unless the owner has approved the relay path for that sample. The relay is the owner's own gateway; state the token and access boundary and each configured provider's retention and deletion terms in the verdict doc, and do not assume a dedicated tenant.
- **Corpus.** A published, immutable manifest (`docs/decisions/lightweight-voice-eval-corpus.json`) listing every sample: id, speaker, language, duration, segmentation (one utterance per file), and the reference transcript with its normalization rules (lowercase, strip punctuation, keep identifiers verbatim). At least 20 samples, 10 to 60 s each, mixed prose, code identifiers and proper nouns. The relay and every candidate process exactly these samples.
- **Latency.** Measured from the end of the audio file read to the final transcript, per sample, warm model (cold start reported separately as model load to first transcript). Report p50 and p95 over at least 3 runs. Streaming candidates also report time to first partial; only the final result counts for WER. TTS: time to first audio chunk, p50 and p95.
- **WER.** Computed on normalized text. "Competitive" means within 1.0 absolute point of the relay; "equal" means within 0.5. CPU configuration (threads, governor, quantization) is recorded per run.
- **Qwen3-ASR-1.7B inclusion gate.** Run it only if Qwen3-ASR-0.6B has WER within 3.0 absolute points of the relay and misses either the latency or RAM bar below by no more than 50%.
- **Memory.** Measured as peak process-tree RSS while the llama services are under their normal load. Bar: peak RSS at most 2 GB, and at least 1 GB of free memory remaining. Over the 10-minute sustained run, RSS growth must stay under 10%. Artifact size and peak RSS are reported separately.
- **TTS rubric.** Dictation read-back: 5 sentences, intelligibility scored 1 to 5 by the owner blind to the model; adopt only at a mean of 4 or above.

### What "win" means

- **ASR (voxtype path):** WER competitive with the relay on real dictation samples (mixed code-words/prose/proper-nouns), and latency ≤ current relay round-trip. RTF < 0.5 on this CPU.
- **TTS (if WordInk voices output):** first-audio ≤ ~300ms, quality acceptable for dictation confirmation/readback.
- Both must run on omarchy-max's CPU without starving the llama services — memory footprint is part of "lightweight."

### Scope boundaries

- **In:** download/bench Qwen3-ASR-0.6B (+1.7B only if needed) and VibeVoice-ASR-BitNet against a dictation corpus; latency+WER+RAM table; verdict doc.
- **Out:** swapping the relay, touching voxtype config, any 7B+ model, GPU assumptions (Asahi has no CUDA; llama.cpp Vulkan path exists but these are CPU-native candidates).

### Requirements

1. Bench harness: fixed corpus of dictation samples (per the corpus manifest above), timed end-to-end, WER computed against reference transcripts.
2. Qwen3-ASR-0.6B via their `qwen-asr` package or vLLM path — on CPU; note whether the 2000× claim survives CPU-only inference (it's a batch claim — dictation is single-stream, measure *streaming* latency).
3. BitNet path: `microsoft/VibeASR.cpp` engine — custom inference stack; flag dependency weight in the verdict.
4. Memory: measure RSS during inference — must fit alongside the llama services within headroom (oomd is watching).
5. Verdict doc `docs/decisions/lightweight-voice-eval.md`: per-model latency/WER/RAM + adopt/reject. "Keep the relay" is a legitimate verdict.

### Acceptance examples

- Table exists with real numbers for every in-scope candidate.
- A rejection cites numbers ("0.6B is 2.1× the latency at equal WER" beats "felt slow").

## Repo Touchpoints

- `docs/decisions/lightweight-voice-eval.md` (new)
- `crates/`/`apps/` untouched — this is eval, not integration

## Test Scenarios

1. Corpus bench: ≥20 samples, mixed content, WER + latency + RAM per model.
2. Cold-start time (model load → first transcript) — matters for a daemon that may restart.
3. Sustained-run check: no memory creep over a 10-min dictation session.

## Risks / Honest Notes

- Most likely outcome: Qwen3-ASR-0.6B wins on paper (52 langs, SOTA-adjacent) but the relay wins on integration — it's already wired. The plan forces the honest comparison so "works today" isn't confused with "is best."
- BitNet is the dark horse: realtime CPU ASR with no GPU on *this exact hardware constraint* — but it's a bespoke runtime, so adoption cost is higher than the weights suggest.
