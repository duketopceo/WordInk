import sys
import threading
from typing import Optional, Any
from PIL import Image, ImageDraw
import pystray
from pystray import MenuItem as item

from .app import GroqFlowApp
from .config import config


class SystemTrayApp:
    """System tray application wrapper"""

    def __init__(self):
        """Initialize system tray app"""
        self.app: Optional[GroqFlowApp] = None
        # pystray.Icon - using Any to avoid import errors before installation
        self.icon: Any = None
        self.app_thread: Optional[threading.Thread] = None

    def create_icon_image(self, color="green"):
        """
        Create a simple icon image programmatically

        Args:
            color: Icon color (green, red, orange)

        Returns:
            PIL Image
        """
        # Create a 64x64 image with a colored circle
        size = 64
        image = Image.new('RGB', (size, size), color='black')
        dc = ImageDraw.Draw(image)

        # Color mapping
        colors = {
            'green': '#4CAF50',
            'red': '#F44336',
            'orange': '#FF9800',
            'blue': '#2196F3'
        }

        fill_color = colors.get(color, colors['green'])

        # Draw a circle (microphone representation)
        margin = 8
        dc.ellipse([margin, margin, size-margin, size-margin], fill=fill_color)

        # Draw inner circle (microphone center)
        inner_margin = 20
        dc.ellipse([inner_margin, inner_margin, size-inner_margin, size-inner_margin],
                   fill='white')

        return image

    def start(self):
        """Start the system tray application"""
        # Create icon image
        icon_image = self.create_icon_image('green')

        # Create menu
        menu = pystray.Menu(
            item('Groq Flow', self._show_info, default=True),
            item('Status: Ready', self._noop, enabled=False),
            pystray.Menu.SEPARATOR,
            item('Settings', pystray.Menu(
                item(f'Hotkey: {config.hotkey}', self._noop, enabled=False),
                item(f'AI Cleanup: {"On" if config.enable_ai_cleanup else "Off"}',
                     self._toggle_ai_cleanup,
                     checked=lambda _: config.enable_ai_cleanup),
                item('Debug Mode', self._toggle_debug,
                     checked=lambda _: config.debug),
            )),
            pystray.Menu.SEPARATOR,
            item('Exit', self._exit_app)
        )

        # Create system tray icon
        self.icon = pystray.Icon(
            name="Groq Flow",
            icon=icon_image,
            title="Groq Flow - AI Speech-to-Text",
            menu=menu
        )

        # Start main app in background thread
        self.app = GroqFlowApp()
        self.app_thread = threading.Thread(target=self.app.start, daemon=True)
        self.app_thread.start()

        # Run system tray (blocks until exit)
        print("🖥️  System tray app started")
        print(f"🎤 Press '{config.hotkey}' anywhere to start recording")
        print("📌 Check system tray for options")
        print()

        try:
            if self.icon:
                self.icon.run()
        except KeyboardInterrupt:
            self._exit_app()

    def _show_info(self):
        """Show application info"""
        print("\n" + "="*50)
        print("🎤 Groq Flow - AI Speech-to-Text")
        print("="*50)
        print(f"Hotkey: {config.hotkey}")
        print(
            f"AI Cleanup: {'Enabled' if config.enable_ai_cleanup else 'Disabled'}")
        print(f"Whisper Model: {config.whisper_model}")
        print(f"LLM Model: {config.llm_model}")
        print(f"Debug Mode: {'On' if config.debug else 'Off'}")
        print("="*50 + "\n")

    def _toggle_ai_cleanup(self):
        """Toggle AI cleanup feature"""
        new_value = not config.enable_ai_cleanup
        config.set('ENABLE_AI_CLEANUP', new_value)
        status = "enabled" if new_value else "disabled"
        print(f"✨ AI cleanup {status}")

        # Update menu (recreate icon with new menu)
        self._update_menu()

    def _toggle_debug(self):
        """Toggle debug mode"""
        new_value = not config.debug
        config.set('DEBUG', new_value)
        status = "enabled" if new_value else "disabled"
        print(f"🐛 Debug mode {status}")

        # Update menu
        self._update_menu()

    def _update_menu(self):
        """Update system tray menu"""
        # Note: pystray doesn't support dynamic menu updates easily
        # User needs to re-open menu to see changes
        pass

    def _noop(self):
        """No operation (for disabled menu items)"""
        pass

    def _exit_app(self):
        """Exit the application"""
        print("\n⚠️  Shutting down Groq Flow...")

        # Stop main app
        if self.app:
            self.app.stop()

        # Stop system tray
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
