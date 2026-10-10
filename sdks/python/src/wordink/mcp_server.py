"""`wordink-mcp`: MCP stdio server exposing gateway transcription as tools."""

from __future__ import annotations

import asyncio
import os
from pathlib import Path

from mcp.server.mcpserver import MCPServer

from .cli import MIMETYPES
from .client import WordInkError, list_models, transcribe

server = MCPServer("wordink")


@server.tool(
    name="transcribe",
    description=(
        "Transcribe a local audio file via the wordink-gateway "
        "(OpenAI-compatible; provider fallback and shared vocabulary apply "
        "server-side). Returns the transcript text."
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


def main() -> None:
    if not os.environ.get("WORDINK_DEVICE_TOKEN"):
        raise SystemExit(
            "wordink-mcp: set WORDINK_DEVICE_TOKEN "
            "(and optionally WORDINK_GATEWAY_URL) in the host's env"
        )
    server.run("stdio")


if __name__ == "__main__":
    main()
