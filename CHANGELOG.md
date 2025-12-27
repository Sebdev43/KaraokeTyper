# Changelog

All notable changes to KaraokeTyper will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-12-27

### Added

- **Push-to-Talk Voice Dictation**: Hold Ctrl+Space to record, release to transcribe and auto-type
- **100% Offline STT**: All speech recognition powered by sherpa-onnx with Whisper models
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
- **WirePlumber Latency Fix**: Optional configuration for reduced audio latency
- **GNOME Extension**: Focused window detection for better text injection

#### Windows
- Native Windows audio capture via WASAPI
- Global hotkey support via Windows API

#### macOS
- CoreAudio integration
- Universal binary support (Intel + Apple Silicon)

### Technical

- **Tauri 2.0**: Modern Rust + React/TypeScript architecture
- **sherpa-onnx Integration**: High-performance ONNX Runtime for STT
- **Zero Telemetry**: No data collection, no cloud dependencies
- **Automatic Model Download**: Models downloaded on first use from HuggingFace

### Security

- All processing happens locally on your machine
- No internet connection required for core functionality
- Audio is processed in memory and never saved to disk
- Open source - audit the code yourself

---

## Release Notes

### v0.1.0 - Initial Release

This is the first public release of KaraokeTyper, featuring:

- Complete offline voice dictation workflow
- Support for all major desktop platforms
- Gaming mouse button hotkey support
- Multiple Whisper model sizes for different use cases

**Known Limitations:**
- macOS code signing not yet configured (may require Gatekeeper bypass)
- Windows SmartScreen may warn on first run (not yet signed)
- Some Wayland compositors may have hotkey limitations
