"""`wordink-dictd`: run the daemon, drive it from a Hyprland bind, and manage the local vocabulary."""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import signal
import sys
from pathlib import Path

from ..client import WordInkError
from .binds import hyprland_bind_lines, hyprland_lua_lines
from .config import DictateConfig, load_config
from .daemon import Daemon, build_pipeline, make_sink, send_command
from . import costs, meeting_cli
from .doctor import exit_code, render, run_checks, to_json
from .platform import OSName, current_os
from .notify import notify
from .vocab import add_terms, read_terms, remove_term
from .waybar import follow, snapshot


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="wordink-dictd", description="WordInk push-to-talk dictation daemon")
    p.add_argument("--config", type=Path, help="path to dictate.toml (default ~/.config/wordink/dictate.toml)")
    p.add_argument("--debug", action="store_true", help="verbose logs, including transcript text (also WORDINK_DEBUG=1)")
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("run", help="run the daemon in the foreground (the systemd unit does this)")
    t = sub.add_parser("trigger", help="tell the daemon to start, stop or toggle recording (what the Hyprland bind runs)")
    t.add_argument("action", choices=("start", "stop", "toggle"))
    c = sub.add_parser("costs", help="usage and estimated Groq cost from the local log, per day and per session")
    c.add_argument("--days", type=int, default=7, help="how many days back (default 7)")
    c.add_argument("--session", help="only one session: a meeting id, or 'dictation'")
    c.add_argument("--json", action="store_true")
    sub.add_parser("set-key", help="store the Groq API key in Windows Credential Manager (Windows only)")
    sub.add_parser("status", help="print the daemon's state as JSON")
    sub.add_parser("last", help="print the last cleaned transcript")
    sub.add_parser("waybar", help="print the waybar module JSON (use --follow as the module's exec)").add_argument(
        "--follow", action="store_true"
    )
    d = sub.add_parser("doctor", help="read-only checks of tools, mic, key, binds and the daemon")
    d.add_argument("--json", action="store_true")
    b = sub.add_parser("binds", help="print the Hyprland bind lines for the configured keys")
    b.add_argument("--format", choices=("conf", "lua"), default="conf", help="hyprland.conf lines (default) or Hyprland Lua config lines")
    b.add_argument("--exe", default="wordink-dictd", help="command the binds run (use an absolute path for Lua configs)")
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


async def _request(cfg: DictateConfig, os_name: OSName, command: str) -> dict:
    """One command to the running daemon: the Unix socket on Linux, the loopback port on Windows."""
    if os_name == "windows":
        from .win.transport import LoopbackTransport

        return await LoopbackTransport(cfg.runtime_dir).request(command)
    return await send_command(cfg.socket_path, command)


def _windows_doctor(cfg: DictateConfig, as_json: bool) -> int:
    from .win import doctor as wd

    checks = wd.run_checks(cfg, os.environ, wd.real_probes(cfg))
    print(to_json(checks) if as_json else render(checks))
    return exit_code(checks)


def _mac_doctor(cfg: DictateConfig, as_json: bool) -> int:
    from .mac import doctor as md

    checks = md.run_checks(cfg, os.environ, md.real_probes(cfg))
    print(to_json(checks) if as_json else render(checks))
    return exit_code(checks)


def main(argv: list[str] | None = None, *, os_name: OSName | None = None) -> int:
    args = build_parser().parse_args(argv)
    debug = args.debug or costs.debug_enabled()
    logging.basicConfig(level=logging.DEBUG if debug else logging.INFO, format="%(levelname)s %(name)s: %(message)s", stream=sys.stderr)
    logging.getLogger().setLevel(logging.DEBUG if debug else logging.INFO)  # basicConfig is a no-op if a handler exists
    for noisy in ("httpx", "httpcore", "hpack"):  # their debug lines can carry request headers
        logging.getLogger(noisy).setLevel(logging.WARNING)
    try:
        os_name = os_name or current_os()
        cfg = load_config(args.config, os_name=os_name)
        if args.cmd == "set-key":
            if os_name == "windows":
                from .win.secrets import set_key_interactive

                where = "Windows Credential Manager"
            elif os_name == "macos":
                from .mac.secrets import set_key_interactive

                where = "the macOS Keychain"
            else:
                raise WordInkError("set-key is for Windows and macOS. On Linux put GROQ_API_KEY in the EnvironmentFile (see the README)")
            set_key_interactive()
            print(f"wordink-dictd: key stored in {where}")
            return 0
        if args.cmd == "run":
            if os_name == "windows":
                from .win.app import run_configured

                return run_configured(cfg, debug)
            if os_name == "macos":
                from .mac.app import run_configured as run_mac

                return run_mac(cfg, debug)
            return asyncio.run(_run(cfg))
        if args.cmd in ("trigger", "status"):
            reply = asyncio.run(_request(cfg, os_name, args.action if args.cmd == "trigger" else "status"))
            print(json.dumps(reply))
            return 0 if reply.get("ok") else 1
        if args.cmd == "last":
            try:
                sys.stdout.write(cfg.last_path.read_text(encoding="utf-8"))
            except FileNotFoundError:
                print("wordink-dictd: no dictation yet", file=sys.stderr)
                return 1
            return 0
        if args.cmd in ("waybar",) and os_name == "windows":
            raise WordInkError("waybar is Linux only; the Windows app shows its state in the tray icon")
        if args.cmd == "waybar" and os_name == "macos":
            raise WordInkError("waybar is Linux only; the macOS app shows its state in the menu bar")
        if args.cmd == "doctor" and os_name == "macos":
            return _mac_doctor(cfg, args.json)
        if args.cmd == "waybar":
            if args.follow:
                follow(cfg.state_path, cfg.socket_path)
            else:
                print(json.dumps(snapshot(cfg.state_path, cfg.socket_path), ensure_ascii=False))
            return 0
        if args.cmd == "doctor" and os_name == "windows":
            return _windows_doctor(cfg, args.json)
        if args.cmd == "doctor":
            checks = run_checks(cfg)
            print(to_json(checks) if args.json else render(checks))
            return exit_code(checks)
        if args.cmd == "binds" and os_name in ("windows", "macos"):
            print(f"hold:   {cfg.hold_bind}\ntoggle: {cfg.toggle_bind}\n(change them under [hotkey] in dictate.toml)")
            return 0
        if args.cmd == "binds":
            make = hyprland_lua_lines if args.format == "lua" else hyprland_bind_lines
            print("\n".join(make(cfg.hold_bind, cfg.toggle_bind, args.exe)))
            return 0
        if args.cmd == "costs":
            summary = costs.summarize(costs.read_entries(), days=args.days, session=args.session)
            print(json.dumps(summary, indent=2) if args.json else costs.render(summary))
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
