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
            # Store original clipboard content
            original_clipboard = pyperclip.paste()

            # Copy text to clipboard
            pyperclip.copy(text)

            # Small delay to ensure clipboard is updated
            time.sleep(delay)

            # Paste using Ctrl+V
            pyautogui.hotkey('ctrl', 'v')

            # Small delay after pasting
            time.sleep(0.1)

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
