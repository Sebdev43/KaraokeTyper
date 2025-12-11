"""KaraokeTyper - Offline voice typing for Ubuntu

Main entry point for the application.
"""

import sys
import logging
from pathlib import Path

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s',
    handlers=[
        logging.StreamHandler(sys.stdout),
        logging.FileHandler(Path.home() / '.local/share/karaoketyper/app.log')
    ]
)

logger = logging.getLogger(__name__)


def main():
    """Main entry point."""
    logger.info("Starting KaraokeTyper...")

    # TODO: Initialize GTK4 application
    # TODO: Load configuration
    # TODO: Start services (shortcut listener, audio capture, etc.)

    print("🎤 KaraokeTyper v0.1.0")
    print("=" * 50)
    print("Offline voice typing for Ubuntu")
    print("=" * 50)
    print()
    print("⚠️  This is a work in progress!")
    print()
    print("Core modules created:")
    print("  ✓ audio_capture.py - Microphone recording")
    print("  ✓ stt_engine.py - Vosk speech recognition")
    print("  ✓ text_output.py - ydotool typing")
    print()
    print("Next steps:")
    print("  1. Install dependencies: pip install -r requirements.txt")
    print("  2. Download Vosk model")
    print("  3. Install ydotool and start ydotoold service")
    print("  4. Build GTK4 UI")
    print()
    print("Press Ctrl+C to exit")

    try:
        # Keep running
        import time
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        logger.info("Shutting down KaraokeTyper...")
        print("\n👋 Goodbye!")


if __name__ == '__main__':
    main()
