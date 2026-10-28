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
from .daemon import Daemon, build_pipeline, make_sink, send_command
from . import meeting_cli
from .doctor import exit_code, render, run_checks, to_json
from .notify import notify
from .vocab import add_terms, read_terms, remove_term
from .waybar import follow, snapshot


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="wordink-dictd", description="WordInk push-to-talk dictation daemon")
    p.add_argument("--config", type=Path, help="path to dictate.toml (default ~/.config/wordink/dictate.toml)")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("run", help="run the daemon in the foreground (the systemd unit does this)")
    t = sub.add_parser("trigger", help="tell the daemon to start, stop or toggle recording (what the Hyprland bind runs)")
    t.add_argument("action", choices=("start", "stop", "toggle"))
    sub.add_parser("status", help="print the daemon's state as JSON")
    sub.add_parser("last", help="print the last cleaned transcript")
    sub.add_parser("waybar", help="print the waybar module JSON (use --follow as the module's exec)").add_argument(
        "--follow", action="store_true"
    )
    d = sub.add_parser("doctor", help="read-only checks of tools, mic, key, binds and the daemon")
    d.add_argument("--json", action="store_true")
    sub.add_parser("binds", help="print the Hyprland bind lines for the configured keys")
    v = sub.add_parser("vocab", help="manage the local vocabulary file")
    vs = v.add_subparsers(dest="vocab_cmd", required=True)
    vs.add_parser("list")
    va = vs.add_parser("add")
    va.add_argument("terms", nargs="+")
    vr = vs.add_parser("remove")
    vr.add_argument("term")
    meeting_cli.add_parser(sub)
    return p


async def _run(cfg: DictateConfig) -> int:
    daemon = Daemon(cfg, build_pipeline(cfg), make_sink(cfg), notifier=notify)
    stop = asyncio.Event()
    loop = asyncio.get_running_loop()
    for sig in (signal.SIGINT, signal.SIGTERM):  # before serve(): an early signal must still stop us cleanly
        loop.add_signal_handler(sig, stop.set)
    await daemon.serve()
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
        if args.cmd == "waybar":
            if args.follow:
                follow(cfg.state_path, cfg.socket_path)
            else:
                print(json.dumps(snapshot(cfg.state_path, cfg.socket_path), ensure_ascii=False))
            return 0
        if args.cmd == "doctor":
            checks = run_checks(cfg)
            print(to_json(checks) if args.json else render(checks))
            return exit_code(checks)
        if args.cmd == "binds":
            print("\n".join(hyprland_bind_lines(cfg.hold_bind, cfg.toggle_bind)))
            return 0
        if args.cmd == "meeting":
            return meeting_cli.handle(args)
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
