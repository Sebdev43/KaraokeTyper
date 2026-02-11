# Changelog

All notable changes to KaraokeTyper will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.4] - 2025-02-11

### Fixed

- **Select dropdown black rectangle**: Removed Radix UI Portal from Select component to fix WebKitGTK rendering issue on Linux (opaque black box instead of dropdown)

## [1.0.3] - 2025-02-11

### Fixed

- **On-demand microphone**: Mic opens only during PTT press, fully released on release (no permanent system mic indicator)
- **Whisper.cpp log flood**: Suppressed verbose beam search output via whisper_log_set
- **Ghostty terminal paste**: Reverse-domain WM_CLASS support (com.mitchellh.ghostty → ghostty)
- **Terminal detection reliability**: Moved window classification to Rust side (no stale GNOME extension JS cache)

## [1.0.2] - 2025-02-11

### Changed

- **Audio: CPAL Migration**: Replaced GStreamer with CPAL for cross-platform audio capture (ALSA on Linux, CoreAudio on macOS, WASAPI on Windows)
- **Instant Push-to-Talk**: Stream stays open, play()/pause() toggles recording (~0ms latency vs ~2s before)
- **Better Device Listing**: Hardware devices via /proc/asound/cards + lsusb for readable USB names, Bluetooth via PipeWire
- **Removed GStreamer Dependency**: No more gstreamer packages needed on Linux

### Fixed

- **VS Code Terminal Paste**: GNOME extension now detects VS Code/Codium/Cursor integrated terminal via window title, uses Ctrl+Shift+V for terminal pane
- **Overlay Transparency**: Fixed black square overlay issue with proper RGBA background

### Removed

- GStreamer dependencies (gstreamer, gstreamer-app, gstreamer-audio)
- Unused shell scripts (download-model.sh, install-deps.sh)
- Legacy audio_cli.rs (CLI-based audio capture)

## [1.0.1] - 2025-01-08

### Fixed

- **Audio Clipping at Start**: Added 200ms silence padding at beginning of audio to prevent first words from being cut off (e.g., "c'est l'" no longer missing)
- Audio pipeline startup delay (~106ms) compensated with start padding

## [1.0.0] - 2024-12-30

### Changed

- **Complete STT Engine Rewrite**: Switched from Parakeet to native Whisper-rs for better performance and reliability
- **On-Demand Audio Capture**: Mic only active when PTT pressed (always_on_microphone=false)
- **Optimized for Privacy**: Microphone icon no longer shows in system tray when not recording
- **Production Build Optimization**: Disabled all debug logs in release builds

### Technical

- Native Whisper-rs integration (no Python dependencies)
- Cross-platform audio capture via CPAL (ALSA, CoreAudio, WASAPI)
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
