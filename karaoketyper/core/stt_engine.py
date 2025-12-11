"""Speech-to-text engine using Vosk."""

from vosk import Model, KaldiRecognizer
import json
import logging
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


class STTEngine:
    """Speech-to-text engine powered by Vosk."""

    DEFAULT_MODEL_PATH = Path.home() / ".local/share/karaoketyper/models/vosk-model-en"

    def __init__(self, model_path: Optional[Path] = None, sample_rate: int = 16000):
        """
        Initialize STT engine with Vosk model.

        Args:
            model_path: Path to Vosk model directory
            sample_rate: Audio sample rate (must match audio capture)
        """
        self.model_path = model_path or self.DEFAULT_MODEL_PATH
        self.sample_rate = sample_rate
        self.model: Optional[Model] = None
        self.recognizer: Optional[KaldiRecognizer] = None

        self._load_model()

    def _load_model(self) -> None:
        """Load Vosk model from disk."""
        if not self.model_path.exists():
            raise FileNotFoundError(
                f"Vosk model not found at {self.model_path}. "
                "Please download a model first using model_manager."
            )

        logger.info(f"Loading Vosk model from {self.model_path}")
        try:
            self.model = Model(str(self.model_path))
            self.recognizer = KaldiRecognizer(self.model, self.sample_rate)
            logger.info("Vosk model loaded successfully")
        except Exception as e:
            logger.error(f"Failed to load Vosk model: {e}")
            raise

    def transcribe(self, audio_data: bytes) -> str:
        """
        Transcribe audio bytes to text.

        Args:
            audio_data: Raw audio bytes (PCM 16-bit)

        Returns:
            Transcribed text string
        """
        if not self.recognizer:
            logger.error("Recognizer not initialized")
            return ""

        try:
            # Process audio data
            if self.recognizer.AcceptWaveform(audio_data):
                result = json.loads(self.recognizer.Result())
                text = result.get('text', '').strip()

                if text:
                    logger.info(f"Transcribed: '{text}'")
                else:
                    logger.warning("Empty transcription result")

                return text
            else:
                # Partial result (not final)
                partial = json.loads(self.recognizer.PartialResult())
                text = partial.get('partial', '').strip()
                logger.debug(f"Partial transcription: '{text}'")
                return text

        except Exception as e:
            logger.error(f"Transcription error: {e}")
            return ""

    def finalize(self) -> str:
        """
        Get final transcription result.

        Useful for getting remaining text after last AcceptWaveform call.

        Returns:
            Final transcribed text
        """
        if not self.recognizer:
            return ""

        try:
            result = json.loads(self.recognizer.FinalResult())
            text = result.get('text', '').strip()
            logger.info(f"Final transcription: '{text}'")
            return text
        except Exception as e:
            logger.error(f"Finalization error: {e}")
            return ""

    def reset(self) -> None:
        """Reset recognizer for new transcription session."""
        if self.model:
            logger.debug("Resetting recognizer")
            self.recognizer = KaldiRecognizer(self.model, self.sample_rate)

    def is_ready(self) -> bool:
        """Check if STT engine is ready to transcribe."""
        return self.model is not None and self.recognizer is not None
