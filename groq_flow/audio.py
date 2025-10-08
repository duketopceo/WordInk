import pyaudio
import wave
import io
import threading
import time
import numpy as np
import webrtcvad
from typing import Optional, Callable
from pathlib import Path

from .config import config


class AudioRecorder:
    """Handles audio recording with voice activity detection"""

    def __init__(self, on_recording_complete: Optional[Callable[[bytes], None]] = None):
        """
        Initialize audio recorder

        Args:
            on_recording_complete: Callback function called with audio data when recording stops
        """
        self.sample_rate = config.sample_rate
        self.channels = config.channels
        self.chunk_size = config.chunk_size

        self.on_recording_complete = on_recording_complete

        self.audio = pyaudio.PyAudio()
        self.stream: Optional[pyaudio.Stream] = None
        self.is_recording = False
        self.frames = []

        # VAD setup (Voice Activity Detection)
        self.vad = webrtcvad.Vad(2)  # Aggressiveness: 0-3, 2 is balanced
        self.silence_threshold = 30  # Number of silent chunks before stopping
        self.silence_counter = 0

        self.recording_thread: Optional[threading.Thread] = None

    def start_recording(self):
        """Start recording audio from microphone"""
        if self.is_recording:
            return

        self.is_recording = True
        self.frames = []
        self.silence_counter = 0

        # Start recording in a separate thread
        self.recording_thread = threading.Thread(
            target=self._record_audio, daemon=True)
        self.recording_thread.start()

    def stop_recording(self):
        """Stop recording and process audio"""
        if not self.is_recording:
            return

        self.is_recording = False

        # Wait for recording thread to finish
        if self.recording_thread:
            self.recording_thread.join(timeout=2.0)

    def _record_audio(self):
        """Internal method to record audio (runs in separate thread)"""
        try:
            self.stream = self.audio.open(
                format=pyaudio.paInt16,
                channels=self.channels,
                rate=self.sample_rate,
                input=True,
                frames_per_buffer=self.chunk_size
            )

            print("🎤 Recording started...")

            while self.is_recording:
                try:
                    data = self.stream.read(
                        self.chunk_size, exception_on_overflow=False)
                    self.frames.append(data)

                    # Voice Activity Detection
                    if self._is_speech(data):
                        self.silence_counter = 0
                    else:
                        self.silence_counter += 1

                    # Auto-stop after sustained silence (optional feature)
                    # Uncomment if you want auto-stop on silence
                    # if self.silence_counter > self.silence_threshold and len(self.frames) > 10:
                    #     print("🔇 Silence detected, stopping recording...")
                    #     break

                except Exception as e:
                    print(f"Error reading audio: {e}")
                    break

            self.stream.stop_stream()
            self.stream.close()

            # Process recorded audio
            if self.frames:
                audio_data = self._frames_to_wav()
                print(
                    f"✅ Recording complete: {len(self.frames)} chunks, {len(audio_data)} bytes")

                if self.on_recording_complete:
                    self.on_recording_complete(audio_data)
            else:
                print("⚠️ No audio data recorded")

        except Exception as e:
            print(f"❌ Recording error: {e}")
        finally:
            if self.stream:
                try:
                    self.stream.stop_stream()
                    self.stream.close()
                except:
                    pass

    def _is_speech(self, data: bytes) -> bool:
        """
        Check if audio chunk contains speech using WebRTC VAD

        Args:
            data: Audio chunk as bytes

        Returns:
            True if speech is detected, False otherwise
        """
        try:
            # VAD expects 10, 20, or 30ms frames at 8000, 16000, 32000, or 48000 Hz
            # We're using 30ms frames (480 samples at 16000 Hz)
            return self.vad.is_speech(data, self.sample_rate)
        except Exception as e:
            # If VAD fails, assume it's speech
            return True

    def _frames_to_wav(self) -> bytes:
        """
        Convert recorded frames to WAV format

        Returns:
            WAV audio data as bytes
        """
        wav_buffer = io.BytesIO()

        with wave.open(wav_buffer, 'wb') as wf:
            wf.setnchannels(self.channels)
            wf.setsampwidth(self.audio.get_sample_size(pyaudio.paInt16))
            wf.setframerate(self.sample_rate)
            wf.writeframes(b''.join(self.frames))

        return wav_buffer.getvalue()

    def save_recording(self, filename: str):
        """Save last recording to file (for debugging)"""
        if not self.frames:
            print("No recording to save")
            return

        audio_data = self._frames_to_wav()
        path = Path(filename)
        path.parent.mkdir(parents=True, exist_ok=True)

        with open(path, 'wb') as f:
            f.write(audio_data)

        print(f"💾 Recording saved to {filename}")

    def cleanup(self):
        """Clean up audio resources"""
        if self.is_recording:
            self.stop_recording()

        if self.stream:
            try:
                self.stream.close()
            except:
                pass

        try:
            self.audio.terminate()
        except:
            pass
