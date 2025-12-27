<p align="center">
  <img src="public/logo.png" alt="KaraokeTyper Logo" width="120" height="120">
</p>

<h1 align="center">KaraokeTyper</h1>

<p align="center">
  <strong>Offline voice dictation with push-to-talk</strong><br>
  <strong>Dictée vocale hors-ligne avec push-to-talk</strong>
</p>

<p align="center">
  <a href="#english">English</a> •
  <a href="#français">Français</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.1.0-blue.svg" alt="Version">
  <img src="https://img.shields.io/badge/license-Polyform%20NC-blue.svg" alt="License">
  <img src="https://img.shields.io/badge/platform-Linux%20|%20Windows%20|%20macOS-lightgrey.svg" alt="Platform">
  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/tauri-2.0-blueviolet.svg" alt="Tauri">
</p>

<p align="center">
  <img src="https://img.shields.io/badge/PRs-welcome-brightgreen.svg" alt="PRs Welcome">
  <img src="https://img.shields.io/badge/STT-sherpa--onnx-ff6b6b.svg" alt="STT Engine">
</p>

---

# English

## Overview

**KaraokeTyper** is a privacy-first, cross-platform voice dictation application. All speech recognition happens **100% offline** using sherpa-onnx with Whisper models - no cloud, no telemetry, no data leaving your machine.

Press `Ctrl+Space`, speak, release - your words appear instantly in any application.

## Features

| Feature | Description |
|---------|-------------|
| **Push-to-Talk** | Hold `Ctrl+Space` to record, release to transcribe |
| **100% Offline** | All AI processing happens locally on your machine |
| **Cross-Platform** | Native support for Linux, Windows, and macOS |
| **Privacy-First** | Zero telemetry, zero cloud, zero data collection |
| **Fast Transcription** | sherpa-onnx delivers high-accuracy results in seconds |
| **Auto-Type** | Text automatically injected into focused application |
| **Multi-Language** | Support for French, English, German, Spanish (configurable) |
| **Multiple Models** | Whisper Tiny/Base/Small/Medium/Large-v3 for quality/speed trade-offs |
| **Modern UI** | Clean dark theme with real-time status indicators |
| **System Tray** | Runs quietly in the background |

## Installation

### Prerequisites

- **Node.js** 18+ and npm
- **Rust** 1.70+

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
2. Select your preferred Whisper model (Tiny for speed, Large-v3 for accuracy)
3. Download the model (automatic via Settings)
4. Select your microphone
5. Ready to dictate!

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

## Available Models

| Model | Size | Speed | Accuracy | Best For |
|-------|------|-------|----------|----------|
| **Whisper Tiny** | ~75MB | Fastest | Good | Quick notes, low-resource devices |
| **Whisper Base** | ~150MB | Fast | Better | Daily dictation (recommended) |
| **Whisper Small** | ~500MB | Medium | Very Good | Professional use |
| **Whisper Medium** | ~1.5GB | Slower | Excellent | High accuracy needs |
| **Whisper Large-v3** | ~3GB | Slowest | Best | Maximum accuracy |

## Building

```bash
# Development
npm run tauri dev

# Production
npm run tauri build
```

**Output formats:**
- **Linux**: AppImage, .deb
- **Windows**: .msi, .exe
- **macOS**: .dmg, .app

## Privacy & Security

- All speech recognition runs **locally** on your hardware
- **No internet connection** required for core functionality
- **No telemetry**, analytics, or tracking of any kind
- **No cloud services** - your voice data never leaves your machine
- Audio is processed in memory and **never saved** to disk
- Open source - audit the code yourself

## License

This project is licensed under the **Polyform Noncommercial License 1.0.0** - see the [LICENSE](LICENSE) file for details.

**You CAN**: Use, modify, and share for personal/non-commercial purposes.
**You CANNOT**: Use commercially or sell without explicit permission.

---

# Français

## Présentation

**KaraokeTyper** est une application de dictée vocale multi-plateforme axée sur la confidentialité. Toute la reconnaissance vocale se fait **100% hors-ligne** avec sherpa-onnx et les modèles Whisper - pas de cloud, pas de télémétrie, aucune donnée ne quitte votre machine.

Appuyez sur `Ctrl+Espace`, parlez, relâchez - vos mots apparaissent instantanément dans n'importe quelle application.

## Fonctionnalités

| Fonctionnalité | Description |
|----------------|-------------|
| **Push-to-Talk** | Maintenez `Ctrl+Espace` pour enregistrer, relâchez pour transcrire |
| **100% Hors-ligne** | Tout le traitement IA se fait localement sur votre machine |
| **Multi-plateforme** | Support natif pour Linux, Windows et macOS |
| **Confidentialité** | Zéro télémétrie, zéro cloud, zéro collecte de données |
| **Transcription rapide** | sherpa-onnx offre des résultats de haute précision en quelques secondes |
| **Frappe automatique** | Le texte est automatiquement injecté dans l'application active |
| **Multi-langue** | Support du français, anglais, allemand, espagnol (configurable) |
| **Plusieurs modèles** | Whisper Tiny/Base/Small/Medium/Large-v3 pour différents compromis qualité/vitesse |
| **Interface moderne** | Thème sombre avec indicateurs de statut en temps réel |
| **Barre système** | Fonctionne discrètement en arrière-plan |

## Installation

### Prérequis

- **Node.js** 18+ et npm
- **Rust** 1.70+

### Démarrage rapide

```bash
# Cloner le dépôt
git clone https://github.com/Sebdev43/KaraokeTyper.git
cd KaraokeTyper

# Installer les dépendances
npm install

# Lancer en mode développement
npm run tauri dev
```

### Premier lancement

1. Ouvrez la fenêtre **Paramètres** (F10 ou barre système)
2. Sélectionnez votre modèle Whisper préféré (Tiny pour la vitesse, Large-v3 pour la précision)
3. Téléchargez le modèle (automatique via Paramètres)
4. Sélectionnez votre microphone
5. Prêt à dicter !

## Utilisation

### Workflow de base

1. **Focalisez** n'importe quel champ texte (navigateur, éditeur, terminal...)
2. **Maintenez** `Ctrl+Espace` et parlez clairement
3. **Relâchez** pour transcrire et taper automatiquement

### Raccourcis clavier

| Raccourci | Action |
|-----------|--------|
| `Ctrl+Espace` | Push-to-talk (maintenir pour enregistrer) |
| `F10` | Ouvrir les paramètres |

### Indicateurs de statut

| Couleur | État |
|---------|------|
| 🟢 Vert | Prêt - en attente |
| 🔴 Rouge | Enregistrement - parlez maintenant |
| 🟠 Orange | Traitement - transcription en cours |

## Modèles disponibles

| Modèle | Taille | Vitesse | Précision | Idéal pour |
|--------|--------|---------|-----------|------------|
| **Whisper Tiny** | ~75Mo | Plus rapide | Bonne | Notes rapides, appareils peu puissants |
| **Whisper Base** | ~150Mo | Rapide | Meilleure | Dictée quotidienne (recommandé) |
| **Whisper Small** | ~500Mo | Moyenne | Très bonne | Usage professionnel |
| **Whisper Medium** | ~1.5Go | Lente | Excellente | Haute précision |
| **Whisper Large-v3** | ~3Go | Plus lente | Meilleure | Précision maximale |

## Compilation

```bash
# Développement
npm run tauri dev

# Production
npm run tauri build
```

**Formats de sortie :**
- **Linux** : AppImage, .deb
- **Windows** : .msi, .exe
- **macOS** : .dmg, .app

## Confidentialité et sécurité

- Toute la reconnaissance vocale s'exécute **localement** sur votre machine
- **Aucune connexion internet** requise pour les fonctionnalités principales
- **Aucune télémétrie**, analytique ou suivi
- **Aucun service cloud** - vos données vocales ne quittent jamais votre machine
- L'audio est traité en mémoire et **jamais sauvegardé** sur le disque
- Open source - auditez le code vous-même

## Licence

Ce projet est sous licence **Polyform Noncommercial License 1.0.0** - voir le fichier [LICENSE](LICENSE) pour plus de détails.

**Vous POUVEZ** : Utiliser, modifier et partager pour usage personnel/non-commercial.
**Vous NE POUVEZ PAS** : Utiliser commercialement ou vendre sans autorisation explicite.

---

## Architecture

```
KaraokeTyper/
├── src/                    # Frontend React/TypeScript
│   ├── components/         # Composants UI (shadcn/ui)
│   └── windows/            # Vues spécifiques aux fenêtres
├── src-tauri/              # Backend Rust
│   ├── src/
│   │   ├── lib.rs          # Commandes Tauri & setup
│   │   ├── audio.rs        # Capture audio (cpal)
│   │   ├── audio_cli.rs    # Capture audio fallback (parec)
│   │   ├── stt_parakeet.rs # Intégration sherpa-onnx STT
│   │   ├── model_downloader.rs  # Gestionnaire de téléchargement
│   │   ├── output.rs       # Injection de texte (enigo/uinput)
│   │   └── keyboard_listener.rs # Gestionnaire push-to-talk
│   └── python/
│       ├── gpu_detector.py # Détection GPU pour optimisation
│       └── parakeet_service.py  # Service Python fallback
└── README.md
```

## Tech Stack

| Couche | Technologie |
|--------|-------------|
| **Frontend** | React 18, TypeScript, Tailwind CSS, shadcn/ui |
| **Backend** | Rust, Tauri 2.0 |
| **Moteur STT** | sherpa-onnx (modèles Whisper) |
| **Audio** | cpal (multi-plateforme), parec fallback (Linux) |
| **Clavier** | rdev (Windows/macOS), evdev (Linux) |
| **Sortie texte** | enigo, arboard, uinput |

---

## Contributing / Contribuer

We welcome contributions! / Les contributions sont les bienvenues !

1. **Fork** the repository / Forkez le dépôt
2. Create a **feature branch** from `dev` / Créez une branche depuis `dev`
3. Make your changes / Faites vos modifications
4. Run tests / Lancez les tests :
   ```bash
   cd src-tauri && cargo clippy && cargo test
   npx biome check --write src/
   ```
5. Submit a **Pull Request** to `dev` / Soumettez une PR vers `dev`

---

## Credits / Crédits

- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) - High-performance speech recognition
- [OpenAI Whisper](https://github.com/openai/whisper) - Speech recognition models
- [Tauri](https://tauri.app) - Cross-platform app framework
- [shadcn/ui](https://ui.shadcn.com) - Beautiful UI components
- [cpal](https://github.com/RustAudio/cpal) - Cross-platform audio I/O
- [rdev](https://github.com/Narsil/rdev) - Cross-platform input events
- [enigo](https://github.com/enigo-rs/enigo) - Cross-platform input simulation

---

<p align="center">
  <strong>Built with care for privacy-conscious users</strong><br>
  <strong>Conçu avec soin pour les utilisateurs soucieux de leur vie privée</strong>
</p>

<p align="center">
  <a href="https://github.com/Sebdev43/KaraokeTyper/issues">Report Bug / Signaler un bug</a> •
  <a href="https://github.com/Sebdev43/KaraokeTyper/issues">Request Feature / Demander une fonctionnalité</a>
</p>
