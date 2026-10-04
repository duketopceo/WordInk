"""
WordInk: Ultra-Fast Voice Dictation, Embeddable SDK, and CUA Grounded Voice Driver
=================================================================================
Powered by Groq Whisper Turbo (~300ms latency).
"""

__version__ = "0.2.0"

from .engine import WordInkEngine, transcribe
from .config import config, Config
from .audio import AudioRecorder
from .groq_service import GroqService
from .text_inserter import TextInserter
from .telemetry import telemetry

__all__ = [
    "WordInkEngine",
    "transcribe",
    "config",
    "Config",
    "AudioRecorder",
    "GroqService",
    "TextInserter",
    "telemetry",
    "__version__",
]
