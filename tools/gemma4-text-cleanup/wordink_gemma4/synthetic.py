"""Bootstrap (raw, cleaned) pairs for dataset building."""

from __future__ import annotations

import json
from pathlib import Path


def default_synthetic_pairs() -> list[dict]:
    path = Path(__file__).with_name("data") / "synthetic_pairs.json"
    return json.loads(path.read_text(encoding="utf-8"))