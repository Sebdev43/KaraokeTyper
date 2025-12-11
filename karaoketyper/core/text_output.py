"""Text output module for typing transcribed text."""

import subprocess
import tempfile
import logging
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


class TextOutput:
    """Handles typing text into active window using ydotool."""

    def __init__(self, key_delay_ms: int = 8):
        """
        Initialize text output.

        Args:
            key_delay_ms: Delay between keystrokes in milliseconds
        """
        self.key_delay_ms = key_delay_ms
        self._check_ydotool()

    def _check_ydotool(self) -> None:
        """Verify ydotool is installed and ydotoold is running."""
        try:
            # Check if ydotool command exists
            result = subprocess.run(
                ['which', 'ydotool'],
                capture_output=True,
                text=True
            )

            if result.returncode != 0:
                raise FileNotFoundError(
                    "ydotool not found. Please install it: sudo apt install ydotool"
                )

            logger.info("ydotool found")

            # Check if ydotoold daemon is running
            result = subprocess.run(
                ['pgrep', '-x', 'ydotoold'],
                capture_output=True
            )

            if result.returncode != 0:
                logger.warning(
                    "ydotoold daemon not running. "
                    "Please start it: systemctl --user start ydotoold"
                )

        except Exception as e:
            logger.error(f"Error checking ydotool: {e}")

    def type_text(self, text: str) -> bool:
        """
        Type text into the currently focused application.

        Args:
            text: Text to type

        Returns:
            True if successful, False otherwise
        """
        if not text:
            logger.warning("Empty text provided, nothing to type")
            return False

        logger.info(f"Typing text: '{text}' ({len(text)} chars)")

        try:
            # Create temporary file with text
            with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.txt') as f:
                f.write(text)
                temp_path = f.name

            # Type via ydotool
            result = subprocess.run(
                [
                    'ydotool', 'type',
                    '--key-delay', str(self.key_delay_ms),
                    '--file', temp_path
                ],
                capture_output=True,
                text=True,
                timeout=30
            )

            # Cleanup temp file
            Path(temp_path).unlink(missing_ok=True)

            if result.returncode == 0:
                logger.info("Text typed successfully")
                return True
            else:
                logger.error(f"ydotool failed: {result.stderr}")
                return False

        except subprocess.TimeoutExpired:
            logger.error("ydotool timed out")
            return False
        except Exception as e:
            logger.error(f"Error typing text: {e}")
            return False

    def paste_from_clipboard(self) -> bool:
        """
        Simulate Ctrl+V to paste from clipboard.

        This is an alternative to typing character by character.

        Returns:
            True if successful, False otherwise
        """
        logger.info("Simulating Ctrl+V paste")

        try:
            # Ctrl key down (29), V key down (47), V key up, Ctrl key up
            result = subprocess.run(
                ['ydotool', 'key', '29:1', '47:1', '47:0', '29:0'],
                capture_output=True,
                text=True,
                timeout=5
            )

            if result.returncode == 0:
                logger.info("Paste command sent successfully")
                return True
            else:
                logger.error(f"Paste failed: {result.stderr}")
                return False

        except Exception as e:
            logger.error(f"Error pasting from clipboard: {e}")
            return False

    def set_key_delay(self, delay_ms: int) -> None:
        """
        Update key delay for future typing operations.

        Args:
            delay_ms: New delay in milliseconds
        """
        if delay_ms < 0:
            logger.warning(f"Invalid delay {delay_ms}ms, using 0ms")
            delay_ms = 0

        self.key_delay_ms = delay_ms
        logger.debug(f"Key delay set to {delay_ms}ms")
