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
MAX_SNAPSHOT_BYTES = 16 * 1024 * 1024  # a bigger pasteboard (a screenshot burst, a video) is never copied around
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


def pasteboard_items(max_bytes: int = MAX_SNAPSHOT_BYTES) -> tuple[tuple[tuple[str, bytes], ...], ...] | None:
    """Every format of every item, so a paste can put rich content (HTML, images, files) back exactly.

    None when a format has no data right now (a lazy promise) or the total passes `max_bytes`. The
    whole snapshot is dropped rather than a partial one: a restore that silently omits a format
    would be a lossy copy of what the user had, and the caller leaves such a pasteboard untouched.
    """
    pb = _appkit().NSPasteboard.generalPasteboard()
    out: list[tuple[tuple[str, bytes], ...]] = []
    total = 0
    for item in pb.pasteboardItems() or []:
        entry: list[tuple[str, bytes]] = []
        for kind in item.types():
            data = item.dataForType_(kind)
            if data is None:
                return None
            total += int(data.length())
            if total > max_bytes:
                return None
            entry.append((str(kind), bytes(data)))
        out.append(tuple(entry))
    return tuple(out)


def pasteboard_snapshot() -> tuple[str | None, bool, tuple[tuple[tuple[str, bytes], ...], ...] | None]:
    ak = _appkit()
    pb = ak.NSPasteboard.generalPasteboard()
    types = [str(t) for t in (pb.types() or [])]
    text = pb.stringForType_(ak.NSPasteboardTypeString)
    other = has_other_formats(types)
    return (str(text) if text is not None else None), other, (pasteboard_items() if other else None)


def pasteboard_restore(items: tuple[tuple[tuple[str, bytes], ...], ...]) -> bool:
    """Replace the pasteboard with `items`. False when AppKit refused the write."""
    ak = _appkit()
    pb = ak.NSPasteboard.generalPasteboard()
    pb.clearContents()
    restored = []
    for entry in items:
        item = ak.NSPasteboardItem.alloc().init()
        for kind, payload in entry:
            if not item.setData_forType_(ak.NSData.dataWithBytes_length_(payload, len(payload)), kind):
                return False
        restored.append(item)
    return bool(pb.writeObjects_(restored))


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


def pasteboard_copy(text: str) -> None:
    """A normal copy (clipboard managers may keep it), for the menu's "Copy last transcript"."""
    ak = _appkit()
    pb = ak.NSPasteboard.generalPasteboard()
    pb.clearContents()
    pb.setString_forType_(text, ak.NSPasteboardTypeString)


def microphone_access() -> str | None:
    """granted, denied or undetermined; None when AVFoundation is not installed or the state is unknown."""
    try:
        import AVFoundation as av  # noqa: PLC0415
    except ImportError:
        return None
    status = int(av.AVCaptureDevice.authorizationStatusForMediaType_(av.AVMediaTypeAudio))
    return {0: "undetermined", 2: "denied", 3: "granted"}.get(status)  # 1 is restricted by policy: unknown


def pasteboard_clear() -> None:
    _appkit().NSPasteboard.generalPasteboard().clearContents()


def _ensure_edit_menu(ak) -> None:
    """A menu-bar-only app has no Edit menu, so Cmd+V does nothing in a text field. Add one."""
    app = ak.NSApplication.sharedApplication()
    if app.mainMenu() is not None:
        return
    main, top = ak.NSMenu.alloc().init(), ak.NSMenuItem.alloc().init()
    main.addItem_(top)
    edit = ak.NSMenu.alloc().initWithTitle_("Edit")
    entries = (("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a"))
    for title, action, key in entries:
        edit.addItemWithTitle_action_keyEquivalent_(title, action, key)
    top.setSubmenu_(edit)
    app.setMainMenu_(main)


def prompt_secret(title: str, message: str, ok: str = "Save", cancel: str = "Cancel") -> str | None:
    """Ask for a secret in a dialog whose field shows dots. None when cancelled. Main thread only.

    `rumps.Window` has no secure mode, so this is an NSAlert with an NSSecureTextField.
    """
    from Foundation import NSThread  # noqa: PLC0415

    if not NSThread.isMainThread():
        raise RuntimeError("the secret prompt must run on the main thread")
    ak = _appkit()
    _ensure_edit_menu(ak)
    ak.NSApp.activateIgnoringOtherApps_(True)
    alert = ak.NSAlert.alloc().init()
    alert.setMessageText_(title)
    alert.setInformativeText_(message)
    alert.addButtonWithTitle_(ok)
    alert.addButtonWithTitle_(cancel)
    field = ak.NSSecureTextField.alloc().initWithFrame_(((0, 0), (320, 24)))
    alert.setAccessoryView_(field)
    alert.window().setInitialFirstResponder_(field)
    if alert.runModal() != ak.NSAlertFirstButtonReturn:
        return None
    value = str(field.stringValue())
    field.setStringValue_("")
    return value
