# 🚀 Quick Start

Get Groq Flow running in 5 minutes!

---

## 1. Get API Key (2 min)

1. Visit [console.groq.com](https://console.groq.com/)
2. Sign up (free)
3. Create API key
4. Copy it (starts with `gsk_...`)

---

## 2. Install (1 min)

```powershell
.\setup.ps1
```

Or manually:
```powershell
uv sync
copy .env.example .env
```

---

## 3. Configure (1 min)

Edit `.env`:
```bash
GROQ_API_KEY=gsk_your_key_here
```

---

## 4. Run (30 sec)

```powershell
.\run.ps1
```

Or:
```powershell
uv run groq-flow
```

---

## 5. Test (1 min)

1. Open Notepad
2. Press `Pause` key
3. Say: "Hello, um, this is a test"
4. Press `Pause` again
5. Text appears: "Hello, this is a test"

✨ Filler words removed automatically!

---

## 🎯 Tips

**Speaking:**
- Talk naturally
- Don't worry about "um", "uh", "like"
- Corrections are handled automatically

**Usage:**
- Works in any app: Slack, Gmail, VS Code
- Click in text field first
- Wait for recording to finish before next one

**Hotkey:**
- Default: `Pause` key
- Change in `.env`: `HOTKEY=ctrl+shift+space`

---

## 🔧 Troubleshooting

| Problem | Fix |
|---------|-----|
| No API key error | Edit `.env`, add your key |
| Hotkey not working | Try `ctrl+shift+space` in `.env` |
| No microphone | Check Windows Settings → Sound → Input |
| Text not appearing | Click in text field first |

**Debug mode:** Set `DEBUG=true` in `.env`

---

## 💰 Cost

~$5/month typical use (much cheaper than Wispr Flow's $7-15/month!)

---

**That's it! Start dictating everywhere!** 🎤

See [README.md](README.md) for full documentation.
