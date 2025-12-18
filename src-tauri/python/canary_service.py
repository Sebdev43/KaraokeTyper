#!/usr/bin/env python3
"""
Canary-1B Speech Recognition Service
JSON-based IPC communication via stdin/stdout

IMPORTANT: All non-JSON output must go to stderr, not stdout!
stdout is reserved exclusively for JSON IPC responses.
"""
import sys
import json
import os
import tempfile
import io
import contextlib
from pathlib import Path
from typing import Optional

# Configure PyTorch memory allocator for better memory management
# This helps prevent fragmentation and reduces peak memory usage
os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")

# Redirect stdout to stderr during NeMo import to prevent log pollution
_real_stdout = sys.stdout
sys.stdout = sys.stderr

try:
    # Suppress NeMo's verbose output during import
    import warnings
    warnings.filterwarnings("ignore")

    # Configure logging to stderr before importing NeMo
    import logging
    logging.basicConfig(stream=sys.stderr, level=logging.WARNING)

    from nemo.collections.asr.models import EncDecMultiTaskModel
    import soundfile as sf
    import numpy as np
    import gc
    import torch
except ImportError as e:
    # Restore stdout for error response
    sys.stdout = _real_stdout
    print(json.dumps({
        "error": f"Missing dependency: {e}. Run: pip install nemo_toolkit[asr] soundfile",
        "type": "dependency_error"
    }), flush=True)
    sys.exit(1)

# Restore stdout for JSON IPC
sys.stdout = _real_stdout


@contextlib.contextmanager
def redirect_nemo_output():
    """Context manager to redirect stdout to stderr during NeMo operations."""
    old_stdout = sys.stdout
    sys.stdout = sys.stderr
    try:
        yield
    finally:
        sys.stdout = old_stdout


def cleanup_memory():
    """Force garbage collection and clear CUDA cache"""
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.synchronize()
        torch.cuda.empty_cache()
        # Clear cuBLAS workspaces (2024 fix for memory leaks)
        try:
            torch._C._cuda_clearCublasWorkspaces()
        except AttributeError:
            pass  # Not available in older PyTorch versions


class CanaryService:
    def __init__(self):
        self.model: Optional[EncDecMultiTaskModel] = None
        self.model_name: Optional[str] = None
        self.temp_dir = tempfile.mkdtemp(prefix="karaoketyper_")

    def download_model(self, model_name: str) -> dict:
        """Download Canary model from HuggingFace with progress reporting"""
        try:
            from huggingface_hub import snapshot_download, HfApi
            import threading
            import time

            model_id = f"nvidia/{model_name}"
            api = HfApi()

            # Get model info for size estimation
            try:
                model_info = api.model_info(model_id)
                total_size = sum(s.size for s in model_info.siblings if s.size) if model_info.siblings else 0
            except Exception:
                total_size = 0

            # Track download progress
            downloaded_bytes = [0]
            last_report = [0]

            def progress_callback(current: int, total: int):
                downloaded_bytes[0] = current
                now = time.time()
                # Report every 0.5 seconds
                if now - last_report[0] >= 0.5 or current >= total:
                    last_report[0] = now
                    percent = (current / total * 100) if total > 0 else 0
                    progress_msg = {
                        "type": "download_progress",
                        "model": model_name,
                        "downloaded_bytes": current,
                        "total_bytes": total,
                        "percent": percent
                    }
                    print(json.dumps(progress_msg), flush=True)

            # Download with progress
            with redirect_nemo_output():
                cache_dir = snapshot_download(
                    model_id,
                    local_files_only=False,
                )

            return {
                "success": True,
                "model": model_name,
                "cache_dir": str(cache_dir),
                "message": f"Downloaded {model_name} successfully"
            }
        except Exception as e:
            return {
                "success": False,
                "error": str(e),
                "type": "download_error"
            }

    def load_model(self, model_name: str, language: str = "fr", use_fp16: bool = True) -> dict:
        """Load Canary model from Hugging Face with optional fp16 for lower memory"""
        try:
            self.model_name = model_name
            # Redirect NeMo output to stderr to keep stdout clean for JSON
            with redirect_nemo_output():
                self.model = EncDecMultiTaskModel.from_pretrained(f"nvidia/{model_name}")

                # Convert to fp16 to reduce memory by ~50%
                if use_fp16 and torch.cuda.is_available():
                    self.model = self.model.half().cuda()
                    print(f"[CANARY] Using fp16 on CUDA", file=sys.stderr)
                elif use_fp16:
                    # CPU fp16 can be slower but uses less memory
                    self.model = self.model.half()
                    print(f"[CANARY] Using fp16 on CPU", file=sys.stderr)

                self.model.eval()  # Set to inference mode

            self.language = language
            cleanup_memory()

            return {
                "success": True,
                "model": model_name,
                "message": f"Loaded {model_name} successfully (fp16={use_fp16})"
            }
        except Exception as e:
            cleanup_memory()
            return {
                "success": False,
                "error": str(e),
                "type": "model_load_error"
            }

    def transcribe(self, audio_path: str, source_lang: str = "fr", target_lang: str = "fr") -> dict:
        """Transcribe audio file using Canary"""
        if self.model is None:
            return {
                "success": False,
                "error": "Model not loaded. Call load_model first.",
                "type": "model_not_loaded"
            }

        try:
            # Verify audio file exists
            if not os.path.exists(audio_path):
                return {
                    "success": False,
                    "error": f"Audio file not found: {audio_path}",
                    "type": "file_not_found"
                }

            # CRITICAL: Use inference_mode to prevent PyTorch from recording activations
            # This reduces memory usage significantly during inference
            with torch.inference_mode():
                # Transcribe using NeMo (redirect output to stderr)
                with redirect_nemo_output():
                    result = self.model.transcribe(
                        [audio_path],
                        source_lang=source_lang,
                        target_lang=target_lang,
                        batch_size=1
                    )

                # Extract text from result
                text = result[0].text if hasattr(result[0], 'text') else str(result[0])

            # Free memory after transcription (outside inference_mode)
            del result
            cleanup_memory()

            return {
                "success": True,
                "text": text,
                "source_lang": source_lang,
                "target_lang": target_lang
            }

        except Exception as e:
            cleanup_memory()
            return {
                "success": False,
                "error": str(e),
                "type": "transcription_error"
            }

    def transcribe_audio_data(self, audio_data: list, sample_rate: int = 16000,
                             source_lang: str = "fr", target_lang: str = "fr") -> dict:
        """Transcribe raw audio data (f32 samples)"""
        try:
            # Save audio data to temporary WAV file
            temp_wav = os.path.join(self.temp_dir, f"audio_{os.getpid()}.wav")

            # Convert list to numpy array
            audio_np = np.array(audio_data, dtype=np.float32)

            # Write WAV file (mono, 16kHz)
            sf.write(temp_wav, audio_np, sample_rate, subtype='PCM_16')

            # Free audio arrays before transcription
            del audio_np
            del audio_data
            cleanup_memory()

            # Transcribe
            result = self.transcribe(temp_wav, source_lang, target_lang)

            # Clean up temp file
            try:
                os.remove(temp_wav)
            except:
                pass

            return result

        except Exception as e:
            cleanup_memory()
            return {
                "success": False,
                "error": str(e),
                "type": "audio_processing_error"
            }

    def get_status(self) -> dict:
        """Get service status"""
        return {
            "success": True,
            "model_loaded": self.model is not None,
            "model_name": self.model_name,
            "language": getattr(self, 'language', None),
            "temp_dir": self.temp_dir
        }

    def unload_model(self) -> dict:
        """Unload model and free GPU memory"""
        try:
            if self.model is not None:
                # Move model to CPU first (critical for proper CUDA memory release)
                if torch.cuda.is_available():
                    self.model = self.model.cpu()

                # Delete model reference
                del self.model
                self.model = None
                self.model_name = None

                # Force memory cleanup
                cleanup_memory()

                return {"success": True, "message": "Model unloaded successfully"}
            return {"success": True, "message": "No model was loaded"}
        except Exception as e:
            return {"success": False, "error": str(e)}

    def shutdown(self) -> dict:
        """Clean shutdown"""
        try:
            # Unload model first to free GPU memory
            self.unload_model()

            # Clean up temp directory
            import shutil
            if os.path.exists(self.temp_dir):
                shutil.rmtree(self.temp_dir)

            return {"success": True, "message": "Service shutdown complete"}
        except Exception as e:
            return {"success": False, "error": str(e)}


def main():
    """Main loop - process JSON commands from stdin"""
    service = CanaryService()

    # Send ready signal
    print(json.dumps({"ready": True, "message": "Canary service started"}), flush=True)

    try:
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue

            try:
                request = json.loads(line)
                command = request.get("command")

                if command == "download_model":
                    response = service.download_model(
                        request.get("model_name", "canary-1b-flash")
                    )

                elif command == "load_model":
                    response = service.load_model(
                        request.get("model_name", "canary-1b-flash"),
                        request.get("language", "fr"),
                        request.get("use_fp16", True)  # Default to fp16 for lower memory
                    )

                elif command == "transcribe":
                    response = service.transcribe(
                        request["audio_path"],
                        request.get("source_lang", "fr"),
                        request.get("target_lang", "fr")
                    )

                elif command == "transcribe_audio_data":
                    response = service.transcribe_audio_data(
                        request["audio_data"],
                        request.get("sample_rate", 16000),
                        request.get("source_lang", "fr"),
                        request.get("target_lang", "fr")
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
                error_response = {
                    "success": False,
                    "error": f"Invalid JSON: {e}",
                    "type": "json_error"
                }
                print(json.dumps(error_response), flush=True)

            except Exception as e:
                error_response = {
                    "success": False,
                    "error": str(e),
                    "type": "unexpected_error"
                }
                print(json.dumps(error_response), flush=True)

    except KeyboardInterrupt:
        service.shutdown()
        sys.exit(0)


if __name__ == "__main__":
    main()
