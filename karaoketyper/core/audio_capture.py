"""Audio capture module for recording microphone input."""

import sounddevice as sd
import numpy as np
from queue import Queue
from typing import Optional
import logging

logger = logging.getLogger(__name__)


class AudioCapture:
    """Captures audio from microphone in real-time."""

    def __init__(self, sample_rate: int = 16000, channels: int = 1):
        """
        Initialize audio capture.

        Args:
            sample_rate: Audio sample rate in Hz (Vosk requires 16000)
            channels: Number of audio channels (1 for mono)
        """
        self.sample_rate = sample_rate
        self.channels = channels
        self.audio_queue: Queue = Queue()
        self.is_recording = False
        self.stream: Optional[sd.InputStream] = None

    def start(self) -> None:
        """Start recording audio from microphone."""
        if self.is_recording:
            logger.warning("Audio capture already running")
            return

        logger.info(f"Starting audio capture at {self.sample_rate}Hz")
        self.is_recording = True
        self.audio_queue = Queue()  # Reset queue

        try:
            self.stream = sd.InputStream(
                samplerate=self.sample_rate,
                channels=self.channels,
                dtype=np.int16,
                callback=self._audio_callback
            )
            self.stream.start()
            logger.info("Audio capture started successfully")
        except Exception as e:
            logger.error(f"Failed to start audio capture: {e}")
            self.is_recording = False
            raise

    def stop(self) -> bytes:
        """
        Stop recording and return captured audio data.

        Returns:
            Recorded audio as bytes
        """
        if not self.is_recording:
            logger.warning("Audio capture not running")
            return b''

        logger.info("Stopping audio capture")
        self.is_recording = False

        if self.stream:
            self.stream.stop()
            self.stream.close()
            self.stream = None

        audio_data = self._get_recorded_audio()
        logger.info(f"Captured {len(audio_data)} bytes of audio")
        return audio_data

    def _audio_callback(self, indata, frames, time, status):
        """
        Callback function called by sounddevice for each audio block.

        Args:
            indata: Input audio data
            frames: Number of frames
            time: Timing information
            status: Stream status
        """
        if status:
            logger.warning(f"Audio stream status: {status}")

        if self.is_recording:
            self.audio_queue.put(indata.copy())

    def _get_recorded_audio(self) -> bytes:
        """
        Retrieve all recorded audio from queue and concatenate.

        Returns:
            Complete audio recording as bytes
        """
        chunks = []
        while not self.audio_queue.empty():
            chunks.append(self.audio_queue.get())

        if not chunks:
            return b''

        # Concatenate all chunks
        audio_array = np.concatenate(chunks, axis=0)
        return audio_array.tobytes()

    def is_capturing(self) -> bool:
        """Check if currently capturing audio."""
        return self.is_recording
