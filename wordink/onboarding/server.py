import http.server
import json
import os
import sys
import threading
import time
import urllib.parse
import webbrowser
from pathlib import Path
from typing import Dict, Any, List

from ..config import config


STATIC_DIR = Path(__file__).parent / "static"


def probe_hotkey_availability(hotkey_str: str) -> tuple[bool, str]:
    """
    Dynamically test if a hotkey is available or already claimed by another app/OS.
    Uses Win32 RegisterHotKey probe on Windows.
    """
    if sys.platform == "win32":
        try:
            import ctypes
            user32 = ctypes.windll.user32
            kernel32 = ctypes.windll.kernel32
            MODS = {'alt': 0x0001, 'ctrl': 0x0002, 'shift': 0x0004, 'win': 0x0008}
            parts = [p.strip().lower() for p in hotkey_str.split("+")]
            mod_val = 0
            key_part = ""
            for p in parts:
                if p in MODS:
                    mod_val |= MODS[p]
                else:
                    key_part = p.upper()

            if not key_part and not mod_val:
                return False, "Invalid hotkey format"

            if len(key_part) == 1:
                vk = ord(key_part)
            elif key_part == "SPACE":
                vk = 0x20
            elif key_part.startswith("F") and key_part[1:].isdigit():
                vk = 0x70 + int(key_part[1:]) - 1
            elif key_part == "PAUSE":
                vk = 0x13
            elif key_part == "INSERT":
                vk = 0x2D
            elif key_part in ("RETURN", "ENTER"):
                vk = 0x0D
            elif key_part == "TAB":
                vk = 0x09
            else:
                vk = 0x20

            # Test register with temporary ID
            test_id = 19999
            res = user32.RegisterHotKey(None, test_id, mod_val | 0x4000, vk)
            if res != 0:
                user32.UnregisterHotKey(None, test_id)
                return True, "Available"
            err = kernel32.GetLastError()
            if err == 1409:
                return False, "Already in use by another application (Conflict)"
            return False, f"System reserved (Error {err})"
        except Exception as e:
            return True, f"Unverified ({e})"
    elif sys.platform.startswith("linux"):
        # Check hyprctl binds if on Hyprland
        try:
            import subprocess
            r = subprocess.run(["hyprctl", "binds", "-j"], capture_output=True, text=True, timeout=2)
            if r.returncode == 0:
                binds = json.loads(r.stdout)
                for b in binds:
                    # e.g. modmask and key
                    if hotkey_str.lower() in str(b).lower():
                        return False, f"Bound in Hyprland to {b.get('dispatcher', '')}"
        except Exception:
            pass
        return True, "Available"
    return True, "Available"


class OnboardingHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(STATIC_DIR), **kwargs)

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path == "/api/status":
            self._handle_status()
        elif parsed.path == "/api/mics":
            self._handle_mics()
        elif parsed.path == "/api/mic_test":
            self._handle_mic_test()
        elif parsed.path == "/api/scan_hotkeys":
            self._handle_scan_hotkeys()
        elif parsed.path == "/api/stats":
            self._handle_stats()
        elif parsed.path in ("/", "/index.html"):
            self.path = "/index.html"
            super().do_GET()
        else:
            super().do_GET()

    def do_POST(self):
        parsed = urllib.parse.urlparse(self.path)
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length).decode("utf-8") if content_length > 0 else "{}"
        try:
            data = json.loads(body)
        except Exception:
            data = {}

        if parsed.path == "/api/validate_key":
            self._handle_validate_key(data)
        elif parsed.path == "/api/probe_hotkey":
            self._handle_probe_hotkey(data)
        elif parsed.path == "/api/save":
            self._handle_save(data)
        else:
            self._send_json({"error": "Not found"}, status=404)

    def _send_json(self, data: Any, status: int = 200):
        response_bytes = json.dumps(data).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(response_bytes)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(response_bytes)

    def _handle_status(self):
        self._send_json({
            "api_key_set": bool(config.groq_api_key and config.groq_api_key not in ("your_groq_key_here", "")),
            "whisper_model": config.whisper_model,
            "hotkey": config.hotkey,
            "enable_ai_cleanup": config.enable_ai_cleanup,
        })

    def _handle_mics(self):
        mics = []
        default_index = -1
        try:
            import pyaudio
            p = pyaudio.PyAudio()
            try:
                def_info = p.get_default_input_device_info()
                default_index = def_info.get("index", -1)
            except Exception:
                pass

            for i in range(p.get_device_count()):
                try:
                    d = p.get_device_info_by_index(i)
                    if d.get("maxInputChannels", 0) > 0:
                        name = d.get("name", f"Device {i}")
                        is_virtual = any(v in name.lower() for v in ("steam streaming", "virtual", "wave", "mapper"))
                        mics.append({
                            "index": i,
                            "name": name,
                            "is_default": (i == default_index),
                            "is_virtual": is_virtual
                        })
                except Exception:
                    pass
            p.terminate()
        except Exception as e:
            mics = [{"index": 0, "name": f"Error loading audio: {e}", "is_default": True, "is_virtual": False}]

        self._send_json({"devices": mics, "default_index": default_index})

    def _handle_mic_test(self):
        level = 0
        try:
            import pyaudio
            import numpy as np
            p = pyaudio.PyAudio()
            stream = p.open(format=pyaudio.paInt16, channels=1, rate=16000, input=True, frames_per_buffer=1024)
            data = stream.read(1024 * 4, exception_on_overflow=False)
            stream.stop_stream()
            stream.close()
            p.terminate()
            audio = np.frombuffer(data, dtype=np.int16)
            peak = float(np.max(np.abs(audio)))
            level = min(100, int((peak / 32767.0) * 200))
        except Exception:
            level = 0

        self._send_json({"audio_level": level})

    def _handle_scan_hotkeys(self):
        candidates = [
            {"hotkey": "alt+d", "label": "Alt + D", "desc": "Ergonomic left hand ('D' for Dictate)", "recommended": True},
            {"hotkey": "ctrl+space", "label": "Ctrl + Space", "desc": "Common coding / quick launcher trigger", "recommended": True},
            {"hotkey": "alt+space", "label": "Alt + Space", "desc": "Popular launcher chord (often used by Gemini/PowerToys)", "recommended": False},
            {"hotkey": "ctrl+shift+space", "label": "Ctrl + Shift + Space", "desc": "Wispr Flow default (often taken by IDEs)", "recommended": False},
            {"hotkey": "alt+q", "label": "Alt + Q", "desc": "Ultra-fast thumb + index key", "recommended": True},
            {"hotkey": "ctrl+alt+space", "label": "Ctrl + Alt + Space", "desc": "Safe 3-key combo with almost zero conflict", "recommended": True},
            {"hotkey": "f13", "label": "F13", "desc": "Dedicated macro key / remapped CapsLock", "recommended": False},
            {"hotkey": "pause", "label": "Pause / Break", "desc": "Dedicated standalone key", "recommended": False},
            {"hotkey": "insert", "label": "Insert", "desc": "Dedicated standalone key above arrow cluster", "recommended": False},
        ]

        results = []
        for c in candidates:
            avail, reason = probe_hotkey_availability(c["hotkey"])
            results.append({
                **c,
                "available": avail,
                "reason": reason
            })

        self._send_json({"hotkeys": results})

    def _handle_probe_hotkey(self, data: Dict[str, Any]):
        hotkey = data.get("hotkey", "").strip()
        if not hotkey:
            self._send_json({"available": False, "reason": "Empty hotkey"}, status=400)
            return

        avail, reason = probe_hotkey_availability(hotkey)
        self._send_json({"hotkey": hotkey, "available": avail, "reason": reason})

    def _handle_validate_key(self, data: Dict[str, Any]):
        key = data.get("api_key", "").strip()
        if not key:
            self._send_json({"valid": False, "error": "API key cannot be empty"}, status=400)
            return

        try:
            from groq import Groq
            client = Groq(api_key=key)
            models = client.models.list()
            whisper_models = []
            for m in models.data:
                if "whisper" in m.id:
                    whisper_models.append(m.id)

            self._send_json({
                "valid": True,
                "models": whisper_models,
                "recommended": "whisper-large-v3-turbo"
            })
        except Exception as e:
            self._send_json({"valid": False, "error": str(e)}, status=400)

    def _handle_stats(self):
        try:
            from ..telemetry import telemetry
            stats = telemetry.get_stats()
            recent = telemetry.get_recent_history(5)
            self._send_json({"stats": stats, "recent": recent})
        except Exception as e:
            self._send_json({"error": str(e)}, status=500)

    def _handle_save(self, data: Dict[str, Any]):
        api_key = data.get("api_key")
        hotkey = data.get("hotkey", "alt+d")
        model = data.get("model", "whisper-large-v3-turbo")

        env_path = Path(__file__).parent.parent.parent / ".env"
        if env_path.exists():
            content = env_path.read_text(encoding="utf-8")
            if api_key:
                import re
                content = re.sub(r"GROQ_API_KEY=.*", f"GROQ_API_KEY={api_key}", content)
            if hotkey:
                import re
                content = re.sub(r"HOTKEY=.*", f"HOTKEY={hotkey}", content)
            if model:
                import re
                content = re.sub(r"WHISPER_MODEL=.*", f"WHISPER_MODEL={model}", content)
            env_path.write_text(content, encoding="utf-8")

        # Also update user config
        if api_key:
            config.set("GROQ_API_KEY", api_key)
        if hotkey:
            config.set("HOTKEY", hotkey)
        if model:
            config.set("WHISPER_MODEL", model)

        self._send_json({"success": True})


def start_onboarding_server(port: int = 18981, open_browser: bool = True):
    """Start local onboarding server and open browser"""
    server = http.server.HTTPServer(("127.0.0.1", port), OnboardingHandler)
    url = f"http://localhost:{port}"
    print(f"🚀 WordInk Setup & Dashboard: {url}")

    if open_browser:
        threading.Thread(target=lambda: (time.sleep(0.5), webbrowser.open(url)), daemon=True).start()

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\n👋 Wizard closed.")
        server.server_close()
