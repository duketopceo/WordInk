#!/bin/sh
set -eu
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT/tools/gemma4-text-cleanup"
PYTHONPATH=. python3 -m pytest tests -q