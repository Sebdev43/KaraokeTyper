# Changelog

All notable changes to KaraokeTyper will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2025-01-08

### Fixed

- **Audio Clipping at Start**: Added 200ms silence padding at beginning of audio to prevent first words from being cut off (e.g., "c'est l'" no longer missing)
- GStreamer pipeline startup delay (~106ms) now compensated with start padding

## [1.0.0] - 2024-12-30

### Changed

- **Complete STT Engine Rewrite**: Switched from Parakeet to native Whisper-rs for better performance and reliability
- **GStreamer Audio Pipeline**: New on-demand audio capture (always_on_microphone=false) - mic only active when PTT pressed
- **Optimized for Privacy**: Microphone icon no longer shows in system tray when not recording
- **Production Build Optimization**: Disabled all debug logs in release builds

### Technical

- Native Whisper-rs integration (no Python dependencies)
- GStreamer pipeline: `pulsesrc ! audioconvert ! audioresample ! audio/x-raw,format=F32LE,channels=1,rate=16000 ! appsink`
- Pipeline created on PTT press (~106ms), destroyed on release
- 500ms end padding to prevent Whisper from truncating last words

## [0.1.0] - 2024-12-27

### Added

- **Push-to-Talk Voice Dictation**: Hold Ctrl+Space to record, release to transcribe and auto-type
- **100% Offline STT**: All speech recognition powered locally with Whisper models
- **Gaming Mouse Button Support**: Configure Mouse4, Mouse5, or any mouse button as hotkey
- **Multi-Language Support**: French, English, German, Spanish, and 95+ other languages
- **Multiple Whisper Models**: Tiny (75MB) to Large-v3 (3GB) for quality/speed trade-offs
- **Cross-Platform**: Native support for Linux, Windows, and macOS
- **System Tray Integration**: Runs quietly in the background
- **Modern Dark UI**: Clean interface with real-time status indicators
- **Auto-Type Output**: Text automatically injected into the focused application
- **Configurable Hotkeys**: Support for keyboard keys and mouse buttons with modifiers (Ctrl, Alt, Shift)

### Platform-Specific Features

#### Linux
- **Wayland Support**: Full compatibility via evdev for push-to-talk
- **PipeWire/PulseAudio**: Automatic audio device detection
- **GNOME Extension**: Focused window detection for better text injection

#### Windows
- Native Windows audio capture via WASAPI
- Global hotkey support via Windows API

#### macOS
- CoreAudio integration
- Universal binary support (Intel + Apple Silicon)

### Technical

- **Tauri 2.0**: Modern Rust + React/TypeScript architecture
- **Zero Telemetry**: No data collection, no cloud dependencies
- **Automatic Model Download**: Models downloaded on first use from HuggingFace

### Security

- All processing happens locally on your machine
- No internet connection required for core functionality
- Audio is processed in memory and never saved to disk
- Open source - audit the code yourself
