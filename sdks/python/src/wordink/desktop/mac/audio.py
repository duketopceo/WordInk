"""Microphone capture on macOS through `sounddevice` (CoreAudio), as 16 kHz mono s16 PCM (plan M2).

The capture is shared with Windows in `wordink.desktop.sdaudio`. macOS adds two things: wording that points at
System Settings, and detection of the way a denied microphone usually shows up. When the Microphone permission
is missing for the process that opened the stream, CoreAudio often opens it anyway and delivers only zeros, so a
capture of at least half a second that is exactly silent is reported as a probable denial instead of being sent
to Groq as empty audio.
"""

from __future__ import annotations

from ..recorder import RecorderError
from ..sdaudio import SoundDeviceSource as _SoundDeviceSource
from ..sdaudio import resample_s16

__all__ = ["MIN_SILENT_SECONDS", "SoundDeviceSource", "resample_s16"]

#: A capture at least this long that is all zero samples counts as a denied microphone.
MIN_SILENT_SECONDS = 0.5

NO_DEVICE = "no microphone found: plug one in or choose an input in System Settings, Sound, Input"
SILENT = (
    "the microphone delivered only silence, which on a Mac usually means it is blocked for this app: "
    "turn it on in System Settings, Privacy & Security, Microphone (for the program that runs wordink-dictd), "
    "then restart it"
)


class SoundDeviceSource(_SoundDeviceSource):
    NO_DEVICE = NO_DEVICE
    DENIED = (
        "macOS blocked microphone access: turn it on in System Settings, Privacy & Security, Microphone "
        "(for the program that runs wordink-dictd), then restart it"
    )
    EXTRA = "macos"

    def _check_capture(self, pcm: bytes) -> None:
        # The check runs on the raw capture, before resampling; zeros stay zeros either way.
        seconds = len(pcm) / 2 / self._rate
        if seconds >= MIN_SILENT_SECONDS and not any(pcm):
            raise RecorderError(SILENT)
