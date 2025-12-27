#!/usr/bin/env python3
"""
Parakeet-TDT Speech Recognition Service via sherpa-onnx

Cross-platform STT using ONNX Runtime with multiple execution providers:
- CUDAExecutionProvider: NVIDIA GPUs
- MIGraphXExecutionProvider: AMD GPUs (ROCm)
- CoreMLExecutionProvider: Apple Silicon
- CPUExecutionProvider: Universal fallback

JSON-based IPC communication via stdin/stdout.
IMPORTANT: All non-JSON output must go to stderr, not stdout!
"""

import sys
import json
import os
import tempfile
import gc
from pathlib import Path
from typing import Optional, Dict, Any, List

# Redirect stdout to stderr during imports
_real_stdout = sys.stdout
sys.stdout = sys.stderr

try:
    import warnings
    warnings.filterwarnings("ignore")

    import logging
    logging.basicConfig(stream=sys.stderr, level=logging.WARNING)

    import numpy as np
    import soundfile as sf

except ImportError as e:
    sys.stdout = _real_stdout
    print(json.dumps({
        "error": f"Missing dependency: {e}. Run installation from Settings.",
        "type": "dependency_error"
    }), flush=True)
    sys.exit(1)

# Restore stdout for JSON IPC
sys.stdout = _real_stdout


def log_stderr(message: str):
    """Log to stderr to keep stdout clean for JSON"""
    print(f"[PARAKEET] {message}", file=sys.stderr, flush=True)


class ParakeetService:
    """
    Parakeet-TDT STT Service using sherpa-onnx

    Supports multiple ONNX Runtime execution providers for cross-platform GPU acceleration.
    """

    def __init__(self):
        self.recognizer = None
        self.model_name: Optional[str] = None
        self.provider: Optional[str] = None
        self.temp_dir = tempfile.mkdtemp(prefix="karaoketyper_parakeet_")
        self.sample_rate = 16000  # Parakeet expects 16kHz audio

    def get_available_providers(self) -> Dict[str, Any]:
        """Get available ONNX Runtime execution providers"""
        try:
            import onnxruntime as ort
            available = ort.get_available_providers()
            return {
                "success": True,
                "providers": available,
                "has_cuda": "CUDAExecutionProvider" in available,
                "has_rocm": "MIGraphXExecutionProvider" in available or "ROCMExecutionProvider" in available,
                "has_coreml": "CoreMLExecutionProvider" in available,
            }
        except ImportError:
            return {
                "success": False,
                "error": "onnxruntime not installed",
                "providers": []
            }

    def download_model(self, model_name: str = "parakeet-tdt-0.6b") -> Dict[str, Any]:
        """
        Download Parakeet model via sherpa-onnx model download

        Available models:
        - parakeet-tdt-0.6b: 600M params, fast, 25 EU languages
        - parakeet-tdt-1.1b: 1.1B params, more accurate, slower
        """
        try:
            # Suppress huggingface_hub logging noise
            os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"
            os.environ["TOKENIZERS_PARALLELISM"] = "false"

            from huggingface_hub import snapshot_download
            import threading
            import time

            # Map model names to HuggingFace repo
            # Models are from csukuangfj (sherpa-onnx maintainer)
            # See: https://huggingface.co/csukuangfj
            model_repos = {
                # Whisper multilingual models (99 languages including French)
                "sherpa-onnx-whisper-tiny": "csukuangfj/sherpa-onnx-whisper-tiny",
                "sherpa-onnx-whisper-base": "csukuangfj/sherpa-onnx-whisper-base",
                "sherpa-onnx-whisper-small": "csukuangfj/sherpa-onnx-whisper-small",
                "sherpa-onnx-whisper-medium": "csukuangfj/sherpa-onnx-whisper-medium",
                "sherpa-onnx-whisper-large-v3": "csukuangfj/sherpa-onnx-whisper-large-v3",
                # Asian language models (Chinese/Japanese/Korean/Cantonese - NO FRENCH)
                "sherpa-onnx-paraformer": "csukuangfj/sherpa-onnx-paraformer-zh-2023-03-28",
                "sherpa-onnx-sense-voice": "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17",
            }

            # Expected sizes in MB for progress estimation
            model_sizes = {
                "sherpa-onnx-whisper-tiny": 75,
                "sherpa-onnx-whisper-base": 150,
                "sherpa-onnx-whisper-small": 400,
                "sherpa-onnx-whisper-medium": 1000,
                "sherpa-onnx-whisper-large-v3": 3000,
                "sherpa-onnx-sense-voice": 500,
                "sherpa-onnx-paraformer": 300,
            }

            repo_id = model_repos.get(model_name, f"k2-fsa/sherpa-onnx-{model_name}")
            expected_size_mb = model_sizes.get(model_name, 100)
            log_stderr(f"Downloading {model_name} from {repo_id}...")

            # Get cache directory path - huggingface downloads to blobs first
            cache_base = Path(os.environ.get("HF_HOME", Path.home() / ".cache" / "huggingface")) / "hub"
            repo_folder = f"models--{repo_id.replace('/', '--')}"
            repo_cache = cache_base / repo_folder
            blobs_dir = repo_cache / "blobs"

            log_stderr(f"Monitoring cache at: {repo_cache}")

            # Progress monitoring in background thread
            download_complete = threading.Event()
            last_emitted = [0]  # Use list to allow modification in nested function

            def monitor_progress():
                log_stderr("Progress monitor thread started")
                iteration = 0
                while not download_complete.is_set():
                    try:
                        current_size = 0
                        files_found = 0
                        # Monitor both blobs (during download) and snapshots (after completion)
                        for check_dir in [blobs_dir, repo_cache]:
                            if check_dir.exists():
                                for f in check_dir.rglob("*"):
                                    if f.is_file():
                                        try:
                                            current_size += f.stat().st_size
                                            files_found += 1
                                        except (OSError, FileNotFoundError):
                                            pass

                        current_mb = current_size / (1024 * 1024)

                        # Log every 10 iterations for debug
                        iteration += 1
                        if iteration % 10 == 0:
                            log_stderr(f"Monitor: {current_mb:.1f}MB, {files_found} files in {repo_cache}")

                        # Only emit if changed by at least 0.5 MB
                        if current_mb - last_emitted[0] >= 0.5:
                            last_emitted[0] = current_mb
                            percent = min(99, (current_mb / expected_size_mb) * 100)
                            progress_msg = {
                                "type": "download_progress",
                                "model": model_name,
                                "percent": round(percent, 1),
                                "downloaded_bytes": int(current_size),
                                "total_bytes": int(expected_size_mb * 1024 * 1024)
                            }
                            print(json.dumps(progress_msg), flush=True)
                            log_stderr(f"Emitted progress: {percent:.1f}%")
                    except Exception as e:
                        log_stderr(f"Progress monitor error: {e}")
                    time.sleep(0.3)
                log_stderr("Progress monitor thread finished")

            # Start progress monitor
            monitor_thread = threading.Thread(target=monitor_progress, daemon=True)
            monitor_thread.start()

            # Emit initial progress immediately
            print(json.dumps({
                "type": "download_progress",
                "model": model_name,
                "percent": 0,
                "downloaded_bytes": 0,
                "total_bytes": int(expected_size_mb * 1024 * 1024)
            }), flush=True)

            try:
                log_stderr(f"Starting snapshot_download for {repo_id}...")
                # Run download in subprocess to avoid stdin/stdout piping issues
                import subprocess
                download_script = f'''
import os
os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"
from huggingface_hub import snapshot_download
path = snapshot_download("{repo_id}", local_files_only=False)
print(path)
'''
                result = subprocess.run(
                    [sys.executable, "-c", download_script],
                    capture_output=True,
                    text=True,
                    timeout=600  # 10 minutes timeout
                )
                if result.returncode != 0:
                    raise Exception(f"Download subprocess failed: {result.stderr}")
                cache_dir = result.stdout.strip()
                log_stderr(f"snapshot_download completed: {cache_dir}")
            except subprocess.TimeoutExpired:
                log_stderr("snapshot_download TIMEOUT after 10 minutes")
                download_complete.set()
                monitor_thread.join(timeout=2)
                raise Exception("Download timed out after 10 minutes")
            except Exception as download_error:
                log_stderr(f"snapshot_download FAILED: {download_error}")
                download_complete.set()
                monitor_thread.join(timeout=2)
                raise download_error
            finally:
                download_complete.set()
                monitor_thread.join(timeout=2)

            log_stderr(f"Model downloaded to: {cache_dir}")

            return {
                "success": True,
                "model": model_name,
                "cache_dir": str(cache_dir),
                "message": f"Downloaded {model_name} successfully"
            }

        except ImportError as e:
            return {
                "success": False,
                "error": f"huggingface_hub not installed: {e}",
                "type": "dependency_error"
            }
        except Exception as e:
            return {
                "success": False,
                "error": str(e),
                "type": "download_error"
            }

    def _select_provider(self, preferred: Optional[str] = None) -> str:
        """Select the best available ONNX Runtime provider"""
        try:
            import onnxruntime as ort
            available = ort.get_available_providers()

            # Priority order
            priority = [
                "CUDAExecutionProvider",
                "MIGraphXExecutionProvider",
                "ROCMExecutionProvider",
                "CoreMLExecutionProvider",
                "CPUExecutionProvider"
            ]

            if preferred and preferred in available:
                return preferred

            for provider in priority:
                if provider in available:
                    return provider

            return "CPUExecutionProvider"
        except Exception:
            return "CPUExecutionProvider"

    def load_model(
        self,
        model_path: str,
        provider: Optional[str] = None,
        num_threads: int = 4,
        language: str = "fr"
    ) -> Dict[str, Any]:
        """
        Load Parakeet/sherpa-onnx model with specified execution provider

        Args:
            model_path: Path to model directory containing ONNX files
            provider: ONNX Runtime execution provider (auto-detected if None)
            num_threads: Number of CPU threads for inference
        """
        try:
            import sherpa_onnx

            self.provider = self._select_provider(provider)
            log_stderr(f"Using execution provider: {self.provider}")

            # Find model files in the directory
            model_dir = Path(model_path)
            if not model_dir.exists():
                return {
                    "success": False,
                    "error": f"Model directory not found: {model_path}",
                    "type": "file_not_found"
                }

            # Look for encoder and decoder files
            encoder_file = None
            decoder_file = None
            joiner_file = None
            tokens_file = None

            for f in model_dir.rglob("*.onnx"):
                fname = f.name.lower()
                if "encoder" in fname:
                    encoder_file = str(f)
                elif "decoder" in fname:
                    decoder_file = str(f)
                elif "joiner" in fname:
                    joiner_file = str(f)

            for f in model_dir.rglob("tokens.txt"):
                tokens_file = str(f)

            # Also check for single model file patterns
            for f in model_dir.rglob("*.onnx"):
                if not encoder_file:
                    encoder_file = str(f)
                    break

            if not tokens_file:
                for f in model_dir.rglob("*.txt"):
                    if "token" in f.name.lower() or "vocab" in f.name.lower():
                        tokens_file = str(f)
                        break

            # Configure provider
            provider_config = ""
            if self.provider == "CUDAExecutionProvider":
                provider_config = "cuda"
            elif self.provider in ["MIGraphXExecutionProvider", "ROCMExecutionProvider"]:
                provider_config = "rocm"
            elif self.provider == "CoreMLExecutionProvider":
                provider_config = "coreml"

            # Create recognizer based on model type
            if decoder_file and joiner_file:
                # Transducer model (streaming)
                log_stderr("Loading transducer model...")
                self.recognizer = sherpa_onnx.OfflineRecognizer.from_transducer(
                    encoder=encoder_file,
                    decoder=decoder_file,
                    joiner=joiner_file,
                    tokens=tokens_file,
                    num_threads=num_threads,
                    provider=provider_config or "cpu",
                )
            else:
                # Detect model type by checking file structure
                # sherpa-onnx Whisper models: base-encoder.onnx, base-decoder.onnx, base-tokens.txt
                # or: tiny-encoder.onnx, small-encoder.onnx, etc.
                whisper_encoder = None
                whisper_decoder = None
                paraformer_model = None
                sense_voice_model = None

                log_stderr(f"Scanning model files in: {model_dir}")
                for f in model_dir.rglob("*.onnx"):
                    fname = f.name.lower()
                    log_stderr(f"  Found: {f.name}")

                    # Whisper models: *-encoder.onnx, *-decoder.onnx (prefer non-int8)
                    if "encoder" in fname and "int8" not in fname:
                        whisper_encoder = str(f)
                    elif "decoder" in fname and "int8" not in fname:
                        whisper_decoder = str(f)
                    elif "paraformer" in fname:
                        paraformer_model = str(f)

                # Find tokens file - check multiple patterns
                if not tokens_file:
                    for pattern in ["*-tokens.txt", "*tokens*.txt", "tokens.txt"]:
                        for f in model_dir.rglob(pattern):
                            tokens_file = str(f)
                            log_stderr(f"  Found tokens: {f.name}")
                            break
                        if tokens_file:
                            break

                # Detect model type by directory name (more reliable than file names)
                model_dir_str = str(model_dir).lower()
                is_sense_voice = "sense-voice" in model_dir_str or "sensevoice" in model_dir_str
                is_paraformer = "paraformer" in model_dir_str

                if is_sense_voice:
                    # SenseVoice uses model.onnx (non-int8 version)
                    for f in model_dir.rglob("*.onnx"):
                        if "int8" not in f.name.lower():
                            sense_voice_model = str(f)
                            break

                if is_paraformer:
                    # Paraformer uses single model.onnx file
                    for f in model_dir.rglob("*.onnx"):
                        if "int8" not in f.name.lower():
                            paraformer_model = str(f)
                            break

                # Check if it's a Whisper model (has encoder.onnx and decoder.onnx)
                if whisper_encoder and whisper_decoder:
                    log_stderr(f"Loading Whisper model...")
                    log_stderr(f"  Encoder: {whisper_encoder}")
                    log_stderr(f"  Decoder: {whisper_decoder}")
                    log_stderr(f"  Tokens: {tokens_file}")
                    log_stderr(f"  Language: {language}")

                    self.recognizer = sherpa_onnx.OfflineRecognizer.from_whisper(
                        encoder=whisper_encoder,
                        decoder=whisper_decoder,
                        tokens=tokens_file,
                        language=language,
                        task="transcribe",
                        num_threads=num_threads,
                        provider=provider_config or "cpu",
                    )
                elif sense_voice_model and tokens_file:
                    log_stderr(f"Loading SenseVoice model: {sense_voice_model}")
                    log_stderr(f"  Model: {sense_voice_model}")
                    log_stderr(f"  Tokens: {tokens_file}")
                    log_stderr(f"  Language: {language}")
                    self.recognizer = sherpa_onnx.OfflineRecognizer.from_sense_voice(
                        model=sense_voice_model,
                        tokens=tokens_file,
                        language=language,
                        num_threads=num_threads,
                        provider=provider_config or "cpu",
                        use_itn=True,  # Enable inverse text normalization (for better output)
                    )
                elif paraformer_model and tokens_file:
                    log_stderr(f"Loading Paraformer model: {paraformer_model}")
                    self.recognizer = sherpa_onnx.OfflineRecognizer.from_paraformer(
                        paraformer=paraformer_model,
                        tokens=tokens_file,
                        num_threads=num_threads,
                        provider=provider_config or "cpu",
                    )
                else:
                    # Fallback: try any single onnx file as paraformer
                    model_file = None
                    for f in model_dir.rglob("*.onnx"):
                        model_file = str(f)
                        break

                    if model_file and tokens_file:
                        log_stderr(f"Loading model (fallback): {model_file}")
                        self.recognizer = sherpa_onnx.OfflineRecognizer.from_paraformer(
                            paraformer=model_file,
                            tokens=tokens_file,
                            num_threads=num_threads,
                            provider=provider_config or "cpu",
                        )

            if self.recognizer is None:
                return {
                    "success": False,
                    "error": "Could not load model. Check model format.",
                    "type": "model_load_error"
                }

            self.model_name = model_path
            log_stderr(f"Model loaded successfully with {self.provider}")

            return {
                "success": True,
                "model": model_path,
                "provider": self.provider,
                "message": f"Loaded model with {self.provider}"
            }

        except ImportError as e:
            return {
                "success": False,
                "error": f"sherpa-onnx not installed: {e}",
                "type": "dependency_error"
            }
        except Exception as e:
            return {
                "success": False,
                "error": str(e),
                "type": "model_load_error"
            }

    def transcribe(self, audio_path: str, language: str = "fr") -> Dict[str, Any]:
        """Transcribe audio file"""
        if self.recognizer is None:
            return {
                "success": False,
                "error": "Model not loaded. Call load_model first.",
                "type": "model_not_loaded"
            }

        try:
            if not os.path.exists(audio_path):
                return {
                    "success": False,
                    "error": f"Audio file not found: {audio_path}",
                    "type": "file_not_found"
                }

            # Read audio file
            audio_data, sample_rate = sf.read(audio_path, dtype="float32")

            # Resample if necessary
            if sample_rate != self.sample_rate:
                # Simple resampling
                ratio = self.sample_rate / sample_rate
                new_length = int(len(audio_data) * ratio)
                indices = np.linspace(0, len(audio_data) - 1, new_length).astype(int)
                audio_data = audio_data[indices]

            # Convert to mono if stereo
            if len(audio_data.shape) > 1:
                audio_data = audio_data.mean(axis=1)

            # Create stream and recognize
            stream = self.recognizer.create_stream()
            stream.accept_waveform(self.sample_rate, audio_data)
            self.recognizer.decode_stream(stream)

            text = stream.result.text.strip()

            # Cleanup
            del stream
            del audio_data
            gc.collect()

            return {
                "success": True,
                "text": text,
                "language": language,
                "provider": self.provider
            }

        except Exception as e:
            gc.collect()
            return {
                "success": False,
                "error": str(e),
                "type": "transcription_error"
            }

    def transcribe_audio_data(
        self,
        audio_data: List[float],
        sample_rate: int = 16000,
        language: str = "fr"
    ) -> Dict[str, Any]:
        """Transcribe raw audio data (f32 samples)"""
        try:
            # Save to temp file and transcribe
            temp_wav = os.path.join(self.temp_dir, f"audio_{os.getpid()}.wav")
            audio_np = np.array(audio_data, dtype=np.float32)
            sf.write(temp_wav, audio_np, sample_rate, subtype='PCM_16')

            del audio_np
            del audio_data
            gc.collect()

            result = self.transcribe(temp_wav, language)

            try:
                os.remove(temp_wav)
            except:
                pass

            return result

        except Exception as e:
            gc.collect()
            return {
                "success": False,
                "error": str(e),
                "type": "audio_processing_error"
            }

    def get_status(self) -> Dict[str, Any]:
        """Get service status"""
        providers_info = self.get_available_providers()
        return {
            "success": True,
            "model_loaded": self.recognizer is not None,
            "model_name": self.model_name,
            "provider": self.provider,
            "temp_dir": self.temp_dir,
            "available_providers": providers_info.get("providers", [])
        }

    def unload_model(self) -> Dict[str, Any]:
        """Unload model and free memory"""
        try:
            if self.recognizer is not None:
                del self.recognizer
                self.recognizer = None
                self.model_name = None
                gc.collect()
                return {"success": True, "message": "Model unloaded successfully"}
            return {"success": True, "message": "No model was loaded"}
        except Exception as e:
            return {"success": False, "error": str(e)}

    def shutdown(self) -> Dict[str, Any]:
        """Clean shutdown"""
        try:
            self.unload_model()
            import shutil
            if os.path.exists(self.temp_dir):
                shutil.rmtree(self.temp_dir)
            return {"success": True, "message": "Service shutdown complete"}
        except Exception as e:
            return {"success": False, "error": str(e)}


def main():
    """Main loop - process JSON commands from stdin"""
    service = ParakeetService()

    # Send ready signal
    print(json.dumps({
        "ready": True,
        "message": "Parakeet service started",
        "providers": service.get_available_providers()
    }), flush=True)

    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue

            try:
                request = json.loads(line)
                command = request.get("command")

                if command == "detect_providers":
                    response = service.get_available_providers()

                elif command == "download_model":
                    response = service.download_model(
                        request.get("model_name", "parakeet-tdt-0.6b")
                    )

                elif command == "load_model":
                    response = service.load_model(
                        request["model_path"],
                        request.get("provider"),
                        request.get("num_threads", 4),
                        request.get("language", "fr")
                    )

                elif command == "transcribe":
                    response = service.transcribe(
                        request["audio_path"],
                        request.get("language", "fr")
                    )

                elif command == "transcribe_audio_data":
                    response = service.transcribe_audio_data(
                        request["audio_data"],
                        request.get("sample_rate", 16000),
                        request.get("language", "fr")
                    )

                elif command == "status":
                    response = service.get_status()

                elif command == "unload_model":
                    response = service.unload_model()

                elif command == "shutdown":
                    response = service.shutdown()
                    print(json.dumps(response), flush=True)
                    break

                else:
                    response = {
                        "success": False,
                        "error": f"Unknown command: {command}",
                        "type": "invalid_command"
                    }

                print(json.dumps(response), flush=True)

            except json.JSONDecodeError as e:
                print(json.dumps({
                    "success": False,
                    "error": f"Invalid JSON: {e}",
                    "type": "json_error"
                }), flush=True)

            except KeyError as e:
                print(json.dumps({
                    "success": False,
                    "error": f"Missing required field: {e}",
                    "type": "missing_field"
                }), flush=True)

            except Exception as e:
                print(json.dumps({
                    "success": False,
                    "error": str(e),
                    "type": "unexpected_error"
                }), flush=True)

    except KeyboardInterrupt:
        service.shutdown()
        sys.exit(0)


if __name__ == "__main__":
    main()
