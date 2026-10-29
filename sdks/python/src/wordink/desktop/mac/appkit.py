"""The only module that imports AppKit: pasteboard and the frontmost app (plan M4).

Imports are lazy, so this module loads on every OS and the functions fail only when called off a
Mac. None of this has run on a Mac yet (plan M9). The menu bar (M5) adds its status item here.
"""

from __future__ import annotations

#: pasteboard types that are just our text in another spelling; anything else counts as "other data"
_TEXT_TYPES = frozenset(
    {
        "public.utf8-plain-text",
        "public.utf16-external-plain-text",
        "public.utf16-plain-text",
        "NSStringPboardType",
        "com.apple.traditional-mac-plain-text",
    }
)
_MARKER_PREFIX = "org.nspasteboard."
CONCEALED, TRANSIENT = "org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"


def _appkit():
    import AppKit  # noqa: PLC0415

    return AppKit


def frontmost_bundle_id() -> str | None:
    app = _appkit().NSWorkspace.sharedWorkspace().frontmostApplication()
    return str(app.bundleIdentifier()) if app is not None and app.bundleIdentifier() else None


def has_other_formats(types: list[str]) -> bool:
    """True when the pasteboard holds more than plain text (images, files, rich text from a document)."""
    return any(t not in _TEXT_TYPES and not t.startswith(_MARKER_PREFIX) for t in types)


def pasteboard_snapshot() -> tuple[str | None, bool]:
    ak = _appkit()
    pb = ak.NSPasteboard.generalPasteboard()
    types = [str(t) for t in (pb.types() or [])]
    text = pb.stringForType_(ak.NSPasteboardTypeString)
    return (str(text) if text is not None else None), has_other_formats(types)


def pasteboard_set(text: str) -> bool:
    """Text only, flagged concealed and transient so clipboard managers skip it.

    False when AppKit refused any of the writes (the caller treats that as a busy pasteboard).
    """
    ak = _appkit()
    pb = ak.NSPasteboard.generalPasteboard()
    pb.declareTypes_owner_([ak.NSPasteboardTypeString, CONCEALED, TRANSIENT], None)
    return all(
        (
            pb.setString_forType_(text, ak.NSPasteboardTypeString),
            pb.setString_forType_("", CONCEALED),
            pb.setString_forType_("", TRANSIENT),
        )
    )


def pasteboard_clear() -> None:
    _appkit().NSPasteboard.generalPasteboard().clearContents()
