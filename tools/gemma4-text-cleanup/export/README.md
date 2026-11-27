# Export Gemma 4 text-cleanup LoRA for local inference

After QLoRA or MLX training under `.local-gemma-training/` (gitignored):

## GGUF Q4_K_M (`llama.cpp`)

1. Merge the Hugging Face adapter into the base weights (optional helper):

   ```bash
   python - <<'PY'
   from peft import PeftModel
   from transformers import AutoModelForCausalLM, AutoTokenizer
   base = "google/gemma-4-E2B-it"
   adapter = ".local-gemma-training/qlora-out"
   model = AutoModelForCausalLM.from_pretrained(base)
   model = PeftModel.from_pretrained(model, adapter)
   merged = model.merge_and_unload()
   out = ".local-gemma-training/merged-hf"
   merged.save_pretrained(out)
   AutoTokenizer.from_pretrained(base).save_pretrained(out)
   print("wrote", out)
   PY
   ```

2. Convert with `llama.cpp` (follow [Gemma export guidance](https://gemma.ai/tuning/export/)):

   ```bash
   python convert_hf_to_gguf.py .local-gemma-training/merged-hf \
     --outfile .local-gemma-training/gemma4-cleanup-f16.gguf
   ./llama-quantize .local-gemma-training/gemma4-cleanup-f16.gguf \
     .local-gemma-training/gemma4-cleanup-q4_k_m.gguf Q4_K_M
   ```

3. Serve:

   ```bash
   ./llama-server -m .local-gemma-training/gemma4-cleanup-q4_k_m.gguf \
     --port 8080 -ngl 99
   ```

## MLX 4-bit (`mlx_vlm.server`)

1. Fuse the MLX adapter:

   ```bash
   python -m mlx_lm.fuse \
     --model google/gemma-4-E2B-it \
     --adapter-path .local-gemma-training/mlx-out \
     --save-path .local-gemma-training/mlx-fused
   ```

2. Quantize and serve (see [Gemma export guidance](https://gemma.ai/tuning/export/)):

   ```bash
   python -m mlx_lm.convert --hf-path .local-gemma-training/mlx-fused \
     -q --q-bits 4 --mlx-path .local-gemma-training/mlx-4bit
   mlx_vlm.server --model .local-gemma-training/mlx-4bit --port 8080
   ```

## WordInk post-processing settings

In the mode's `post_processing` block (settings JSON or Modes UI when exposed):

```json
{
  "enabled": true,
  "backend": "local_openai",
  "local_base_url": "http://127.0.0.1:8080",
  "local_model": "gemma4-cleanup",
  "prompt": "Rewrite the transcript into clear, natural text while preserving the speaker's intended meaning.",
  "deadline_seconds": 30
}
```

Groq/Parakeet transcription defaults are unchanged; only the optional rewrite step uses the local server.