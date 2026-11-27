# Gemma 4 text-cleanup (Track 1)

Local-first LoRA training and OpenAI-compatible inference hook for dictation
post-processing. Does **not** change default STT engines (Groq / Parakeet).

## Dataset

Build JSONL under `.local-gemma-training/` (gitignored):

```bash
cd tools/gemma4-text-cleanup
uv sync
uv run wordink-gemma-dataset \
  --output ../../.local-gemma-training/train.jsonl \
  --general-chat-slice 0.1

# Optional: mix in your own history (never commit the file)
uv run wordink-gemma-dataset \
  --history ~/.local/share/wordink/history.json \
  --output ../../.local-gemma-training/train.jsonl
```

Formats match [Gemma dataset guidance](https://gemma.ai/tuning/datasets-formatting/)
(`--style chat` default; `--style prompt_completion` also supported).

Bootstrap pairs live in `wordink_gemma4/data/synthetic_pairs.json` (filler-style
cleanup, spoken formatting, vocabulary-aware examples).

## Training

**CUDA (TRL + PEFT QLoRA, rank 16, completion-only loss):**

```bash
uv sync --extra train-cuda
uv run python train/qlora_cuda.py \
  --data ../../.local-gemma-training/train.jsonl
```

**Apple Silicon (MLX LoRA, 32 GB Macs):**

```bash
uv sync --extra train-mlx
uv run python train/lora_mlx.py \
  --data ../../.local-gemma-training/train.jsonl
```

See [export/README.md](export/README.md) for GGUF `Q4_K_M` and MLX 4-bit serving.

## Desktop integration

`DictationPostProcessing` supports `backend: "local_openai"` with
`local_base_url` and `local_model`. The rewrite uses `POST /v1/chat/completions`
via `apps/desktop/src/dictation/local_openai_rewrite.rs`.

Evaluate cleanup quality on the vocabulary corpus:

```bash
python3 apps/desktop/scripts/gemma-postprocess-eval.py \
  --base-url http://127.0.0.1:8080 \
  --model gemma4-cleanup
```

This reuses `scripts/vocabulary-eval.py` term-recall and false-replacement gates.