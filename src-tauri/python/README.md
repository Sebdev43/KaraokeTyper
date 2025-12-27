# KaraokeTyper Python Services

This directory contains Python services for speech recognition using sherpa-onnx.

## Files

- `parakeet_service.py` - Main STT service using sherpa-onnx with Whisper models
- `gpu_detector.py` - GPU detection for optimal execution provider selection

## Architecture

```
Rust (Tauri) <--> Python (parakeet_service.py)
                  JSON via stdin/stdout
```

## Requirements

The Python dependencies are automatically installed by the application into a virtual environment:
- `~/.local/share/karaoketyper/venv/` (Linux)
- `%APPDATA%\karaoketyper\venv\` (Windows)
- `~/Library/Application Support/karaoketyper/venv/` (macOS)

### Main Dependencies

- `sherpa-onnx` - High-performance speech recognition
- `numpy` - Audio data processing
- `onnxruntime` - Model inference (CPU/GPU)

## Communication Protocol

### Commands

**Load Model:**
```json
{"command": "load_model", "model_path": "/path/to/model", "language": "fr"}
```

**Transcribe Audio:**
```json
{
  "command": "transcribe",
  "audio_data": [0.0, 0.1, -0.05, ...],
  "sample_rate": 16000
}
```

**Get Status:**
```json
{"command": "status"}
```

**Shutdown:**
```json
{"command": "shutdown"}
```

### Responses

**Success:**
```json
{"success": true, "text": "transcription result"}
```

**Error:**
```json
{"success": false, "error": "error message"}
```

## Supported Models

All models are downloaded automatically via the Settings UI:

| Model | Size | Speed | Accuracy |
|-------|------|-------|----------|
| Whisper Tiny | ~75MB | Fastest | Good |
| Whisper Base | ~150MB | Fast | Better |
| Whisper Small | ~500MB | Medium | Very Good |
| Whisper Medium | ~1.5GB | Slower | Excellent |
| Whisper Large-v3 | ~3GB | Slowest | Best |

Models are stored in `~/.cache/huggingface/hub/` (Linux/macOS) or `%USERPROFILE%\.cache\huggingface\hub\` (Windows).
