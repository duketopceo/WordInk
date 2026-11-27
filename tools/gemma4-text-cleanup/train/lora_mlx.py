#!/usr/bin/env python3
"""
MLX LoRA fine-tune for google/gemma-4-E2B-it on Apple Silicon (32 GB class).

Requires: uv sync --extra train-mlx
See tools/gemma4-text-cleanup/README.md
"""

from __future__ import annotations

import argparse
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path(".local-gemma-training/mlx-out"))
    parser.add_argument("--model", default="google/gemma-4-E2B-it")
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--iters", type=int, default=300)
    parser.add_argument("--learning-rate", type=float, default=1e-5)
    args = parser.parse_args()

    try:
        from mlx_lm import load
        from mlx_lm.tuner import lora as mlx_lora
    except ImportError as error:
        raise SystemExit(
            "install MLX deps: uv sync --extra train-mlx\n" f"missing: {error}"
        ) from error

    args.output_dir.mkdir(parents=True, exist_ok=True)
    mlx_lora.train(
        model=args.model,
        train=args.data,
        adapter_path=str(args.output_dir),
        lora_layers=args.rank,
        iters=args.iters,
        learning_rate=args.learning_rate,
        batch_size=1,
    )
    print(f"saved MLX adapter to {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())