#!/bin/bash
# Download Whisper Base model for KaraokeTyper

set -e

MODEL_DIR="src-tauri/models"
MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
MODEL_FILE="$MODEL_DIR/ggml-base.bin"

echo "🎤 KaraokeTyper - Whisper Model Downloader"
echo "=========================================="
echo ""

# Create models directory if it doesn't exist
mkdir -p "$MODEL_DIR"

# Check if model already exists
if [ -f "$MODEL_FILE" ]; then
    echo "✓ Model already exists at: $MODEL_FILE"
    FILE_SIZE=$(du -h "$MODEL_FILE" | cut -f1)
    echo "  File size: $FILE_SIZE"
    echo ""
    read -p "Do you want to re-download? (y/N): " -n 1 -r
    echo ""
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Skipping download."
        exit 0
    fi
fi

echo "Downloading Whisper Base model (~150MB)..."
echo "From: $MODEL_URL"
echo "To: $MODEL_FILE"
echo ""

# Download with progress bar
if command -v wget &> /dev/null; then
    wget -O "$MODEL_FILE" "$MODEL_URL" --progress=bar:force
elif command -v curl &> /dev/null; then
    curl -L -o "$MODEL_FILE" "$MODEL_URL" --progress-bar
else
    echo "❌ Error: Neither wget nor curl is installed."
    echo "Please install either wget or curl to download the model."
    exit 1
fi

# Verify download
if [ -f "$MODEL_FILE" ]; then
    FILE_SIZE=$(du -h "$MODEL_FILE" | cut -f1)
    echo ""
    echo "✓ Download complete!"
    echo "  Model saved to: $MODEL_FILE"
    echo "  File size: $FILE_SIZE"
    echo ""
    echo "You can now build and run KaraokeTyper with:"
    echo "  npm run tauri dev"
else
    echo "❌ Download failed!"
    exit 1
fi
