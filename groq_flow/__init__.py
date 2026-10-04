"""
groq_flow backwards compatibility shim.
Redirects to the official WordInk engine.
"""
import sys
from wordink import *

__all__ = [
    "WordInkEngine",
    "transcribe",
    "config",
    "Config",
    "AudioRecorder",
    "GroqService",
    "TextInserter",
    "telemetry",
]
