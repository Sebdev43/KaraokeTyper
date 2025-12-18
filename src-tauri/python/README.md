# Canary-1B Python Service

This service provides NVIDIA Canary-1B speech recognition via Python subprocess.

## Installation

### 1. Install Python 3.8+

**Linux:**
```bash
sudo apt install python3 python3-pip
```

**macOS:**
```bash
brew install python3
```

**Windows:**
Download from https://python.org/downloads

### 2. Install NeMo Toolkit

```bash
pip install -r requirements.txt
```

This will download ~2GB of dependencies including:
- NVIDIA NeMo Framework
- PyTorch
- Audio processing libraries

### 3. First Run

On first run, NeMo will automatically download the selected Canary model to:
- Linux/macOS: `~/.cache/huggingface/hub/`
- Windows: `%USERPROFILE%\.cache\huggingface\hub\`

Model sizes:
- `canary-1b-flash`: ~700MB (recommended)
- `canary-1b`: ~3.5GB
- `canary-1b-v2`: ~3.5GB (25 languages)

## Architecture

```
Rust (Tauri) ←→ Python (canary_service.py)
               JSON via stdin/stdout
```

### Communication Protocol

#### Commands

**Load Model:**
```json
{"command": "load_model", "model_name": "canary-1b-flash", "language": "fr"}
```

**Transcribe Audio Data:**
```json
{
  "command": "transcribe_audio_data",
  "audio_data": [0.0, 0.1, -0.05, ...],
  "sample_rate": 16000,
  "source_lang": "fr",
  "target_lang": "fr"
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

#### Responses

**Success:**
```json
{"success": true, "text": "transcription result"}
```

**Error:**
```json
{"success": false, "error": "error message", "type": "error_type"}
```

## Testing Standalone

```bash
# Run service directly
python3 canary_service.py

# Send commands via stdin
echo '{"command":"load_model","model_name":"canary-1b-flash","language":"fr"}' | python3 canary_service.py
```

## Troubleshooting

### Import Error: ModuleNotFoundError

```bash
pip install nemo_toolkit[asr]
```

### CUDA/GPU Issues

NeMo will use GPU if available, but CPU-only also works (slower).

### Model Download Fails

Check internet connection and Hugging Face access:
```bash
huggingface-cli login  # Optional: for faster downloads
```
