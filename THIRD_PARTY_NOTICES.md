# Third-Party Notices

WordInk's own source is MIT licensed (see [LICENSE](LICENSE)). This file lists
the code and models WordInk builds on. Dependencies keep their own upstream
licenses.

## Code

### Hex

The desktop app (`apps/desktop`, added in a separate change) is built on Hex
(https://github.com/anomalyco/hex), vendored into that directory.

MIT License. Copyright (c) 2026 Christopher Langton.

Hex's own notices (icon assets, model licenses and its other dependencies)
are in its `THIRD_PARTY_NOTICES.md`, which is kept alongside the vendored
source in `apps/desktop`. This file does not repeat them.

### Voxtype

Some features are ported from Voxtype (https://github.com/peteonrails/voxtype).

MIT License. Copyright (c) 2025 Peter Jackson.

### transcribe.cpp and ggml

Local speech-to-text runs on transcribe.cpp
(https://github.com/handy-computer/transcribe.cpp), which uses ggml
(https://github.com/ggml-org/ggml). Both are MIT licensed; their copyright
notices are in their upstream repositories and ship with the vendored source.

### groq_flow

WordInk began as a fork of groq_flow (https://github.com/ParthJain18/groq_flow)
by Parth Jain, released under the MIT License. See [LICENSE](LICENSE).

## Models

Models are downloaded at runtime, on request. WordInk does not bundle or
redistribute any model weights.

### Parakeet TDT 0.6B v3 (default local model)

Parakeet TDT 0.6B v3 by NVIDIA (https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3).
Creative Commons Attribution 4.0 International (CC BY 4.0):
https://creativecommons.org/licenses/by/4.0/legalcode.en

WordInk uses the model unmodified. The only change is a format conversion to
GGUF for transcribe.cpp: the conversion repackages the weights, renaming and
re-laying-out tensors, and quantized variants also store them at reduced
numeric precision. The weights are not retrained or fine-tuned. The weights are
provided as-is, without warranties, as the license states.

### Whisper large-v3-turbo

By OpenAI. Its model card lists the MIT license (the Whisper code is MIT; some
distributions of the weights list Apache-2.0). Check the card of the exact
file you download.

### Qwen3-ASR

By the Qwen team at Alibaba Cloud. Apache License 2.0.

### Models under non-open licenses

Models with non-open licenses (for example the NVIDIA Open Model License) are
optional. They are labelled with their license in the model picker and are
never the default.
