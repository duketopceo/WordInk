"""wordink — dictation tooling that talks to a wordink-gateway."""

from .client import WordInkError, list_models, transcribe

__all__ = ["WordInkError", "list_models", "transcribe"]
__version__ = "0.1.0"
