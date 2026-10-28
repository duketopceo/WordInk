import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))


import pytest


@pytest.fixture(autouse=True)
def _private_usage_log(tmp_path_factory, monkeypatch):
    """No test may write the real ~/.local/state/wordink/usage.jsonl or read the real pricing file."""
    root = tmp_path_factory.mktemp("usage")
    monkeypatch.setenv("WORDINK_USAGE_LOG", str(root / "usage.jsonl"))
    monkeypatch.setenv("XDG_CONFIG_HOME", str(root / "cfg"))
    monkeypatch.delenv("WORDINK_DEBUG", raising=False)
