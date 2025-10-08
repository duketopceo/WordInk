import os
import json
from pathlib import Path
from typing import Any, Dict, Optional
from dotenv import load_dotenv

# Load environment variables
load_dotenv()


class Config:
    """Application configuration manager"""

    def __init__(self):
        self.config_file = Path.home() / ".groq_flow" / "config.json"
        self.config_file.parent.mkdir(parents=True, exist_ok=True)

        # Load user config if exists
        self.user_config = self._load_user_config()

    def _load_user_config(self) -> Dict[str, Any]:
        """Load user configuration from file"""
        if self.config_file.exists():
            try:
                with open(self.config_file, 'r') as f:
                    return json.load(f)
            except Exception as e:
                print(f"Error loading config: {e}")
                return {}
        return {}

    def save_user_config(self):
        """Save user configuration to file"""
        try:
            with open(self.config_file, 'w') as f:
                json.dump(self.user_config, f, indent=2)
        except Exception as e:
            print(f"Error saving config: {e}")

    def get(self, key: str, default: Any = None) -> Any:
        """Get configuration value"""
        # Check user config first, then environment variables
        if key in self.user_config:
            return self.user_config[key]
        return os.getenv(key, default)

    def set(self, key: str, value: Any):
        """Set configuration value"""
        self.user_config[key] = value
        self.save_user_config()

    # API Configuration
    @property
    def groq_api_key(self) -> Optional[str]:
        return self.get("GROQ_API_KEY")

    @property
    def whisper_model(self) -> str:
        return self.get("WHISPER_MODEL", "whisper-large-v3")

    @property
    def llm_model(self) -> str:
        return self.get("LLM_MODEL", "llama-3.1-8b-instant")

    # Audio Configuration
    @property
    def sample_rate(self) -> int:
        return int(self.get("SAMPLE_RATE", 16000))

    @property
    def channels(self) -> int:
        return int(self.get("CHANNELS", 1))

    @property
    def chunk_size(self) -> int:
        return int(self.get("CHUNK_SIZE", 480))

    # Hotkey Configuration
    @property
    def hotkey(self) -> str:
        return self.get("HOTKEY", "pause")

    # Feature Flags
    @property
    def enable_ai_cleanup(self) -> bool:
        value = self.get("ENABLE_AI_CLEANUP", "true")
        return value.lower() in ("true", "1", "yes") if isinstance(value, str) else bool(value)

    @property
    def debug(self) -> bool:
        value = self.get("DEBUG", "false")
        return value.lower() in ("true", "1", "yes") if isinstance(value, str) else bool(value)

    @property
    def auto_start(self) -> bool:
        value = self.get("AUTO_START", "false")
        return value.lower() in ("true", "1", "yes") if isinstance(value, str) else bool(value)


# Global configuration instance
config = Config()
