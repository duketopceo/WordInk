"""Microphone capture on Windows through `sounddevice` (WASAPI), as 16 kHz mono s16 PCM (plan W3).

The capture itself is shared with macOS in `wordink.desktop.sdaudio`; this module pins the Windows wording.
"""

from __future__ import annotations

from ..sdaudio import SoundDeviceSource as _SoundDeviceSource
from ..sdaudio import resample_s16

__all__ = ["SoundDeviceSource", "resample_s16"]


class SoundDeviceSource(_SoundDeviceSource):
    NO_DEVICE = "no microphone found: plug one in or set a default input device in Settings, System, Sound"
    DENIED = "Windows blocked microphone access: turn on Settings, Privacy & security, Microphone, 'Let desktop apps access your microphone'"
    EXTRA = "windows"
