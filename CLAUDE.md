# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

KaraokeTyper is an **offline, cross-platform** voice dictation application with push-to-talk functionality. All speech recognition happens locally using Whisper, with no cloud dependencies.

**Target Platforms**: Linux, Windows, macOS
**Current Status**: v0.1.0 - Active development
**Stack**: Tauri 2.0 (Rust backend + React/TypeScript frontend)

---

## 🚨 CRITICAL: Code Quality & UI Rules (MUST FOLLOW)

### Rule #0: ALWAYS Run Biome After EVERY Code Modification

**MANDATORY after EVERY file modification in TypeScript/React:**

```bash
npx biome check --write <file-path>
```

**Process:**
1. Modify TypeScript/React file
2. **IMMEDIATELY** run `npx biome check --write` on that file
3. Fix ALL errors and warnings reported by Biome
4. Verify the file passes linting before moving on

**No exceptions. This is non-negotiable.**

### Rule #0.1: ALWAYS Use shadcn/ui Components

**NEVER create custom UI components when shadcn/ui provides them.**

❌ **FORBIDDEN:**
```tsx
<button className="my-custom-button">Click</button>
<select className="my-select">...</select>
<input type="text" />
```

✅ **REQUIRED:**
```tsx
import { Button } from "@/components/ui/button"
import { Select } from "@/components/ui/select"
import { Input } from "@/components/ui/input"

<Button variant="default">Click</Button>
<Select>...</Select>
<Input />
```

**Available shadcn/ui components:**
- `Button`, `Card`, `Select`, `Input`, `Label`, `Switch`, `Badge`, `Separator`, `ScrollArea`, `Dialog`, `Toast`, `Form`
- See: https://ui.shadcn.com/docs/components

**Adding new components:**
```bash
npx shadcn@latest add <component-name>
```

### Rule #0.2: ALWAYS Use Tailwind CSS Classes

**NEVER use inline styles or custom CSS files.**

❌ **FORBIDDEN:**
```tsx
<div style={{ padding: "12px", background: "#1a1a1a" }}>
<div className="my-custom-class">
```

✅ **REQUIRED:**
```tsx
<div className="p-3 bg-zinc-900">
```

**Use the `cn()` utility for conditional classes:**
```tsx
import { cn } from "@/lib/utils"

<Button className={cn(
  "base-classes",
  isActive && "active-classes"
)}>
```

### Rule #0.3: ABSOLUTELY NO MANUAL INSTALLATIONS

**🚨 RÈGLE ABSOLUE - AUCUNE EXCEPTION 🚨**

**TOUTES les installations de dépendances DOIVENT se faire via l'interface utilisateur de l'application.**

❌ **STRICTEMENT INTERDIT:**
```bash
# JAMAIS exécuter ces commandes manuellement
pip install sherpa-onnx
pip install anything
apt install something
brew install something
npm install -g something
```

✅ **OBLIGATOIRE:**
- Toute installation doit passer par un bouton dans l'UI Settings
- Le backend Rust gère l'installation via `Command::new()`
- L'utilisateur ne doit JAMAIS ouvrir un terminal pour installer quoi que ce soit
- Afficher une barre de progression pendant l'installation
- Émettre des événements Tauri pour le feedback utilisateur

**Implémentation Pattern:**
```rust
#[tauri::command]
async fn install_dependencies(app_handle: AppHandle) -> Result<String, String> {
    // 1. Émettre événement "installation démarrée"
    app_handle.emit("install-progress", json!({"status": "installing", "message": "..."})).ok();

    // 2. Créer venv si nécessaire
    // 3. Installer via pip dans le venv
    // 4. Émettre événement "installation terminée"

    Ok("Installation réussie".to_string())
}
```

**Cette règle est NON-NÉGOCIABLE.**

### Rule #0.4: NEVER MODIFY THE LINUX SYSTEM

**🚨 INTERDICTION ABSOLUE - AUCUNE EXCEPTION 🚨**

**Tu n'as JAMAIS le droit de modifier le système Linux de l'utilisateur.**

❌ **STRICTEMENT INTERDIT:**
```bash
# JAMAIS exécuter ces commandes
systemctl --user restart wireplumber
systemctl --user restart pipewire
systemctl restart anything

# JAMAIS modifier ces fichiers/dossiers
~/.config/wireplumber/*
~/.config/pipewire/*
/etc/systemd/*
/etc/pulse/*
/etc/pipewire/*

# JAMAIS toucher à la configuration système
gsettings set ...
dconf write ...
```

❌ **JAMAIS modifier:**
- WirePlumber / PipeWire / PulseAudio
- systemd services (user ou system)
- Configuration GNOME/KDE/desktop
- Fichiers système (/etc/*, /usr/*, etc.)
- Variables d'environnement système

✅ **CE QUI EST AUTORISÉ:**
- Modifier UNIQUEMENT le code source de KaraokeTyper
- Lire les fichiers système pour diagnostic (lecture seule)
- Exécuter des commandes de build (cargo, npm, etc.)
- Exécuter l'application elle-même

**Si un problème nécessite une modification système, INFORMER l'utilisateur et lui donner les instructions à exécuter lui-même.**

**Cette règle est NON-NÉGOCIABLE. Toute violation est INACCEPTABLE.**

---

## CRITICAL: Cross-Platform Development Rules

### Rule #1: NO Platform-Specific Code Without Abstraction

**NEVER** write code that only works on one platform. Every feature MUST work on:
- Linux (X11 + Wayland)
- Windows 10/11
- macOS (Intel + Apple Silicon)

**Bad Example:**
```rust
// ❌ FORBIDDEN - Linux only
use evdev::{Device, InputEventKind};
std::process::Command::new("ydotool").arg("type").arg(text);
```

**Good Example:**
```rust
// ✅ CORRECT - Cross-platform abstraction
#[cfg(target_os = "linux")]
mod linux_impl { /* Linux-specific code */ }

#[cfg(target_os = "windows")]
mod windows_impl { /* Windows-specific code */ }

#[cfg(target_os = "macos")]
mod macos_impl { /* macOS-specific code */ }

pub fn platform_function() -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    return linux_impl::function();
    #[cfg(target_os = "windows")]
    return windows_impl::function();
    #[cfg(target_os = "macos")]
    return macos_impl::function();
}
```

### Rule #2: Use Cross-Platform Libraries

| Feature | Approved Library | Forbidden |
|---------|-----------------|-----------|
| Audio Capture | `cpal` | portaudio, platform-specific APIs |
| Text Injection | `enigo` | ydotool, xdotool, wtype |
| Clipboard | `arboard` | xclip, wl-copy, platform commands |
| Global Hotkeys | `rdev` or Tauri plugin | evdev, platform-specific hooks |
| File Paths | `dirs` crate | Hardcoded paths |
| Async Runtime | `tokio` | Platform-specific threading |

### Rule #3: Dependencies Must Auto-Install

**ALL dependencies must be handled automatically at app startup.** The user should NEVER need to:
- Run manual installation commands
- Install system packages separately
- Configure daemons or services

**Implementation Pattern:**
```rust
fn ensure_dependencies() -> Result<(), Error> {
    // 1. Check if dependency exists
    // 2. If missing, install automatically OR bundle with app
    // 3. If cannot install, show clear error with instructions
}
```

### Rule #4: Cargo.toml Platform-Specific Dependencies

```toml
[dependencies]
# Cross-platform dependencies
cpal = "0.17"
enigo = "0.2"
arboard = "3.4"
rdev = "0.5"
dirs = "5.0"

[target.'cfg(target_os = "linux")'.dependencies]
# Linux-only deps (if absolutely necessary)

[target.'cfg(target_os = "windows")'.dependencies]
# Windows-only deps (if absolutely necessary)

[target.'cfg(target_os = "macos")'.dependencies]
# macOS-only deps (if absolutely necessary)
```

### Rule #5: Test on All Platforms

Before any PR merge:
1. Build succeeds on Linux, Windows, macOS
2. Core functionality works on all platforms
3. No platform-specific warnings or errors

---

## Architecture

### Current Structure (Tauri 2.0)

```
karaoketyper/
├── src/                          # React/TypeScript frontend
│   ├── components/              # UI components
│   ├── windows/                 # Window-specific views
│   └── App.tsx                  # Main app
├── src-tauri/                   # Rust backend
│   ├── src/
│   │   ├── main.rs             # Entry point
│   │   ├── lib.rs              # Tauri commands & setup
│   │   ├── audio.rs            # Audio capture (cpal) ✅ Cross-platform
│   │   ├── stt.rs              # Speech-to-text (Whisper) ✅ Cross-platform
│   │   ├── output.rs           # Text injection ⚠️ NEEDS REFACTOR
│   │   ├── config.rs           # Configuration ✅ Cross-platform
│   │   ├── keyboard/           # Keyboard handling (NEW)
│   │   │   ├── mod.rs          # Platform abstraction
│   │   │   ├── linux.rs        # Linux impl (rdev)
│   │   │   ├── windows.rs      # Windows impl (rdev)
│   │   │   └── macos.rs        # macOS impl (rdev)
│   │   └── platforms/          # Platform-specific code (NEW)
│   │       ├── mod.rs
│   │       ├── linux.rs
│   │       ├── windows.rs
│   │       └── macos.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
└── CLAUDE.md
```

### Module Status

| Module | Cross-Platform | Status |
|--------|---------------|--------|
| `audio.rs` | ✅ Yes | Uses cpal |
| `stt.rs` | ✅ Yes | Uses whisper-rs |
| `config.rs` | ✅ Yes | Uses dirs crate |
| `output.rs` | ❌ No | Uses ydotool - MUST REFACTOR to enigo |
| `evdev_listener.rs` | ❌ No | Uses evdev - MUST REFACTOR to rdev |
| `gnome_shortcut.rs` | ❌ No | GNOME only - MUST REMOVE or make optional |

---

## Development Commands

### Environment Setup

```bash
# Prerequisites (install once)
# Linux:
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev

# Windows:
# Install Visual Studio Build Tools + Rust

# macOS:
xcode-select --install
```

### Running the Application

**🚨 CRITICAL: ALWAYS Launch Dev Server as Background Task**

**MANDATORY workflow for testing/debugging:**

```bash
cd karaoketyper

# 1. ALWAYS launch in background to read logs
npm run tauri dev &

# This creates a background task (e.g., task ID: abc1234)
# Output is written to: /tmp/claude/tasks/abc1234.output

# 2. Wait for compilation (10-20 seconds)
sleep 15

# 3. Read logs to check for errors
tail -100 /tmp/claude/tasks/abc1234.output

# 4. If you need to restart, kill the old process first
lsof -ti:1420 | xargs -r kill -9  # Kill Vite dev server
# Then relaunch: npm run tauri dev &
```

**Why background tasks:**
- You can read compilation errors and runtime logs
- You can continue working while app compiles
- You can easily restart the app without blocking the terminal

**Production builds:**
```bash
# Build for production
npm run tauri build

# Build for specific platform
npm run tauri build -- --target x86_64-unknown-linux-gnu
npm run tauri build -- --target x86_64-pc-windows-msvc
npm run tauri build -- --target x86_64-apple-darwin
```

### Code Quality

```bash
# Rust
cd src-tauri
cargo fmt          # Format
cargo clippy       # Lint
cargo test         # Test

# TypeScript/React
npx biome check --write <file>   # Lint and auto-fix (MANDATORY after EVERY modification)
npm run typecheck                # TypeScript type checking
```

---

## Key Components

### 1. Audio Capture (audio.rs) - ✅ Cross-Platform

```rust
// Uses cpal - works on Linux/Windows/macOS
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

// IMPORTANT: Always force 16kHz for Whisper compatibility
let config = cpal::StreamConfig {
    channels: 1,
    sample_rate: cpal::SampleRate(16000),
    buffer_size: cpal::BufferSize::Default,
};
```

### 2. Speech-to-Text (stt.rs) - ✅ Cross-Platform

```rust
// Uses whisper-rs - works everywhere
use whisper_rs::{WhisperContext, WhisperContextParameters};

// Model path using dirs crate (cross-platform)
let model_path = dirs::data_dir()
    .unwrap()
    .join("karaoketyper")
    .join("models")
    .join("ggml-base.bin");
```

### 3. Text Output (output.rs) - ⚠️ NEEDS REFACTOR

**Current (BAD):**
```rust
// ❌ Linux only - ydotool
Command::new("ydotool").arg("type").arg(&text).spawn();
```

**Required (GOOD):**
```rust
// ✅ Cross-platform - enigo
use enigo::{Enigo, Key, KeyboardControllable};

pub fn type_text(text: &str) -> Result<(), String> {
    let mut enigo = Enigo::new();
    enigo.key_sequence(text);
    Ok(())
}

// Alternative: clipboard + paste
pub fn paste_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text)?;

    let mut enigo = Enigo::new();
    #[cfg(target_os = "macos")]
    enigo.key_down(Key::Meta);
    #[cfg(not(target_os = "macos"))]
    enigo.key_down(Key::Control);

    enigo.key_click(Key::Layout('v'));

    #[cfg(target_os = "macos")]
    enigo.key_up(Key::Meta);
    #[cfg(not(target_os = "macos"))]
    enigo.key_up(Key::Control);

    Ok(())
}
```

### 4. Keyboard Listener - ⚠️ NEEDS REFACTOR

**Current (BAD):**
```rust
// ❌ Linux only - evdev
use evdev::{Device, InputEventKind, Key};
```

**Required (GOOD):**
```rust
// ✅ Cross-platform - rdev
use rdev::{listen, Event, EventType, Key};

pub fn start_keyboard_listener<F>(callback: F) -> Result<(), String>
where
    F: Fn(bool) + Send + 'static, // true = pressed, false = released
{
    std::thread::spawn(move || {
        let mut ctrl_pressed = false;

        listen(move |event| {
            match event.event_type {
                EventType::KeyPress(Key::ControlLeft) |
                EventType::KeyPress(Key::ControlRight) => {
                    ctrl_pressed = true;
                }
                EventType::KeyRelease(Key::ControlLeft) |
                EventType::KeyRelease(Key::ControlRight) => {
                    ctrl_pressed = false;
                }
                EventType::KeyPress(Key::Space) if ctrl_pressed => {
                    callback(true); // Recording started
                }
                EventType::KeyRelease(Key::Space) if ctrl_pressed => {
                    callback(false); // Recording stopped
                }
                _ => {}
            }
        }).expect("Failed to listen");
    });
    Ok(())
}
```

---

## Configuration

### Config Location (Cross-Platform via dirs crate)

| Platform | Path |
|----------|------|
| Linux | `~/.config/karaoketyper/settings.json` |
| Windows | `C:\Users\<User>\AppData\Roaming\karaoketyper\settings.json` |
| macOS | `~/Library/Application Support/karaoketyper/settings.json` |

### Config Structure

```json
{
  "shortcut": "CommandOrControl+Space",
  "audio_device_index": 0,
  "audio_device_name": "USB Device 0x46d:0x825",
  "model": "base",
  "key_delay_ms": 8,
  "language": "fr"
}
```

---

## Bundling & Distribution

### tauri.conf.json Configuration

```json
{
  "bundle": {
    "active": true,
    "targets": ["deb", "appimage", "msi", "dmg"],
    "identifier": "com.karaoketyper.app",
    "resources": ["models/*"],
    "linux": {
      "deb": {
        "depends": ["libwebkit2gtk-4.1-0", "libgtk-3-0", "libasound2"]
      }
    }
  }
}
```

### Model Bundling

Whisper models should be:
1. Downloaded on first launch if missing
2. Stored in platform-appropriate data directory
3. NOT bundled with the app (too large)

```rust
async fn ensure_model() -> Result<PathBuf, Error> {
    let model_dir = dirs::data_dir()
        .unwrap()
        .join("karaoketyper")
        .join("models");

    let model_path = model_dir.join("ggml-base.bin");

    if !model_path.exists() {
        // Download model automatically
        download_model(&model_path).await?;
    }

    Ok(model_path)
}
```

---

## Privacy & Offline Operation

**Critical Design Principle**: Zero network dependencies for core functionality.

- All STT processing happens locally via Whisper
- No telemetry or analytics
- No cloud services required
- Models downloaded once, cached locally

---

## Forbidden Patterns

### ❌ NEVER DO:

1. **Hardcoded Linux commands**
   ```rust
   Command::new("ydotool")  // ❌
   Command::new("xdotool")  // ❌
   Command::new("wtype")    // ❌
   ```

2. **Linux-specific paths**
   ```rust
   "/dev/input/event0"      // ❌
   "/tmp/something"         // ❌
   "~/.config/..."          // ❌ Use dirs crate instead
   ```

3. **Platform assumptions**
   ```rust
   std::env::var("XDG_SESSION_TYPE")  // ❌ Linux only
   ```

4. **External daemon requirements**
   ```rust
   // ❌ Requires user to run ydotoold
   // ❌ Requires user to install packages
   // ❌ Requires user to configure permissions
   ```

### ✅ ALWAYS DO:

1. **Use cross-platform crates**
2. **Implement platform abstraction**
3. **Test on all platforms**
4. **Bundle or auto-download dependencies**
5. **Use dirs crate for paths**

---

## Migration Checklist

### Phase 1: Text Output (HIGH PRIORITY)
- [ ] Replace ydotool with enigo
- [ ] Implement clipboard + paste fallback
- [ ] Test on Linux, Windows, macOS

### Phase 2: Keyboard Handling (HIGH PRIORITY)
- [ ] Replace evdev with rdev
- [ ] Remove evdev_listener.rs
- [ ] Create cross-platform keyboard module
- [ ] Test push-to-talk on all platforms

### Phase 3: Cleanup (MEDIUM PRIORITY)
- [ ] Remove gnome_shortcut.rs (use Tauri plugin)
- [ ] Remove dbus_service.rs (use Tauri IPC)
- [ ] Update Cargo.toml dependencies
- [ ] Remove all platform-specific code from lib.rs

### Phase 4: Distribution (LOW PRIORITY)
- [ ] Configure GitHub Actions for multi-platform builds
- [ ] Set up auto-updater
- [ ] Create installers for all platforms

---

## Frontend UI Framework

### shadcn/ui + Tailwind CSS

**Official Repository**: https://github.com/shadcn-ui/ui
**Documentation**: https://ui.shadcn.com/

KaraokeTyper uses **shadcn/ui** for all UI components with **Tailwind CSS** for styling.

### UI Development Rules

#### Rule #1: Use shadcn/ui Components

**ALWAYS** use shadcn/ui components instead of custom HTML elements:

```tsx
// ❌ FORBIDDEN - Raw HTML
<button className="my-button">Click</button>
<select className="my-select">...</select>

// ✅ CORRECT - shadcn/ui components
import { Button } from "@/components/ui/button"
import { Select } from "@/components/ui/select"

<Button variant="default">Click</Button>
<Select>...</Select>
```

#### Rule #2: Use Tailwind CSS Classes

**NEVER** use inline styles or custom CSS files:

```tsx
// ❌ FORBIDDEN - Inline styles
<div style={{ padding: "12px", background: "#1a1a1a" }}>

// ❌ FORBIDDEN - Custom CSS classes
<div className="my-custom-class">

// ✅ CORRECT - Tailwind classes
<div className="p-3 bg-zinc-900">
```

#### Rule #3: Use the cn() Utility

For conditional class merging, use the `cn()` utility:

```tsx
import { cn } from "@/lib/utils"

<Button className={cn(
  "base-classes",
  isActive && "active-classes",
  className
)}>
```

#### Rule #4: Component Structure

```
karaoketyper/src/
├── components/
│   └── ui/           # shadcn/ui components (DO NOT EDIT)
│       ├── button.tsx
│       ├── card.tsx
│       ├── select.tsx
│       └── ...
├── lib/
│   └── utils.ts      # cn() utility function
└── styles/
    └── globals.css   # Tailwind + CSS variables
```

#### Rule #5: Adding New Components

```bash
# Add a new shadcn/ui component
npx shadcn@latest add <component-name>

# Examples:
npx shadcn@latest add dialog
npx shadcn@latest add toast
npx shadcn@latest add form
```

### Available Components

| Component | Usage |
|-----------|-------|
| `Button` | All clickable actions |
| `Card` | Section containers |
| `Select` | Dropdown menus (audio devices, models, etc.) |
| `Input` | Text input fields |
| `Label` | Form labels |
| `Switch` | Toggle settings |
| `Badge` | Status indicators |
| `Separator` | Visual dividers |
| `ScrollArea` | Scrollable containers |

### Theming

Colors are defined via CSS variables in `globals.css`:

```css
:root {
  --background: 0 0% 7%;
  --foreground: 0 0% 95%;
  --primary: 239 84% 67%;
  --destructive: 0 84% 60%;
  /* ... */
}
```

**NEVER hardcode colors**. Use Tailwind classes that reference theme variables:
- `bg-background`, `bg-primary`, `bg-destructive`
- `text-foreground`, `text-muted-foreground`
- `border-border`, `border-input`
