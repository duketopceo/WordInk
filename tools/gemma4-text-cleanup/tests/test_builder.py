import json
import tempfile
from pathlib import Path

from wordink_gemma4.builder import build_training_rows
from wordink_gemma4.history_export import load_history_pairs


def test_history_export_skips_raw_entries():
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "history.json"
        path.write_text(
            json.dumps(
                {
                    "entries": [
                        {
                            "kind": "dictation",
                            "raw_text": "um hi",
                            "final_text": "Hi",
                        },
                        {
                            "kind": "raw",
                            "raw_text": "unchanged",
                            "final_text": "unchanged",
                        },
                    ]
                }
            ),
            encoding="utf-8",
        )
        rows = load_history_pairs(path)
        assert len(rows) == 1
        assert rows[0]["completion"] == "Hi"


def test_build_training_rows_includes_synthetic_and_general_slice():
    rows = build_training_rows(
        profile_prompt="Global",
        synthetic=[{"raw": "a", "completion": "A"}],
        history=[],
        general_slice=0.5,
        seed=1,
        style="chat",
    )
    assert any(r["messages"][1]["content"] == "A" for r in rows)
    assert len(rows) >= 2