# 🎤 KaraokeTyper

**Offline voice typing for Ubuntu with push-to-talk**

KaraokeTyper is a 100% local, open-source voice dictation application for Linux that respects your privacy. Unlike cloud-based solutions, all speech recognition happens on your machine.

## ✨ Features

- 🔒 **Completely Offline** - No data sent to cloud servers
- ⚡ **Push-to-Talk** - Hold Ctrl+Space to record, release to type
- 🎯 **Wayland Native** - Full support for modern Linux desktops
- 💨 **Low Latency** - < 500ms from speech to text
- 🌍 **Multi-Language** - Support for English, French, Spanish, German (via Vosk models)
- 📜 **History** - Search and reuse previous dictations
- 🎨 **Modern UI** - GTK4 with libadwaita

## 🚀 Quick Start

### Prerequisites

- Ubuntu 24.04+ (or compatible Linux distribution)
- Python 3.12+
- Wayland session (default on Ubuntu GNOME)

### Installation

```bash
# Clone repository
git clone https://github.com/yourusername/karaoketyper.git
cd karaoketyper

# Run installation script
./install.sh
```

The installer will:
1. Install system dependencies (ydotool, audio libraries)
2. Set up Python virtual environment
3. Download Vosk speech model (~300MB)
4. Configure ydotoold systemd service
5. Create desktop launcher

### Manual Installation

See [INSTALLATION.md](INSTALLATION.md) for detailed manual installation steps.

## 📖 Usage

### Basic Dictation

1. **Start KaraokeTyper** - Launch from application menu or run `karaoketyper`
2. **Click on any text field** - Position cursor where you want text
3. **Hold Ctrl+Space** - Start speaking (microphone indicator turns red)
4. **Release Ctrl+Space** - Text is automatically typed

### Keyboard Shortcuts

- `Ctrl+Space` - Push-to-talk (configurable)
- `Ctrl+H` - Show/hide main window
- `Ctrl+Q` - Quit application

### Voice Commands (Coming in v1.1)

- "rewrite: [text]" - Improve and rewrite text
- "reply: [context]" - Generate reply to clipboard content
- "punctuate: [text]" - Add smart punctuation

## 🏗️ Architecture

```
karaoketyper/
├── core/           # Core functionality
│   ├── audio_capture.py    # Microphone recording
│   ├── stt_engine.py       # Vosk speech recognition
│   ├── text_output.py      # ydotool typing
│   └── command_parser.py   # Voice command detection
├── ui/             # GTK4 interface
│   ├── main_window.py      # Main application window
│   ├── settings_dialog.py  # Preferences
│   └── history_view.py     # Dictation history
├── services/       # Background services
│   ├── shortcut_listener.py  # Global hotkeys
│   └── state_manager.py      # Application state
└── data/           # Data layer
    ├── database.py         # SQLite interface
    └── models.py           # Data models
```

## 🔧 Configuration

Configuration file: `~/.config/karaoketyper/config.json`

```json
{
  "model": "vosk-model-en",
  "shortcut": "Ctrl+Space",
  "key_delay_ms": 8,
  "auto_punctuation": true,
  "language": "en-US"
}
```

## 📊 Supported Languages

Download additional models from [Vosk Models](https://alphacephei.com/vosk/models):

- **English** (en-US) - Default, ~300MB
- **French** (fr-FR) - ~300MB
- **Spanish** (es-ES) - ~300MB
- **German** (de-DE) - ~300MB

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Setup

```bash
# Clone repository
git clone https://github.com/yourusername/karaoketyper.git
cd karaoketyper

# Create virtual environment
python3 -m venv venv
source venv/bin/activate

# Install dependencies
pip install -r requirements.txt
pip install -r requirements-dev.txt

# Run tests
pytest

# Run application
python -m karaoketyper
```

## 📝 Comparison

| Feature | VibeTyper | nerd-dictation | **KaraokeTyper** |
|---------|-----------|----------------|------------------|
| Offline | ❌ Cloud | ✅ Yes | ✅ Yes |
| GUI | ✅ Electron | ❌ CLI | ✅ GTK4 |
| Push-to-talk | ✅ Yes | ❌ Toggle | ✅ Yes |
| History | ✅ Yes | ❌ No | ✅ Yes |
| Open Source | ❌ No | ✅ GPL | ✅ GPL v3 |
| Size | ~200MB | ~50MB | **~80MB** |

## 🔒 Privacy

KaraokeTyper is designed with privacy as a core principle:

- **Zero telemetry** - No analytics, no tracking
- **Local processing** - Speech recognition happens on your machine
- **No network** - Works completely offline
- **Open source** - Audit the code yourself

## 📜 License

GPL-3.0 License - see [LICENSE](LICENSE) for details.

## 🙏 Acknowledgments

- [Vosk](https://alphacephei.com/vosk/) - Offline speech recognition
- [nerd-dictation](https://github.com/ideasman42/nerd-dictation) - Inspiration for Linux voice typing
- [ydotool](https://github.com/ReimuNotMoe/ydotool) - Wayland input injection
- AlterFlow.ai - Original guide for offline voice typing

## 🐛 Support

- **Issues**: [GitHub Issues](https://github.com/yourusername/karaoketyper/issues)
- **Discussions**: [GitHub Discussions](https://github.com/yourusername/karaoketyper/discussions)

## 🗺️ Roadmap

- [x] Core dictation functionality
- [ ] Voice commands (rewrite, reply)
- [ ] Multi-language support
- [ ] Whisper integration (alternative STT)
- [ ] Custom vocabulary
- [ ] Noise cancellation
- [ ] VSCode/IntelliJ plugins

---

Made with ❤️ for the Linux community
