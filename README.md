# 🎤 Groq Flow

**AI-Powered Speech-to-Text** — A Wispr Flow alternative using Groq's blazing-fast API.

Type 3-4x faster by speaking naturally. Groq Flow automatically removes filler words, fixes grammar, and makes your dictation professional. Works in any Windows application.

---

## ✨ Features

- 🎯 **Universal**: Works in Slack, VS Code, Gmail, Notion, browsers — everywhere
- 🤖 **AI Cleanup**: Removes "um," "uh," "like" and fixes grammar automatically
- ⚡ **Fast**: Groq's LPU inference (faster than OpenAI)
- 🔑 **Global Hotkey**: Press `Pause` from anywhere to start/stop
- 🖥️ **System Tray**: Runs quietly in background
- 💰 **Affordable**: ~$5/month vs $7-15 for Wispr Flow

---

## � Quick Start

### 1. Install

```powershell
# Run setup script
.\setup.ps1

# Or manually
uv sync
```

### 2. Configure

Get your free API key from [console.groq.com](https://console.groq.com/)

```bash
# Edit .env file
GROQ_API_KEY=gsk_your_key_here
```

### 3. Run

```powershell
.\run.ps1
# Or: uv run groq-flow
```

### 4. Use

1. Press `Pause` key
2. Speak naturally
3. Press `Pause` again
4. Text appears, cleaned!

See [QUICKSTART.md](QUICKSTART.md) for detailed setup.

---

## � How to Use

**Basic workflow:**
1. Click in any text field
2. Press `Pause` key
3. Speak naturally
4. Press `Pause` again
5. Text appears, polished!

**Works everywhere:** Slack, Gmail, VS Code, Notion, browsers, any text field

**System tray:** Right-click icon for settings (toggle AI cleanup, debug mode, exit)

---

## ⚙️ Configuration

Edit `.env` to customize:

```bash
# Required
GROQ_API_KEY=gsk_your_key_here

# Optional
HOTKEY=pause                         # pause, ctrl+shift+space, f13
ENABLE_AI_CLEANUP=true               # true/false
WHISPER_MODEL=whisper-large-v3       # or whisper-large-v3-turbo
LLM_MODEL=llama-3.1-8b-instant       # or llama-3.3-70b-versatile
DEBUG=false                          # true for detailed logs
```

---

## 🔧 Troubleshooting

| Problem | Solution |
|---------|----------|
| **No API key** | Edit `.env` and add `GROQ_API_KEY=gsk_...` |
| **Hotkey not working** | Try `HOTKEY=ctrl+shift+space` in `.env` |
| **No audio device** | Check Windows Settings → Sound → Input |
| **Text not inserting** | Click in text field first (cursor blinking) |
| **Poor quality** | Speak clearly, reduce background noise |

**Debug mode:** Set `DEBUG=true` in `.env`, then run: `uv run groq-flow --no-tray`

---

## 💰 Cost

~$5/month for typical use vs $7-15/month for Wispr Flow subscription.

Get your free API key: [console.groq.com](https://console.groq.com/)

---

##  License

MIT License - Use freely!

---

**Made with ❤️ for productive dictation** 🎤✨
