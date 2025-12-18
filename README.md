<p align="center">
  <img src="public/tauri.svg" alt="KaraokeTyper Logo" width="120" height="120">
</p>

<h1 align="center">KaraokeTyper</h1>

<p align="center">
  <strong>Offline voice dictation with push-to-talk</strong>
</p>

<p align="center">
  <a href="#features">Features</a> •
  <a href="#installation">Installation</a> •
  <a href="#usage">Usage</a> •
  <a href="#building">Building</a> •
  <a href="#contributing">Contributing</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.0-blue.svg" alt="Version">
  <img src="https://img.shields.io/badge/license-GPL--3.0-green.svg" alt="License">
  <img src="https://img.shields.io/badge/platform-Linux%20|%20Windows%20|%20macOS-lightgrey.svg" alt="Platform">
  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/tauri-2.0-blueviolet.svg" alt="Tauri">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs Welcome">
  <img src="https://img.shields.io/badge/AI-Canary--1B-ff6b6b.svg" alt="AI Model">
</p>

---

## Overview

**KaraokeTyper** is a privacy-first, cross-platform voice dictation application. All speech recognition happens **100% offline** using NVIDIA's Canary-1B model - no cloud, no telemetry, no data leaving your machine.

Press `Ctrl+Space`, speak, release - your words appear instantly in any application.

---

## Features

| Feature | Description |
|---------|-------------|
| **Push-to-Talk** | Hold `Ctrl+Space` to record, release to transcribe |
| **100% Offline** | All AI processing happens locally on your machine |
| **Cross-Platform** | Native support for Linux, Windows, and macOS |
| **Privacy-First** | Zero telemetry, zero cloud, zero data collection |
| **Fast Transcription** | Canary-1B delivers high-accuracy results in seconds |
| **Auto-Type** | Text automatically injected into focused application |
| **Multi-Language** | Support for French, English, German, Spanish (configurable) |
| **Modern UI** | Clean dark theme with real-time status indicators |
| **System Tray** | Runs quietly in the background |

---

## Screenshots

<p align="center">
  <em>Recording overlay with visual feedback</em>
</p>

| Recording | Processing | Ready |
|:---------:|:----------:|:-----:|
| 🔴 Red dot + equalizer | 🟠 Spinner | 🟢 Green dot |

---

## Installation

### Prerequisites

- **Node.js** 18+ and npm
- **Rust** 1.70+ (installed automatically)
- **Python** 3.8+ (for Canary AI model)

### Quick Start

```bash
# Clone the repository
git clone https://github.com/Sebdev43/KaraokeTyper.git
cd KaraokeTyper

# Install dependencies
npm install

# Run in development mode
npm run tauri dev
```

### First Launch

1. Open the **Settings** window (F10 or system tray)
2. Install Python dependencies (one-click button)
3. Download the Canary-1B model (~2GB)
4. Select your microphone
5. Ready to dictate!

---

## Usage

### Basic Workflow

1. **Focus** any text field (browser, editor, terminal...)
2. **Hold** `Ctrl+Space` and speak clearly
3. **Release** to transcribe and auto-type

### Hotkeys

| Shortcut | Action |
|----------|--------|
| `Ctrl+Space` | Push-to-talk (hold to record) |
| `F10` | Open settings window |

### Status Indicators

| Color | State |
|-------|-------|
| 🟢 Green | Ready - waiting for input |
| 🔴 Red | Recording - speak now |
| 🟠 Orange | Processing - transcribing audio |

---

## Building

### Development

```bash
npm run tauri dev
```

### Production

```bash
npm run tauri build
```

**Output formats:**
- **Linux**: AppImage, .deb
- **Windows**: .msi, .exe
- **macOS**: .dmg, .app

---

## Architecture

```
KaraokeTyper/
├── src/                    # React/TypeScript frontend
│   ├── components/         # UI components (shadcn/ui)
│   └── windows/            # Window-specific views
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── lib.rs          # Tauri commands & app setup
│   │   ├── audio.rs        # Audio capture (cpal)
│   │   ├── stt_canary.rs   # Canary AI integration
│   │   ├── output.rs       # Text injection (enigo/uinput)
│   │   └── keyboard_listener.rs  # Push-to-talk handler
│   └── python/
│       └── canary_service.py     # Python AI service
└── documentation/          # Technical docs
```

### Tech Stack

| Layer | Technology |
|-------|------------|
| **Frontend** | React 18, TypeScript, Tailwind CSS, shadcn/ui |
| **Backend** | Rust, Tauri 2.0 |
| **AI Model** | NVIDIA Canary-1B (NeMo toolkit) |
| **Audio** | cpal (cross-platform) |
| **Keyboard** | rdev (Windows/macOS), evdev (Linux) |
| **Text Output** | enigo, arboard, uinput |

---

## Privacy & Security

**KaraokeTyper is designed with privacy as a core principle:**

- All speech recognition runs **locally** on your hardware
- **No internet connection** required for core functionality
- **No telemetry**, analytics, or tracking of any kind
- **No cloud services** - your voice data never leaves your machine
- Audio is processed in memory and **never saved** to disk
- Open source - audit the code yourself

---

## Configuration

Settings are stored in:
- **Linux**: `~/.config/karaoketyper/settings.json`
- **Windows**: `%APPDATA%\karaoketyper\settings.json`
- **macOS**: `~/Library/Application Support/karaoketyper/settings.json`

### Available Options

| Setting | Description | Default |
|---------|-------------|---------|
| `shortcut` | Push-to-talk hotkey | `Ctrl+Space` |
| `language` | Recognition language | `fr` |
| `model` | AI model name | `canary-1b-flash` |
| `audio_device_name` | Microphone device | System default |

---

## Contributing

We welcome contributions! Please read our guidelines before submitting.

### Branch Protection

This repository uses strict branch protection:

- **`main`** - Production branch (protected)
  - Requires pull request with review
  - Requires status checks to pass
  - No direct pushes allowed

- **`dev`** - Development branch (protected)
  - Requires pull request
  - Feature branches merge here first

### Workflow

1. **Fork** the repository
2. Create a **feature branch** from `dev`
3. Make your changes
4. Run tests and linting:
   ```bash
   cd src-tauri && cargo clippy && cargo test
   npx biome check --write src/
   ```
5. Submit a **Pull Request** to `dev`

### Code Style

- **Rust**: `cargo fmt` + `cargo clippy` (zero warnings)
- **TypeScript**: Biome linter (`npx biome check --write`)
- **UI Components**: shadcn/ui only (no custom components)
- **Styling**: Tailwind CSS only (no inline styles)

---

## Troubleshooting

<details>
<summary><strong>Push-to-talk not working</strong></summary>

**Linux**: Add your user to the `input` group:
```bash
sudo usermod -aG input $USER
# Log out and back in
```

**Wayland**: Install required tools:
```bash
sudo apt install wtype ydotool
```
</details>

<details>
<summary><strong>No audio captured</strong></summary>

1. Check Settings > Audio Device
2. Ensure microphone permissions are granted
3. Test microphone in another app first
</details>

<details>
<summary><strong>Model download fails</strong></summary>

Manual download:
```bash
pip install huggingface_hub
huggingface-cli download nvidia/canary-1b-flash
```
</details>

<details>
<summary><strong>Text not appearing</strong></summary>

- Ensure target window is focused before releasing hotkey
- Some applications block simulated input (games, secure apps)
- Try clipboard mode in settings
</details>

---

## Roadmap

- [x] Cross-platform support (Linux, Windows, macOS)
- [x] Canary-1B AI integration
- [x] Push-to-talk with global hotkey
- [x] System tray integration
- [ ] Voice commands ("new paragraph", "delete that")
- [ ] Custom vocabulary and corrections
- [ ] Real-time transcription preview
- [ ] Multiple language models
- [ ] Audio level visualization

---

## License

This project is licensed under the **GNU General Public License v3.0** - see the [LICENSE](LICENSE) file for details.

**Key points:**
- You can use, modify, and distribute this software
- Derivative works must also be open source under GPL-3.0
- Commercial use is allowed but must comply with GPL terms
- No warranty is provided

---

## Credits

- [NVIDIA NeMo](https://github.com/NVIDIA/NeMo) - Canary-1B speech recognition model
- [Tauri](https://tauri.app) - Cross-platform app framework
- [shadcn/ui](https://ui.shadcn.com) - Beautiful UI components
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio I/O
- [rdev](https://github.com/Narsil/rdev) - Cross-platform input events
- [enigo](https://github.com/enigo-rs/enigo) - Cross-platform input simulation

---

<p align="center">
  <strong>Built with care for privacy-conscious users</strong>
</p>

<p align="center">
  <a href="https://github.com/Sebdev43/KaraokeTyper/issues">Report Bug</a> •
  <a href="https://github.com/Sebdev43/KaraokeTyper/issues">Request Feature</a>
</p>
