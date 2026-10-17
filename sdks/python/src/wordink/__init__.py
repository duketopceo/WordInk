"""wordink — dictation tooling that talks to a wordink-gateway."""

from .client import WordInkError, list_models, transcribe
from .flow import (
    FlowOptions,
    FlowTone,
    LearnedVocabularyDiff,
    apply_flow_cleanup,
    apply_tone_preset,
    expand_snippets,
    format_spoken_lists,
    learn_vocabulary_from_edit,
    resolve_course_corrections,
    strip_fillers,
)

__all__ = [
    "FlowOptions",
    "FlowTone",
    "LearnedVocabularyDiff",
    "WordInkError",
    "apply_flow_cleanup",
    "apply_tone_preset",
    "expand_snippets",
    "format_spoken_lists",
    "learn_vocabulary_from_edit",
    "list_models",
    "resolve_course_corrections",
    "strip_fillers",
    "transcribe",
]
__version__ = "0.1.0"
