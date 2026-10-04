"""
WordInk Engine & SDK
====================
Embeddable speech-to-text engine and Grounded GUI voice driver.
Supports direct transcription, live microphone streams, clipboard injection,
and Computer-Using Agent (CUA) intent routing.
"""
import sys
import threading
import time
from typing import Callable, Optional

from .audio import AudioRecorder
from .groq_service import GroqService
from .text_inserter import TextInserter
from .config import config, Config
from .telemetry import telemetry


class WordInkEngine:
    """
    Core embeddable engine for WordInk.
    
    Example:
        >>> from wordink import WordInkEngine
        >>> engine = WordInkEngine(on_transcript=lambda text: print("Said:", text))
        >>> engine.start_recording()
        >>> # speak...
        >>> engine.stop_recording()
    """

    def __init__(
        self,
        api_key: Optional[str] = None,
        model: Optional[str] = None,
        on_transcript: Optional[Callable[[str], None]] = None,
        auto_paste: bool = False,
        hotkey: Optional[str] = None,
    ):
        if api_key:
            config.set("GROQ_API_KEY", api_key)
        if model:
            config.set("WHISPER_MODEL", model)
        if hotkey:
            config.set("HOTKEY", hotkey)

        self.groq_service = GroqService()
        self.audio_recorder = AudioRecorder(on_recording_complete=self._on_audio_data)
        self.text_inserter = TextInserter() if auto_paste else None
        self.on_transcript = on_transcript
        self.auto_paste = auto_paste
        self.is_recording = False
        self._lock = threading.Lock()

    def transcribe(self, audio_data: bytes) -> str:
        """Transcribe raw PCM audio bytes to text synchronously."""
        return self.groq_service.process_audio(audio_data)

    def start_recording(self):
        """Start recording from active microphone."""
        with self._lock:
            if not self.is_recording:
                self.is_recording = True
                self.audio_recorder.start_recording()

    def stop_recording(self):
        """Stop recording and trigger asynchronous transcription pipeline."""
        with self._lock:
            if self.is_recording:
                self.is_recording = False
                self.audio_recorder.stop_recording()

    def toggle_recording(self):
        """Toggle recording between active and stopped."""
        if self.is_recording:
            self.stop_recording()
        else:
            self.start_recording()

    def _on_audio_data(self, audio_data: bytes):
        threading.Thread(
            target=self._process_background,
            args=(audio_data,),
            daemon=True
        ).start()

    def _process_background(self, audio_data: bytes):
        t0 = time.time()
        text = self.groq_service.process_audio(audio_data)
        latency_ms = (time.time() - t0) * 1000.0
        duration_sec = max(0.1, len(audio_data) / 32000.0)

        if text:
            try:
                telemetry.record_session(
                    duration_sec=duration_sec,
                    text=text,
                    latency_ms=latency_ms,
                    model_used=config.whisper_model
                )
            except Exception:
                pass

            if self.auto_paste and self.text_inserter:
                self.text_inserter.insert_text(text)

            if self.on_transcript:
                self.on_transcript(text)


def transcribe(audio_path_or_bytes) -> str:
    """Convenience helper to transcribe an audio file or bytes directly."""
    service = GroqService()
    if isinstance(audio_path_or_bytes, (str, bytes)):
        if isinstance(audio_path_or_bytes, str):
            with open(audio_path_or_bytes, "rb") as f:
                data = f.read()
        else:
            data = audio_path_or_bytes
        return service.process_audio(data)
    raise ValueError("Expected filepath string or audio bytes")
