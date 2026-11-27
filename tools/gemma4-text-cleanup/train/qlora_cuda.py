#!/usr/bin/env python3
"""
QLoRA fine-tune for google/gemma-4-E2B-it (rank 16, completion-only loss).

Requires: uv sync --extra train-cuda
See tools/gemma4-text-cleanup/README.md
"""

from __future__ import annotations

import argparse
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, default=Path(".local-gemma-training/qlora-out"))
    parser.add_argument("--model", default="google/gemma-4-E2B-it")
    parser.add_argument("--rank", type=int, default=16)
    parser.add_argument("--epochs", type=float, default=1.0)
    parser.add_argument("--batch-size", type=int, default=1)
    parser.add_argument("--learning-rate", type=float, default=2e-4)
    args = parser.parse_args()

    try:
        import torch
        from datasets import load_dataset
        from peft import LoraConfig, get_peft_model, prepare_model_for_kbit_training
        from transformers import AutoModelForCausalLM, AutoTokenizer, BitsAndBytesConfig
        from trl import SFTConfig, SFTTrainer
    except ImportError as error:
        raise SystemExit(
            "install CUDA training deps: uv sync --extra train-cuda\n"
            f"missing: {error}"
        ) from error

    dataset = load_dataset("json", data_files=str(args.data), split="train")

    bnb = BitsAndBytesConfig(
        load_in_4bit=True,
        bnb_4bit_quant_type="nf4",
        bnb_4bit_compute_dtype=torch.bfloat16,
    )
    tokenizer = AutoTokenizer.from_pretrained(args.model)
    model = AutoModelForCausalLM.from_pretrained(
        args.model,
        quantization_config=bnb,
        device_map="auto",
    )
    model = prepare_model_for_kbit_training(model)

    # E2B: keep per-layer embeddings frozen (best practice for Gemma 4 E2B).
    for name, parameter in model.named_parameters():
        if "embed" in name:
            parameter.requires_grad = False

    lora = LoraConfig(
        r=args.rank,
        lora_alpha=args.rank,
        lora_dropout=0.05,
        bias="none",
        task_type="CAUSAL_LM",
        target_modules=["q_proj", "k_proj", "v_proj", "o_proj", "gate_proj", "up_proj", "down_proj"],
    )
    model = get_peft_model(model, lora)

    def format_example(row):
        if "messages" in row:
            text = tokenizer.apply_chat_template(
                row["messages"], tokenize=False, add_generation_prompt=False
            )
        else:
            text = row["prompt"] + row["completion"]
        return {"text": text}

    formatted = dataset.map(format_example, remove_columns=dataset.column_names)

    training = SFTConfig(
        output_dir=str(args.output_dir),
        num_train_epochs=args.epochs,
        per_device_train_batch_size=args.batch_size,
        learning_rate=args.learning_rate,
        logging_steps=5,
        save_steps=100,
        bf16=True,
        completion_only_loss=True,
        dataset_text_field="text",
    )
    trainer = SFTTrainer(
        model=model,
        args=training,
        train_dataset=formatted,
        processing_class=tokenizer,
    )
    trainer.train()
    trainer.save_model()
    tokenizer.save_pretrained(args.output_dir)
    print(f"saved adapter to {args.output_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())