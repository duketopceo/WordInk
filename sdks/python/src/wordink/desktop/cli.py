"""`wordink-dictd`: run the daemon, drive it from a Hyprland bind, and manage the local vocabulary."""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import signal
import sys
from pathlib import Path

from ..client import WordInkError
from .binds import hyprland_bind_lines
from .config import DictateConfig, load_config
from .daemon import Daemon, LastTranscriptSink, build_pipeline, send_command
from .vocab import add_terms, read_terms, remove_term


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="wordink-dictd", description="WordInk push-to-talk dictation daemon")
    p.add_argument("--config", type=Path, help="path to dictate.toml (default ~/.config/wordink/dictate.toml)")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("run", help="run the daemon in the foreground (the systemd unit does this)")
    t = sub.add_parser("trigger", help="tell the daemon to start, stop or toggle recording (what the Hyprland bind runs)")
    t.add_argument("action", choices=("start", "stop", "toggle"))
    sub.add_parser("status", help="print the daemon's state as JSON")
    sub.add_parser("last", help="print the last cleaned transcript")
    sub.add_parser("binds", help="print the Hyprland bind lines for the configured keys")
    v = sub.add_parser("vocab", help="manage the local vocabulary file")
    vs = v.add_subparsers(dest="vocab_cmd", required=True)
    vs.add_parser("list")
    va = vs.add_parser("add")
    va.add_argument("terms", nargs="+")
    vr = vs.add_parser("remove")
    vr.add_argument("term")
    return p


async def _run(cfg: DictateConfig) -> int:
    daemon = Daemon(cfg, build_pipeline(cfg), LastTranscriptSink(cfg.last_path))
    await daemon.serve()
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set)
    logging.getLogger("wordink.dictd").info("listening on %s", cfg.socket_path)
    await stop.wait()
    await daemon.close()
    return 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s", stream=sys.stderr)
    try:
        cfg = load_config(args.config)
        if args.cmd == "run":
            return asyncio.run(_run(cfg))
        if args.cmd in ("trigger", "status"):
            reply = asyncio.run(send_command(cfg.socket_path, args.action if args.cmd == "trigger" else "status"))
            print(json.dumps(reply))
            return 0 if reply.get("ok") else 1
        if args.cmd == "last":
            try:
                sys.stdout.write(cfg.last_path.read_text(encoding="utf-8"))
            except FileNotFoundError:
                print("wordink-dictd: no dictation yet", file=sys.stderr)
                return 1
            return 0
        if args.cmd == "binds":
            print("\n".join(hyprland_bind_lines(cfg.hold_bind, cfg.toggle_bind)))
            return 0
        if args.cmd == "vocab":
            if args.vocab_cmd == "list":
                print("\n".join(read_terms(cfg.vocab_path)))
            elif args.vocab_cmd == "add":
                added = add_terms(cfg.vocab_path, args.terms)
                print(json.dumps({"added": added}))
            else:
                print(json.dumps({"removed": remove_term(cfg.vocab_path, args.term)}))
            return 0
    except WordInkError as exc:
        print(f"wordink-dictd: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
