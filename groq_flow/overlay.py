from enum import Enum
import queue
import threading
import tkinter as tk
from typing import Optional


class RecorderState(Enum):
    """Recording states"""

    IDLE = "idle"
    RECORDING = "recording"
    PROCESSING = "processing"
    ERROR = "error"


class StatusOverlay:
    _TRANSPARENT_COLOR = "#010203"

    def __init__(self) -> None:
        self.state = RecorderState.IDLE
        self.is_shown = False

        self._cmd_queue: "queue.Queue[tuple[str, Optional[RecorderState]]]" = queue.Queue(
        )
        self._thread = threading.Thread(target=self._run_overlay, daemon=True)
        self._thread.start()

        self._closing = False

    # ------------------------------------------------------------------
    # Public API (thread-safe)
    # ------------------------------------------------------------------
    def show(self, state: RecorderState) -> None:
        """Show overlay for the given state."""

        if state == RecorderState.IDLE:
            self.hide()
            return

        self.state = state
        self._cmd_queue.put(("show", state))

    def hide(self) -> None:
        """Hide overlay."""

        self.state = RecorderState.IDLE
        self._cmd_queue.put(("hide", None))

    def update_state(self, state: RecorderState) -> None:
        """Update overlay state (alias for show/hide)."""

        if state in (RecorderState.RECORDING, RecorderState.PROCESSING, RecorderState.ERROR):
            self.show(state)
        else:
            self.hide()

    def cleanup(self) -> None:
        """Shutdown overlay thread."""

        self._closing = True
        self._cmd_queue.put(("close", None))

    # ------------------------------------------------------------------
    # Overlay thread & UI
    # ------------------------------------------------------------------
    def _run_overlay(self) -> None:
        import sys
        if sys.platform == "win32":
            try:
                import ctypes
                user32 = ctypes.windll.user32
                hdesk = user32.OpenDesktopW("Default", 0, False, 0x01FF)
                if hdesk:
                    user32.SetThreadDesktop(hdesk)
            except Exception:
                pass

        self._root = tk.Tk()
        self._root.withdraw()
        self._root.overrideredirect(True)
        self._root.attributes("-topmost", True)
        try:
            self._root.attributes("-transparentcolor", self._TRANSPARENT_COLOR)
        except tk.TclError:
            # Older Tk versions on Windows may not support transparentcolor
            pass
        self._root.configure(bg=self._TRANSPARENT_COLOR)

        self._width = 260
        self._height = 60

        self._canvas = tk.Canvas(
            self._root,
            width=self._width,
            height=self._height,
            bg=self._TRANSPARENT_COLOR,
            highlightthickness=0,
            bd=0,
        )
        self._canvas.pack(fill=tk.BOTH, expand=True)

        # Pre-create canvas items for reuse
        self._pill_items = []
        self._dot_item = None
        self._spinner_item = None
        self._text_item = None
        self._spinner_job: Optional[str] = None
        self._spinner_frames = ["◐", "◓", "◑", "◒"]
        self._spinner_index = 0

        self._build_canvas()

        self._process_queue()
        self._root.mainloop()

    # Canvas helpers ----------------------------------------------------
    def _build_canvas(self) -> None:
        bg_color = "#1f1f28"
        accent_color = "#22d3ee"

        radius = 28
        w, h = self._width, self._height

        # Draw pill background (rounded rectangle)
        segments = [
            (radius, 0, w - radius, h),
            (0, radius, w, h - radius),
        ]
        arcs = [
            (0, 0, radius * 2, radius * 2),
            (w - radius * 2, 0, w, radius * 2),
            (0, h - radius * 2, radius * 2, h),
            (w - radius * 2, h - radius * 2, w, h),
        ]

        for coords in segments:
            self._pill_items.append(
                self._canvas.create_rectangle(
                    *coords, fill=bg_color, outline=bg_color)
            )
        for coords in arcs:
            self._pill_items.append(
                self._canvas.create_oval(
                    *coords, fill=bg_color, outline=bg_color)
            )

        # Indicator dot
        dot_radius = 10
        dot_center_x = 32
        dot_center_y = h / 2
        self._dot_item = self._canvas.create_oval(
            dot_center_x - dot_radius,
            dot_center_y - dot_radius,
            dot_center_x + dot_radius,
            dot_center_y + dot_radius,
            fill=accent_color,
            outline="",
        )

        # Spinner text (hidden by default)
        self._spinner_item = self._canvas.create_text(
            dot_center_x,
            dot_center_y,
            text="",
            fill=accent_color,
            font=("Segoe UI", 16, "bold"),
        )

        # Status text
        self._text_item = self._canvas.create_text(
            60,
            dot_center_y,
            anchor="w",
            text="Listening…",
            fill="#f7f7f7",
            font=("Segoe UI", 12, "bold"),
        )

    def _update_background(self, color: str) -> None:
        for item in self._pill_items:
            self._canvas.itemconfigure(item, fill=color, outline=color)

    def _set_indicator(self, color: str) -> None:
        if self._dot_item is not None:
            self._canvas.itemconfigure(self._dot_item, fill=color)

    # Queue handling ----------------------------------------------------
    def _process_queue(self) -> None:
        try:
            while True:
                command, value = self._cmd_queue.get_nowait()

                if command == "show" and isinstance(value, RecorderState):
                    self._show_impl(value)
                elif command == "hide":
                    self._hide_impl()
                elif command == "close":
                    self._hide_impl()
                    self._closing = True
                    self._root.after(10, self._root.quit)
                    return
        except queue.Empty:
            pass

        if not self._closing:
            self._root.after(50, self._process_queue)

    # State handlers ----------------------------------------------------
    def _show_impl(self, state: RecorderState) -> None:
        config = self._state_config(state)

        self._update_background(config["bg"])
        if self._text_item is not None:
            self._canvas.itemconfigure(
                self._text_item, text=config["text"], fill=config["fg"])

        if config.get("use_spinner"):
            self._start_spinner(config["accent"])
        else:
            self._stop_spinner()
            self._set_indicator(config["accent"])
            if self._dot_item is not None:
                self._canvas.itemconfigure(self._dot_item, state="normal")

        self._update_indicator_visibility(config.get("use_spinner", False))

        self._position_window()
        self._root.deiconify()
        self._root.lift()
        self._root.update_idletasks()
        self.is_shown = True

    def _hide_impl(self) -> None:
        self._stop_spinner()
        self._root.withdraw()
        self.is_shown = False

    # Spinner -----------------------------------------------------------
    def _start_spinner(self, color: str) -> None:
        if self._spinner_item is None:
            return

        self._canvas.itemconfigure(self._spinner_item, fill=color)
        self._canvas.itemconfigure(self._spinner_item, state="normal")
        if self._dot_item is not None:
            self._canvas.itemconfigure(self._dot_item, state="hidden")

        if self._spinner_job is None:
            self._spinner_index = 0
            self._animate_spinner()

    def _animate_spinner(self) -> None:
        if self._spinner_item is None:
            return

        frame = self._spinner_frames[self._spinner_index % len(
            self._spinner_frames)]
        self._spinner_index += 1
        self._canvas.itemconfigure(self._spinner_item, text=frame)
        self._spinner_job = self._root.after(120, self._animate_spinner)

    def _stop_spinner(self) -> None:
        if self._spinner_job is not None:
            self._root.after_cancel(self._spinner_job)
            self._spinner_job = None

        if self._spinner_item is not None:
            self._canvas.itemconfigure(
                self._spinner_item, text="", state="hidden")

    def _update_indicator_visibility(self, spinner_active: bool) -> None:
        if self._dot_item is not None:
            state = "hidden" if spinner_active else "normal"
            self._canvas.itemconfigure(self._dot_item, state=state)

    # Utility -----------------------------------------------------------
    def _position_window(self) -> None:
        self._root.update_idletasks()
        screen_w = self._root.winfo_screenwidth()
        screen_h = self._root.winfo_screenheight()

        x = int(screen_w - self._width - 30)
        y = int(screen_h - self._height - 90)

        self._root.geometry(f"{self._width}x{self._height}+{x}+{y}")

    def _state_config(self, state: RecorderState) -> dict:
        base_bg = {RecorderState.ERROR: "#451a1a"}.get(state, "#1f1f28")
        base_fg = "#f5f5f5"

        if state == RecorderState.RECORDING:
            return {
                "bg": base_bg,
                "fg": base_fg,
                "accent": "#F97316",
                "text": "Listening…",
                "use_spinner": False,
            }
        if state == RecorderState.PROCESSING:
            return {
                "bg": base_bg,
                "fg": base_fg,
                "accent": "#38BDF8",
                "text": "Processing…",
                "use_spinner": True,
            }
        if state == RecorderState.ERROR:
            return {
                "bg": base_bg,
                "fg": base_fg,
                "accent": "#F87171",
                "text": "Something went wrong",
                "use_spinner": False,
            }

        return {
            "bg": "#1f1f28",
            "fg": base_fg,
            "accent": "#22d3ee",
            "text": "",
            "use_spinner": False,
        }


# Singleton instance ----------------------------------------------------
_overlay_instance: Optional[StatusOverlay] = None


def get_overlay() -> StatusOverlay:
    global _overlay_instance
    if _overlay_instance is None:
        _overlay_instance = StatusOverlay()
    return _overlay_instance
