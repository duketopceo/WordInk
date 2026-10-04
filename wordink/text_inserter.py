import time
import pyperclip
import pyautogui
from typing import Optional


class TextInserter:
    """Handles inserting text at cursor position across all applications"""

    @staticmethod
    def insert_text(text: str, delay: float = 0.1) -> bool:
        """
        Insert text at current cursor position using clipboard
        This works universally across all Windows applications

        Args:
            text: Text to insert
            delay: Delay before pasting (seconds)

        Returns:
            True if successful, False otherwise
        """
        if not text:
            return False

        try:
            # Release any lingering modifier keys (like Alt from hotkey)
            try:
                import keyboard
                for mod in ('alt', 'ctrl', 'shift'):
                    try:
                        keyboard.release(mod)
                    except Exception:
                        pass
            except Exception:
                pass

            # Copy text to clipboard
            pyperclip.copy(text)

            # Check if active window is a terminal (Ghostty/noctty, Windows Terminal, console)
            is_terminal = False
            try:
                import ctypes
                user32 = ctypes.windll.user32
                hwnd = user32.GetForegroundWindow()
                class_buf = ctypes.create_unicode_buffer(256)
                user32.GetClassNameW(hwnd, class_buf, 256)
                class_name = class_buf.value.lower()
                terminal_classes = ('noctty', 'ghostty', 'console', 'cascadia', 'terminal', 'mintty', 'wezterm', 'alacritty', 'term', 'xterm')
                is_terminal = any(term in class_name for term in terminal_classes)
            except Exception:
                is_terminal = False

            if is_terminal:
                # In terminals, type characters directly via SendInput
                time.sleep(0.05)
                keyboard.write(text, exact=True)
            else:
                # In standard GUI apps, paste via Ctrl+V
                time.sleep(0.12)
                pyautogui.hotkey('ctrl', 'v')
                time.sleep(0.05)

            # Optionally restore original clipboard (commented out by default)
            # This allows users to paste the transcription again if needed
            # Uncomment if you prefer to restore original clipboard
            # time.sleep(0.5)
            # pyperclip.copy(original_clipboard)

            return True

        except Exception as e:
            print(f"❌ Error inserting text: {e}")
            return False

    @staticmethod
    def insert_with_newline(text: str, delay: float = 0.1) -> bool:
        """
        Insert text followed by Enter key
        Useful for chat applications or command prompts

        Args:
            text: Text to insert
            delay: Delay before actions (seconds)

        Returns:
            True if successful, False otherwise
        """
        if TextInserter.insert_text(text, delay):
            time.sleep(0.1)
            pyautogui.press('enter')
            return True
        return False

    @staticmethod
    def type_text_directly(text: str, interval: float = 0.01) -> bool:
        """
        Type text character by character (alternative method)
        Slower but more compatible with certain applications

        Args:
            text: Text to type
            interval: Delay between keystrokes (seconds)

        Returns:
            True if successful, False otherwise
        """
        try:
            pyautogui.write(text, interval=interval)
            return True
        except Exception as e:
            print(f"❌ Error typing text: {e}")
            return False
