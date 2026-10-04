import threading
import time
import keyboard
from typing import Optional

from .audio import AudioRecorder
from .groq_service import GroqService
from .text_inserter import TextInserter
from .overlay import get_overlay, RecorderState
from .config import config


class GroqFlowApp:
    """Main application controller"""

    def __init__(self):
        """Initialize application"""
        self.is_running = False
        self.is_recording = False

        # Initialize components
        try:
            self.groq_service = GroqService()
            self.audio_recorder = AudioRecorder(
                on_recording_complete=self._on_audio_complete)
            self.text_inserter = TextInserter()
            self.overlay = get_overlay()
        except Exception as e:
            print(f"❌ Initialization error: {e}")
            raise

        # Hotkey setup
        self.hotkey = config.hotkey
        print(f"🔑 Hotkey configured: {self.hotkey}")

    def start(self):
        """Start the application"""
        if self.is_running:
            return

        self.is_running = True
        print(f"🚀 Groq Flow started!")
        print(f"🎤 Press '{self.hotkey}' to start/stop recording")
        print(
            f"✨ AI cleanup: {'Enabled' if config.enable_ai_cleanup else 'Disabled'}")
        print(f"⚙️  Model: {config.whisper_model}")
        print()

        # Ensure overlay starts hidden / idle
        self.overlay.update_state(RecorderState.IDLE)

        # Register global hotkey
        try:
            keyboard.add_hotkey(
                self.hotkey, self._toggle_recording, suppress=True)
        except Exception as e:
            print(f"❌ Error registering hotkey '{self.hotkey}': {e}")
            print("Please check your HOTKEY configuration in .env")
            self.is_running = False
            return

        # Keep application running
        try:
            while self.is_running:
                time.sleep(0.1)
        except KeyboardInterrupt:
            print("\n⚠️  Shutting down...")
            self.stop()

    def stop(self):
        """Stop the application"""
        if not self.is_running:
            return

        self.is_running = False

        # Stop recording if active
        if self.is_recording:
            self.audio_recorder.stop_recording()

        # Unregister hotkey
        try:
            keyboard.remove_hotkey(self.hotkey)
        except:
            pass

        # Cleanup
        self.audio_recorder.cleanup()
        self.overlay.hide()

        print("👋 Groq Flow stopped")

    def _toggle_recording(self):
        """Toggle recording on/off (called by hotkey)"""
        if self.is_recording:
            self._stop_recording()
        else:
            self._start_recording()

    def _start_recording(self):
        """Start recording audio"""
        if self.is_recording:
            return

        self.is_recording = True
        print("\n🎤 Recording...")

        # Update overlay
        self.overlay.update_state(RecorderState.RECORDING)

        # Start audio recording
        self.audio_recorder.start_recording()

    def _stop_recording(self):
        """Stop recording audio"""
        if not self.is_recording:
            return

        self.is_recording = False
        print("⏹️  Recording stopped")

        # Update overlay
        self.overlay.update_state(RecorderState.PROCESSING)

        # Stop audio recording (will trigger callback)
        self.audio_recorder.stop_recording()

    def _on_audio_complete(self, audio_data: bytes):
        """
        Callback when audio recording is complete
        Processes audio in a separate thread

        Args:
            audio_data: Recorded audio data
        """
        # Process in background thread to not block hotkey handling
        thread = threading.Thread(
            target=self._process_audio,
            args=(audio_data,),
            daemon=True
        )
        thread.start()

    def _process_audio(self, audio_data: bytes):
        """
        Process recorded audio: transcribe, clean, and insert

        Args:
            audio_data: Recorded audio data
        """
        try:
            t0 = time.time()
            # Process audio through Groq API
            text = self.groq_service.process_audio(audio_data)
            latency_ms = (time.time() - t0) * 1000.0
            audio_duration_sec = max(0.1, len(audio_data) / 32000.0)

            if text:
                try:
                    from .telemetry import telemetry
                    telemetry.record_session(
                        duration_sec=audio_duration_sec,
                        text=text,
                        latency_ms=latency_ms,
                        model_used=config.whisper_model
                    )
                except Exception:
                    pass

                # Insert text at cursor
                print(f"📝 Inserting: {text} ({latency_ms:.0f}ms)")
                success = self.text_inserter.insert_text(text)

                if success:
                    print("✅ Text inserted successfully!")
                else:
                    print("❌ Failed to insert text")
                    self.overlay.update_state(RecorderState.ERROR)
                    time.sleep(1)
            else:
                print("❌ No text generated")
                self.overlay.update_state(RecorderState.ERROR)
                time.sleep(2)

        except Exception as e:
            print(f"❌ Processing error: {e}")
            self.overlay.update_state(RecorderState.ERROR)
            time.sleep(2)

        finally:
            # Return to idle state
            self.overlay.update_state(RecorderState.IDLE)
