# SPDX-License-Identifier: MIT
"""A stand-in for the runtime's CLI script, for the worker tests.

Like the real script it exposes `main(argv)`, prints one JSON report to
stdout, returns an int, and raises refusals as exceptions. Behavior is picked
by a substring of the audio file's name; with no audio it runs the "golden
self-test" (set STUB_SELFTEST=mismatch to make that fail).
"""

import json
import os
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPORT = json.loads((HERE / "cli-match.json").read_text())
REFUSAL = (HERE / "cli-refusal.txt").read_text().strip().removeprefix("error: ")
REPORT_SCHEMA = "stub.parakeet-transcribe.v1"
CALLS = 0


class TranscribeRefusal(Exception):
    pass


def main(argv=None):
    global CALLS
    CALLS += 1
    assert argv[0] == "transcribe"
    out = Path(argv[argv.index("-o") + 1])
    audio = argv[1] if not argv[1].startswith("-") else None
    name = Path(audio).name if audio else ""
    if audio:
        mode = Path(audio).parent / "mode"
        if mode.is_file():
            name += mode.read_text().strip()
    # Leaves a file behind, as the real CLI does, so cleanup is observable.
    (out / "stub-output.npy").write_bytes(b"x" * 16)

    if audio is None and os.environ.get("STUB_SELFTEST") == "mismatch":
        print(json.dumps({**REPORT, "status": "mismatch", "out": str(out)}, indent=2))
        return 1
    if "banner" in name:
        print("mlx-omarchy runtime banner {not json}")
        print("loading session...")
    if "refuse" in name:
        raise TranscribeRefusal(REFUSAL)
    if "exit" in name:
        raise SystemExit(1)
    if "crash" in name:
        os._exit(9)
    if "hang" in name:
        time.sleep(600)
    if "slow" in name:
        time.sleep(0.4)
    if "garbage" in name:
        print("this is not a report")
        return 0
    if "fail" in name:
        print(json.dumps({**REPORT, "status": "mismatch", "out": str(out)}, indent=2))
        return 1
    if "stderr" in name:
        print("progress noise", file=sys.stderr)
    # Writes straight to fd 1, as a native library would.
    os.write(1, b"native noise on fd 1\n")
    print(json.dumps({**REPORT, "out": str(out)}, indent=2, ensure_ascii=False))
    return 0
