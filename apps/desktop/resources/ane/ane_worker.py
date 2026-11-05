#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) WordInk contributors.
"""Resident worker for WordInk's Apple Neural Engine Parakeet engine.

Run by the interpreter of the user-installed mlx-omarchy runtime, never
bundled with it. It loads the runtime's CLI script once, then serves
transcription requests one at a time, so the runtime's resident session
stays warm between dictations. It drives the CLI's documented command line
(`transcribe <wav> -o <scratch> --deadline-ms N`) through the script's
`main`, not its private functions.

Protocol: JSON lines. Requests arrive on stdin, replies leave on a private
duplicate of the original stdout; fd 1 is then pointed at stderr so nothing
the runtime prints can corrupt a reply. stdin EOF means exit.

  {"id": 1, "op": "hello"}
      -> {"id": 1, "ok": true, "protocol": 1, "runtime": "...", "pid": N,
          "pipeline_ms": N}        (after the golden self-test matched)
  {"id": 2, "op": "transcribe", "wav": "<path>", "deadline_ms": 8000}
      -> {"id": 2, "ok": true, "transcript": "...", "pipeline_ms": N}
      -> {"id": 2, "ok": false, "code": "refused", "message": "..."}
  {"id": 3, "op": "shutdown"} -> {"id": 3, "ok": true}, then exit

`deadline_ms` on a request is the supervisor's own timeout and is not passed
on: the runtime keys its shared session on its `--deadline-ms`, so one fixed
value is chosen at start (`--deadline-ms`).
"""

import argparse
import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
import shutil
import sys
import tempfile

PROTOCOL = 1
MAX_LINE = 64 * 1024
MAX_MESSAGE = 400


def reply(channel, payload):
    channel.write(json.dumps(payload, ensure_ascii=False) + "\n")
    channel.flush()


def failure(request_id, code, message):
    return {
        "id": request_id,
        "ok": False,
        "code": code,
        "message": str(message)[:MAX_MESSAGE],
    }


def load_cli(path):
    loader = importlib.machinery.SourceFileLoader("ane_runtime_cli", path)
    spec = importlib.util.spec_from_loader("ane_runtime_cli", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def parse_report(text):
    """The CLI prints one JSON object; a banner may precede it."""
    decoder = json.JSONDecoder()
    offset = 0
    candidates = []
    for line in text.splitlines(keepends=True):
        if line.startswith("{"):
            candidates.append(offset)
        offset += len(line)
    for start in candidates:
        try:
            value, _ = decoder.raw_decode(text, start)
        except ValueError:
            continue
        if isinstance(value, dict) and "status" in value:
            return value
    return None


class Worker:
    def __init__(self, cli, work_dir, deadline_ms):
        self.cli = cli
        self.work_dir = work_dir
        self.deadline_ms = deadline_ms

    def run_cli(self, wav):
        """Run one `transcribe` through the CLI's main. Returns
        (return_code, report, error) where error is an exception or None."""
        scratch = tempfile.mkdtemp(prefix="run-", dir=self.work_dir)
        argv = ["transcribe"]
        if wav is not None:
            argv.append(wav)
        argv += ["-o", scratch, "--deadline-ms", str(self.deadline_ms)]
        captured = io.StringIO()
        code, error = None, None
        try:
            with contextlib.redirect_stdout(captured):
                code = self.cli.main(argv)
        except SystemExit as exit_:
            error = exit_
        except Exception as exc:  # the CLI raises refusals as exceptions
            error = exc
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
        return code, parse_report(captured.getvalue()), error

    def self_test(self):
        code, report, error = self.run_cli(None)
        if error is not None:
            return False, "self-test refused: %s" % describe(error)
        if code not in (0, None) or report is None or report.get("status") != "match":
            status = report.get("status") if report else "no report"
            return False, "self-test did not match (%s)" % status
        # The cache was fully verified once; later calls may trust its stamps.
        os.environ["MLX_OMARCHY_PK_TRUST_CACHE"] = "1"
        return True, report.get("total_pipeline_ms")

    def transcribe(self, request_id, wav):
        code, report, error = self.run_cli(wav)
        if error is not None:
            return failure(request_id, "refused", describe(error)), True
        if report is None:
            return failure(request_id, "bad_output", "no report from the runtime"), False
        if code not in (0, None) or report.get("status") != "match":
            return failure(
                request_id, "bad_status", "status %s" % report.get("status")
            ), False
        return {
            "id": request_id,
            "ok": True,
            "transcript": report.get("transcript", ""),
            "pipeline_ms": report.get("total_pipeline_ms"),
        }, False


def describe(error):
    if isinstance(error, SystemExit):
        return "runtime exited (%s)" % (error.code,)
    return "%s: %s" % (type(error).__name__, error)


def read_line(stream):
    """One request line, or None at EOF. A line over the cap is reported as
    the string 'too_large' after the rest of it is discarded."""
    line = stream.readline(MAX_LINE + 1)
    if not line:
        return None
    if len(line) > MAX_LINE and not line.endswith(b"\n"):
        while True:
            rest = stream.readline(MAX_LINE)
            if not rest or rest.endswith(b"\n"):
                break
        return "too_large"
    return line


def serve(channel, stdin, worker):
    exit_after = False
    while True:
        line = read_line(stdin)
        if line is None:
            return 0
        if line == "too_large":
            reply(channel, failure(None, "too_large", "request line too long"))
            continue
        try:
            request = json.loads(line)
            if not isinstance(request, dict):
                raise ValueError("not an object")
            request_id = request.get("id")
            op = request.get("op")
        except ValueError:
            reply(channel, failure(None, "bad_request", "malformed request"))
            continue
        if op == "hello":
            ok, detail = worker.self_test()
            if not ok:
                reply(channel, failure(request_id, "selftest", detail))
                return 3
            reply(channel, {
                "id": request_id,
                "ok": True,
                "protocol": PROTOCOL,
                "runtime": str(getattr(worker.cli, "REPORT_SCHEMA", "")),
                "pid": os.getpid(),
                "pipeline_ms": detail,
            })
        elif op == "transcribe":
            wav = request.get("wav")
            if not isinstance(wav, str) or not os.path.isabs(wav):
                reply(channel, failure(request_id, "bad_request", "wav must be an absolute path"))
                continue
            payload, exit_after = worker.transcribe(request_id, wav)
            reply(channel, payload)
            if exit_after:
                return 3
        elif op == "shutdown":
            reply(channel, {"id": request_id, "ok": True})
            return 0
        else:
            reply(channel, failure(request_id, "bad_request", "unknown op"))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli-script", required=True)
    parser.add_argument("--work-dir", required=True)
    parser.add_argument("--deadline-ms", type=int, default=20000)
    args = parser.parse_args()

    # Replies go to a private copy of stdout; fd 1 now feeds stderr.
    proto_fd = os.dup(1)
    os.dup2(2, 1)
    channel = os.fdopen(proto_fd, "w", encoding="utf-8")
    os.makedirs(args.work_dir, mode=0o700, exist_ok=True)

    try:
        cli = load_cli(args.cli_script)
    except Exception as exc:
        reply(channel, failure(None, "load", describe(exc)))
        return 2
    worker = Worker(cli, args.work_dir, args.deadline_ms)
    try:
        return serve(channel, sys.stdin.buffer, worker)
    finally:
        for entry in os.listdir(args.work_dir):
            if entry.startswith("run-"):
                shutil.rmtree(os.path.join(args.work_dir, entry), ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
