import sys
import threading
import webbrowser
from typing import Optional, Any
from PIL import Image, ImageDraw
import pystray
from pystray import MenuItem as item

from .app import GroqFlowApp
from .config import config


class SystemTrayApp:
    """System tray application wrapper for WordInk"""

    def __init__(self):
        """Initialize system tray app"""
        self.app: Optional[GroqFlowApp] = None
        self.icon: Any = None
        self.app_thread: Optional[threading.Thread] = None

    def create_icon_image(self, color="green"):
        """Create a clean modern icon with transparent background"""
        size = 64
        image = Image.new('RGBA', (size, size), (0, 0, 0, 0))
        dc = ImageDraw.Draw(image)

        colors = {
            'green': '#10B981',
            'red': '#EF4444',
            'orange': '#F59E0B',
            'blue': '#6366F1'
        }

        fill_color = colors.get(color, colors['green'])

        # Draw modern microphone capsule
        mic_width = 20
        mic_height = 28
        mic_x = (size - mic_width) // 2
        mic_y = 12

        dc.rounded_rectangle(
            [mic_x, mic_y, mic_x + mic_width, mic_y + mic_height],
            radius=10,
            fill=fill_color
        )

        # Draw mic stand (vertical line)
        stand_width = 3
        stand_x = size // 2 - stand_width // 2
        dc.rectangle(
            [stand_x, mic_y + mic_height, stand_x + stand_width, size - 12],
            fill=fill_color
        )

        # Draw mic base (horizontal line)
        base_width = 16
        base_height = 3
        base_x = (size - base_width) // 2
        dc.rectangle(
            [base_x, size - 12, base_x + base_width, size - 12 + base_height],
            fill=fill_color
        )

        return image

    def start(self):
        """Start the system tray application"""
        icon_image = self.create_icon_image('green')

        menu = pystray.Menu(
            item('WordInk 🖋️', self._show_info, default=True),
            item(f'Hotkey: {config.hotkey}', self._noop, enabled=False),
            item('Open Setup & Telemetry Wizard', self._open_wizard),
            pystray.Menu.SEPARATOR,
            item('Settings', pystray.Menu(
                item(f'Model: {config.whisper_model}', self._noop, enabled=False),
                item(f'AI Cleanup: {"On" if config.enable_ai_cleanup else "Off"}',
                     self._toggle_ai_cleanup,
                     checked=lambda _: config.enable_ai_cleanup),
                item('Debug Mode', self._toggle_debug,
                     checked=lambda _: config.debug),
            )),
            pystray.Menu.SEPARATOR,
            item('Exit WordInk', self._exit_app)
        )

        self.icon = pystray.Icon(
            name="WordInk",
            icon=icon_image,
            title=f"WordInk — Press {config.hotkey} to dictate",
            menu=menu
        )

        # Start main app in background thread
        self.app = GroqFlowApp()
        self.app_thread = threading.Thread(target=self.app.start, daemon=True)
        self.app_thread.start()

        print("🖥️  WordInk system tray app started")
        print(f"🎤 Press '{config.hotkey}' anywhere to start recording")
        print("📌 Check system tray for options")
        print()

        try:
            if self.icon:
                self.icon.run()
        except KeyboardInterrupt:
            self._exit_app()

    def _open_wizard(self):
        """Open web-based onboarding wizard and telemetry dashboard"""
        from .onboarding.server import start_onboarding_server
        threading.Thread(target=start_onboarding_server, kwargs={"open_browser": True}, daemon=True).start()

    def _show_info(self):
        """Show application info"""
        print("\n" + "=" * 50)
        print("🖋️  WordInk - Sub-Second Voice Dictation")
        print("=" * 50)
        print(f"Hotkey: {config.hotkey}")
        print(f"AI Cleanup: {'Enabled' if config.enable_ai_cleanup else 'Disabled'}")
        print(f"Whisper Model: {config.whisper_model}")
        print(f"Debug Mode: {'On' if config.debug else 'Off'}")
        print("=" * 50 + "\n")

    def _toggle_ai_cleanup(self):
        new_value = not config.enable_ai_cleanup
        config.set('ENABLE_AI_CLEANUP', new_value)

    def _toggle_debug(self):
        new_value = not config.debug
        config.set('DEBUG', new_value)

    def _noop(self):
        pass

    def _exit_app(self):
        print("\n⚠️  Shutting down WordInk...")
        if self.app:
            self.app.stop()
        if self.icon:
            self.icon.stop()
        sys.exit(0)


def run_system_tray():
    """Run the system tray application"""
    try:
        tray_app = SystemTrayApp()
        tray_app.start()
    except KeyboardInterrupt:
        print("\n👋 Goodbye!")
        sys.exit(0)
    except Exception as e:
        print(f"❌ Fatal error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
