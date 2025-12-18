# 🎤 KaraokeTyper

**Offline voice dictation application for Linux** with push-to-talk functionality using Whisper AI.

All speech recognition happens locally using [Whisper.cpp](https://github.com/ggerganov/whisper.cpp), with **no cloud dependencies**.

## ✨ Features

- 🔴 **Push-to-talk** dictation with Ctrl+Space hotkey
- 🤖 **100% offline** - All processing happens locally using Whisper
- ⚡ **Standalone app** - No system dependencies to install
- 🎯 **Auto-typing** - Text automatically injected into active window
- 🌐 **Cross-platform** - Built with Tauri (Rust + React)
- 🎨 **Modern UI** - Dark theme with real-time status indicators

## 🚀 Quick Start

### Prerequisites

- Node.js 18+ and npm
- Rust toolchain (will be installed automatically if missing)

### Setup

1. **Clone the repository**
   ```bash
   git clone <repo-url>
   cd KaraokeTyper
   ```

2. **Install dependencies**
   ```bash
   npm install
   ```

3. **Download Whisper model** (~150MB)
   ```bash
   ./download-model.sh
   ```

4. **Run in development mode**
   ```bash
   npm run tauri dev
   ```

## 🔨 Building

### Development Build
```bash
npm run tauri dev
```

### Production Build
```bash
npm run tauri build
```

This creates a standalone executable in `src-tauri/target/release/bundle/`:
- **AppImage** - Single file, no installation needed
- **deb** - Debian/Ubuntu package

## 📖 Usage

1. **Launch the app** - KaraokeTyper window will appear
2. **Wait for initialization** - Status will show "Ready! Press Ctrl+Space to dictate"
3. **Click in target window** - Focus any text editor or application
4. **Press and hold Ctrl+Space** - Speak clearly into your microphone
5. **Release Ctrl+Space** - Text will be processed and typed automatically

### Hotkeys

- `Ctrl+Space` - Start/stop recording (global hotkey)
- Manual button - Click "Start Recording" button as alternative

### Status Indicators

- 🟢 **Green** - Idle, ready to record
- 🔴 **Red** - Recording audio (pulsing)
- 🟠 **Orange** - Processing speech-to-text

## 🏗️ Architecture

### Technology Stack

- **Frontend**: React 18 + TypeScript + Vite
- **Backend**: Rust + Tauri v2
- **Speech Recognition**: whisper-rs (Whisper.cpp bindings)
- **Audio Capture**: cpal (cross-platform audio library)
- **Text Injection**: enigo (keyboard simulation)
- **Hotkeys**: tauri-plugin-global-shortcut

### Project Structure

```
karaoketyper/
├── src/                    # React frontend
│   ├── App.tsx            # Main UI component
│   ├── App.css            # Dark theme styling
│   └── main.tsx           # React entry point
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── audio.rs       # Audio capture with cpal
│   │   ├── stt.rs         # Whisper speech recognition
│   │   ├── output.rs      # Keyboard text injection
│   │   └── lib.rs         # Tauri commands & state
│   ├── models/            # Whisper model storage
│   │   └── ggml-base.bin  # Base model (~150MB)
│   ├── Cargo.toml         # Rust dependencies
│   └── tauri.conf.json    # Tauri configuration
└── old/                   # Original Python implementation

```

### Workflow

1. **Initialization** (`init_stt`)
   - Loads Whisper model from `models/ggml-base.bin`
   - Creates STT engine with English language settings

2. **Recording** (`start_recording`)
   - Captures audio at 16kHz mono (Whisper requirement)
   - Buffers audio samples in real-time using cpal
   - Emits "recording" status event to UI

3. **Processing** (`stop_recording`)
   - Stops audio capture
   - Passes audio buffer to Whisper engine
   - Transcribes speech to text
   - Returns text to frontend

4. **Output** (`type_text`)
   - Simulates keyboard input using enigo
   - Types text character-by-character with 8ms delay
   - Works in any focused application

## 🔧 Configuration

### Whisper Model

The default model is **Whisper Base** (~150MB):
- Good balance of speed and accuracy
- Runs efficiently on most systems
- English language optimized

To use a different model:
1. Download from [Hugging Face](https://huggingface.co/ggerganov/whisper.cpp/tree/main)
2. Place in `src-tauri/models/`
3. Update model path in `lib.rs:35`

Available models:
- `ggml-tiny.bin` - Fastest, lower accuracy (~75MB)
- `ggml-base.bin` - Default, good balance (~150MB)
- `ggml-small.bin` - Better accuracy, slower (~500MB)
- `ggml-medium.bin` - High accuracy (~1.5GB)

### Keyboard Delay

Default keystroke delay is 8ms. To change:
Edit `src-tauri/src/lib.rs:121`:
```rust
let mut output = TextOutput::new(8)?; // Change to your preferred delay
```

### Global Hotkey

Default is `Ctrl+Space`. To change:
Edit `src/App.tsx:43`:
```typescript
const shortcut = "CommandOrControl+Space"; // Change to your preferred hotkey
```

## 🐛 Troubleshooting

### "Whisper model not found"
Run `./download-model.sh` to download the model.

### No audio captured
- Check microphone permissions
- Verify default input device is correct
- Test microphone in other apps first

### Text not typing
- Ensure target window is focused
- Check if application blocks simulated input (some games/secure apps do)
- Try increasing keystroke delay if characters are missing

### Build errors
```bash
# Clear cache and rebuild
rm -rf node_modules package-lock.json
npm install
npm run tauri build
```

## 🔒 Privacy

**KaraokeTyper is completely offline:**
- ✅ All speech recognition happens locally
- ✅ No telemetry or analytics
- ✅ No network connections required
- ✅ Audio is never stored or transmitted
- ✅ Whisper model runs on your CPU/GPU only

## 📝 Development

### Adding new features

1. **Rust backend** - Edit files in `src-tauri/src/`
2. **React frontend** - Edit files in `src/`
3. **Tauri commands** - Add to `lib.rs` and update `invoke_handler![]`

### Testing

```bash
# Run development build with console output
npm run tauri dev

# Check Rust compilation
cd src-tauri
cargo check

# Format Rust code
cargo fmt

# Lint Rust code
cargo clippy
```

## 📄 License

[Your license here]

## 🙏 Credits

- [Whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Fast C++ implementation of OpenAI Whisper
- [Tauri](https://tauri.app) - Build smaller, faster desktop apps
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio I/O
- [enigo](https://github.com/enigo-rs/enigo) - Cross-platform input simulation

## 🗺️ Roadmap

- [ ] Multi-language support (Spanish, French, German, etc.)
- [ ] Voice commands ("new paragraph", "delete that")
- [ ] Dictation history with local SQLite storage
- [ ] Custom vocabulary and auto-corrections
- [ ] System tray integration
- [ ] Pause/resume during recording
- [ ] Audio level meter
- [ ] Configurable settings UI

---

**Built with ❤️ for the Linux community**
