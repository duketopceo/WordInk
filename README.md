# 🖋️ WordInk

**Sub-Second Voice Dictation, Embeddable SDK, and Grounded GUI Voice Driver**  
*Powered by Groq Whisper Turbo (`whisper-large-v3-turbo`) on LPUs (~300ms latency).*

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![Groq Whisper Turbo](https://img.shields.io/badge/Model-Whisper%20Large%20v3%20Turbo-orange.svg)](https://console.groq.com)
[![Status](https://img.shields.io/badge/Status-Active-brightgreen.svg)]()

WordInk turns spoken thoughts into immediate text at the speed of thought. Built as a fast, privacy-first open alternative to proprietary tools like Wispr Flow and Superwhisper, WordInk functions simultaneously as a silent system dictation daemon, an embeddable Python SDK, and a high-bandwidth voice frontend for Computer-Using Agents (CUAs).

---

## ⚡ Why WordInk?

- ⚡ **Sub-Second Latency (~300ms)**: Direct raw streaming to Groq's Whisper Turbo LPU without unnecessary LLM hops.
- 📋 **Instant Native Paste**: Injects full paragraphs all at once using clipboard injection, with automatic fallback to Unicode bracketed typing in terminals (Ghostty, Cascadia, Windows Terminal).
- 🎛️ **Interactive Setup Wizard**: Beautiful browser-based onboarding with live key detection visualizer (`[F]`, `[U]`, `[Insert]`, `[Numpad 8]`), microphone VU meter, and Groq API verification.
- 📊 **Local Telemetry & Savings**: Tracks total words dictated, average latency, and monthly savings stored in local SQLite without external phoning home.
- 🧰 **First-Class Embeddable SDK**: Add sub-second voice transcription to any desktop app, agent pipeline, or CLI with a simple `from wordink import WordInkEngine`.
- 🤖 **Computer-Using Agent (CUA) Ready**: Designed to pair with Grounded GUI agents using temperature-scaled probabilistic centering.

---

## 🚀 Quick Start

### 1. Installation

Clone your fork and install dependencies via `uv`:

```bash
git clone https://github.com/duketopceo/wordink.git
cd wordink

# Install dependencies and editable package
uv sync
uv pip install -e .
```

### 2. Interactive Onboarding Wizard

Launch the visual onboarding wizard to test your microphone, verify your Groq API key, and configure your hotkey:

```bash
uv run wordink --onboard
# Or:
# python -m wordink.onboarding.server
```

The wizard opens at `http://localhost:18981` in your browser:
1. **API Key**: Enter your Groq API key (free tier includes ~2 hours daily).
2. **Model Selection**: Select `whisper-large-v3-turbo` (sub-second response).
3. **Microphone**: Live VU audio meter to test your mic before saving.
4. **Keyboard Visualizer**: Press any key on your keyboard to test physical scan codes and select your hotkey (<kbd>Alt</kbd> + <kbd>D</kbd> recommended).
5. **Launch**: Saves configuration to `.env` and initializes silent background operation.

### 3. Running WordInk

```bash
# Run in system tray
uv run wordink

# Or run silent in background on Windows boot
wscript.exe start_silent.vbs
```

---

## 📦 Using the Embeddable SDK

WordInk can be embedded into any Python application, CLI, or agent runtime:

```python
from wordink import WordInkEngine, transcribe

# 1. Direct one-liner transcription
text = transcribe("path/to/meeting.wav")
print("Transcribed:", text)

# 2. Live event-driven voice listener
def handle_text(text: str):
    print(f"User dictated: {text}")

engine = WordInkEngine(
    on_transcript=handle_text,
    auto_paste=True  # Automatically injects text at active cursor
)

# Start / stop listening
engine.start_recording()
# ... speak ...
engine.stop_recording()
```

---

## 🧠 Architecture: WordInk & Computer-Using Agents (CUAs)

### The Grounded Icon Problem & Probabilistic Centering

When Computer-Using Agents (CUAs) attempt to interact with desktop user interfaces based on natural language instructions (e.g., *"Click the monitor icon in the menu bar"*), Vision-Language Models (VLMs) and Decision APIs output spatial probability heatmaps over screen coordinates $(x, y)$.

Direct greedy argmax coordinate prediction often causes **coordinate drift**—landing on adjacent padding, border edges, or missing sub-16px icons.

WordInk bridges high-bandwidth voice commands to CUA drivers using **temperature-scaled probabilistic centering**:

```
[Voice Input: "Click monitor icon"]
               │
               ▼  (~300ms)
    ┌─────────────────────┐
    │ WordInk Voice Engine│
    └─────────────────────┘
               │  Intent Text
               ▼
    ┌─────────────────────┐
    │  CUA Decision API   │
    │  Spatial Heatmap    │
    └─────────────────────┘
               │
               ▼  Power Scaling: P'(x,y) = P(x,y)^α / Σ P^α
    ┌─────────────────────┐
    │Probabilistic Center │  ==> Computes center of mass E[X, Y]
    └─────────────────────┘
               │
               ▼  Sub-pixel exact click
        [ Monitor Icon ]
```

1. **Power Scaling ($P^\alpha$)**: Raising raw coordinate heatmaps to an exponent $\alpha > 1$ sharpens the distribution peak around the target icon while dropping ambient background noise to zero.
2. **Probabilistic Centering**: Calculating the expected center of mass:
   $$\mathbb{E}[X] = \sum_x x \cdot P'(x), \quad \mathbb{E}[Y] = \sum_y y \cdot P'(y)$$
   reliably drives the OS mouse cursor into the geometric center of the targeted UI component.

---

## ⚙️ Configuration Reference (`.env`)

```ini
# Groq API Configuration
GROQ_API_KEY=gsk_your_key_here

# Dictation Hotkey (Alt+D recommended)
HOTKEY=alt+d

# Whisper Model (whisper-large-v3-turbo for ~300ms latency)
WHISPER_MODEL=whisper-large-v3-turbo

# Noise Reduction (disabled by default for maximum speed & short utterances)
ENABLE_NOISE_REDUCTION=false

# Secondary LLM Cleanup (disabled by default to maintain sub-second speed)
ENABLE_AI_CLEANUP=false

# Debug Output
DEBUG=false
```

---

## 🪟 Windows Startup Setup

To have WordInk launch silently on system boot without any console window:

1. Press <kbd>Win</kbd> + <kbd>R</kbd> and type `shell:startup`.
2. Create a shortcut to `start_silent.vbs` located in this repository directory.
3. WordInk will run quietly in your Windows system tray on boot.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).  
Original work copyright (c) Parth Jain. Modifications and enhancements copyright (c) Duketop CEO.
