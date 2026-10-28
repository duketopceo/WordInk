"""The daemon's state file: one JSON line the bar, the notifier and `status` all read."""

from __future__ import annotations

import json
import os
import tempfile
import time
from pathlib import Path

STATES = ("idle", "recording", "transcribing", "error")


class StateFile:
    def __init__(self, path: Path) -> None:
        self.path = path

    def write(self, state: str, detail: str | None = None) -> None:
        if state not in STATES:
            raise ValueError(f"unknown state {state!r}")
        payload = {"state": state, "ts": round(time.time(), 3)}
        if detail:
            payload["detail"] = detail
        self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd, tmp = tempfile.mkstemp(dir=self.path.parent, prefix=".state-")
        try:
            with os.fdopen(fd, "w", encoding="utf-8") as fh:
                fh.write(json.dumps(payload) + "\n")
            os.replace(tmp, self.path)
        except BaseException:
            Path(tmp).unlink(missing_ok=True)
            raise

    def read(self) -> dict:
        try:
            return json.loads(self.path.read_text(encoding="utf-8"))
        except (FileNotFoundError, ValueError):
            return {"state": "idle"}
