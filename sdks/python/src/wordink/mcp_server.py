"""`wordink-mcp`: MCP stdio server exposing gateway transcription as tools."""

from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path

from mcp.server.mcpserver import MCPServer

from .cli import MIMETYPES
from .client import (
    WordInkError,
    list_models,
    transcribe,
    vocab_add,
    vocab_list,
    vocab_remove,
)

server = MCPServer("wordink")


@server.tool(
    name="transcribe",
    description=(
        "Transcribe a local audio file via the wordink-gateway "
        "(OpenAI-compatible; provider fallback and shared vocabulary apply "
        "server-side). Returns the transcript text. "
        "For a ONE-OFF correction — a name the ASR misspelled this once — pass "
        "prompt='…' with the right spelling; that stays local to the call. "
        "Use vocab_add only for terms that recur across sessions."
    )
)
async def transcribe_tool(
    file_path: str, language: str | None = None, prompt: str | None = None
) -> str:
    path = Path(file_path).expanduser().resolve()
    if not path.is_file():
        return f"wordink: not a file: {path}"
    try:
        return await asyncio.to_thread(
            transcribe,
            path,
            filename=path.name,
            content_type=MIMETYPES.get(
                path.suffix.lower(), "application/octet-stream"
            ),
            language=language,
            prompt=prompt,
        )
    except WordInkError as exc:
        return f"wordink: {exc}"


@server.tool(name="list_models", description="List the transcription models the gateway serves.")
async def list_models_tool() -> str:
    try:
        models = await asyncio.to_thread(list_models)
        return "\n".join(models) or "(none)"
    except WordInkError as exc:
        return f"wordink: {exc}"


@server.tool(
    name="vocab_list",
    description=(
        "List the gateway's effective vocabulary as JSON: config-pinned terms plus "
        "runtime additions, each with source ('config'|'runtime'), pinned, optional "
        "runtimeOverride, effective {prompt, keyterm} (which merge budgets the term "
        "actually reaches — a stored term reporting both false is inert), and runtime "
        "provenance (addedBy, addedAt). Admin token required."
    )
)
async def vocab_list_tool() -> str:
    try:
        return json.dumps(await asyncio.to_thread(vocab_list))
    except WordInkError as exc:
        return f"wordink: {exc}"


@server.tool(
    name="vocab_add",
    description=(
        "Add RECURRING terms (repo names, project jargon, names the ASR keeps "
        "misspelling) to the gateway's shared runtime vocabulary — global, shared "
        "across every device and session. For a one-off correction prefer "
        "transcribe(file, prompt='…'): local to that call and reversible. "
        "Per-term outcomes: added / already_present / updated (respelled a runtime "
        "term) / pinned (already a config term — nothing stored) / config_shadowed "
        "(runtime spelling override on a config term — the new casing wins but "
        "presence stays operator-owned). NEVER add secrets, passwords or sensitive "
        "dictated content: every term is sent verbatim to transcription providers "
        "on every call. Returns JSON."
    )
)
async def vocab_add_tool(terms: list[str], dry_run: bool = False) -> str:
    try:
        return json.dumps(await asyncio.to_thread(vocab_add, terms, dry_run=dry_run))
    except WordInkError as exc:
        return f"wordink: {exc}"


@server.tool(
    name="vocab_remove",
    description=(
        "Remove a runtime vocabulary term (case-folded match). A term_pinned error "
        "means the term lives in the operator's gateway.json — only the operator can "
        "remove it there. Removing a runtime spelling override on a config term "
        "reverts to the config spelling (reported as reverted_to). Returns JSON."
    )
)
async def vocab_remove_tool(term: str) -> str:
    try:
        return json.dumps(await asyncio.to_thread(vocab_remove, term))
    except WordInkError as exc:
        return f"wordink: {exc}"


def main() -> None:
    # Either scope works: device tokens transcribe; the vocab tools additionally
    # need admin (a device token earns a clean 403 from the gateway).
    if not (os.environ.get("WORDINK_DEVICE_TOKEN") or os.environ.get("WORDINK_ADMIN_TOKEN")):
        raise SystemExit(
            "wordink-mcp: set WORDINK_DEVICE_TOKEN (or WORDINK_ADMIN_TOKEN for the "
            "vocabulary tools) and optionally WORDINK_GATEWAY_URL in the host's env"
        )
    server.run("stdio")


if __name__ == "__main__":
    main()
