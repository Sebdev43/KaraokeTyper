# Plan de Développement - KaraokeTyper

## Vision

**KaraokeTyper** est une application de dictée vocale 100% locale pour Ubuntu/Linux, offrant une expérience utilisateur fluide type "push-to-talk" avec intégration transparente au système.

## Objectifs

### ✅ Fonctionnalités Principales
- [ ] Dictée vocale offline (Vosk ou Whisper)
- [ ] Push-to-talk avec raccourci global (Ctrl+Space)
- [ ] Paste automatique dans le champ actif
- [ ] Interface GTK4 moderne et légère
- [ ] Support Wayland complet
- [ ] Historique de dictées avec recherche
- [ ] Gestion des modèles de langue

### ✅ Fonctionnalités Avancées (Phase 2)
- [ ] Commandes vocales (comme VibeTyper: "rewrite", "reply")
- [ ] Correction automatique et ponctuation intelligente
- [ ] Personnalisation du vocabulaire
- [ ] Statistiques d'utilisation

## Architecture Technique

### Stack Technologique

**Backend (Python 3.12+)**
- `vosk` - STT offline (ou `faster-whisper` en alternative)
- `pyaudio` / `sounddevice` - Capture audio
- `ydotool` - Injection clavier sur Wayland
- `pynput` ou `python-xlib` - Raccourcis globaux
- `PyGObject` - GTK4 bindings

**Frontend**
- GTK4 avec libadwaita - Interface native GNOME
- CSS custom pour thème sombre/clair

**Stockage**
- SQLite pour historique de dictées
- JSON pour configuration utilisateur
- Cache des modèles dans `~/.local/share/karaoketyper/`

### Architecture Modulaire

```
karaoketyper/
├── core/
│   ├── audio_capture.py      # Gestion microphone
│   ├── stt_engine.py          # Vosk/Whisper wrapper
│   ├── text_output.py         # ydotool + clipboard
│   └── command_parser.py      # Détection commandes vocales
├── ui/
│   ├── main_window.py         # Fenêtre principale GTK4
│   ├── settings_dialog.py     # Préférences
│   ├── history_view.py        # Historique dictées
│   └── model_manager.py       # Téléchargement modèles
├── services/
│   ├── shortcut_listener.py   # Raccourcis globaux
│   ├── state_manager.py       # État recording/idle
│   └── config_manager.py      # Configuration
├── data/
│   ├── database.py            # SQLite helpers
│   └── models.py              # ORM simple
└── utils/
    ├── logger.py              # Logging centralisé
    └── validators.py          # Validation entrées
```

## Workflow Utilisateur

### Flux de Dictée Basique

1. **Idle** - Application en arrière-plan, icône dans system tray
2. **Trigger** - Utilisateur presse `Ctrl+Space` (push-to-talk)
3. **Recording** - Indicateur visuel (couleur rouge), capture audio
4. **Release** - Utilisateur relâche `Ctrl+Space`
5. **Processing** - Traitement STT local (100-500ms selon modèle)
6. **Output** - Texte tapé automatiquement dans le champ actif via ydotool

### Flux Avancé (Commandes)

1. **Detection** - Si dictée commence par "rewrite:", "reply:", etc.
2. **Context** - Lecture du presse-papier pour contexte
3. **Processing** - Application de la commande (local, pas d'IA cloud)
4. **Output** - Résultat collé dans le champ

## Comparaison avec Alternatives

| Fonctionnalité | VibeTyper | nerd-dictation | **KaraokeTyper** |
|---|---|---|---|
| Offline | ❌ Cloud | ✅ Oui | ✅ Oui |
| Interface graphique | ✅ Electron | ❌ CLI | ✅ GTK4 |
| Push-to-talk | ✅ Oui | ❌ Toggle | ✅ Oui |
| Commandes vocales | ✅ Oui (GPT) | ❌ Non | ✅ Oui (local) |
| Historique | ✅ Oui | ❌ Non | ✅ Oui |
| Wayland | ✅ Oui | ✅ Oui | ✅ Oui |
| Open Source | ❌ Non | ✅ GPL | ✅ GPL v3 |
| Poids install | ~200MB | ~50MB | **~80MB** |

## Phases de Développement

### Phase 1: MVP (Minimum Viable Product)

**Objectif**: Dictée vocale basique fonctionnelle

1. ✅ **Setup Projet** (1 jour)
   - Créer structure de dossiers
   - Setup venv Python
   - Installer dépendances de base
   - Créer repo GitHub privé

2. **Core STT Engine** (2 jours)
   - Intégrer Vosk avec modèle anglais
   - Capturer audio avec PyAudio
   - Transcription basique

3. **Output System** (1 jour)
   - Intégration ydotool
   - Typage automatique du texte reconnu
   - Gestion des caractères spéciaux

4. **Shortcut Listener** (2 jours)
   - Écoute raccourci global (Ctrl+Space)
   - State machine: idle → recording → processing → output
   - Indicateur visuel basique

5. **UI GTK4 Basique** (2 jours)
   - Fenêtre principale avec status
   - Bouton start/stop manuel
   - Icône system tray
   - Settings de base (choix modèle, raccourci)

### Phase 2: Enrichissement

1. **Historique de Dictées** (1 jour)
   - SQLite database
   - Vue liste avec recherche
   - Export CSV/JSON

2. **Commandes Vocales** (3 jours)
   - Parser de commandes (rewrite, reply, etc.)
   - Lecture presse-papier
   - Logique de transformation de texte locale
   - Templates de commandes configurables

3. **Model Manager** (2 jours)
   - Interface téléchargement modèles Vosk
   - Support multi-langues (FR, EN, ES, DE)
   - Switch rapide entre modèles

4. **UI Polish** (2 jours)
   - Animations GTK4
   - Thème libadwaita
   - Préférences avancées
   - About dialog

### Phase 3: Optimisation

1. **Performance** (2 jours)
   - Buffering audio intelligent
   - Cache modèles en RAM
   - Optimisation latence (<500ms)

2. **Alternative Whisper** (3 jours)
   - Intégration faster-whisper
   - Comparaison benchmarks Vosk vs Whisper
   - Switch dynamique selon besoin

3. **Documentation** (1 jour)
   - README complet
   - Guide installation
   - Tutoriel utilisateur
   - Documentation développeur

## Détails Techniques Clés

### 1. Capture Audio

```python
# audio_capture.py
import sounddevice as sd
import numpy as np
from queue import Queue

class AudioCapture:
    def __init__(self, sample_rate=16000, channels=1):
        self.sample_rate = sample_rate
        self.channels = channels
        self.audio_queue = Queue()
        self.is_recording = False

    def start(self):
        self.is_recording = True
        self.stream = sd.InputStream(
            samplerate=self.sample_rate,
            channels=self.channels,
            callback=self._audio_callback
        )
        self.stream.start()

    def stop(self):
        self.is_recording = False
        self.stream.stop()
        self.stream.close()
        return self._get_recorded_audio()

    def _audio_callback(self, indata, frames, time, status):
        if self.is_recording:
            self.audio_queue.put(indata.copy())
```

### 2. STT Engine (Vosk)

```python
# stt_engine.py
from vosk import Model, KaldiRecognizer
import json

class STTEngine:
    def __init__(self, model_path="/opt/karaoketyper/models/vosk-model-en"):
        self.model = Model(model_path)
        self.recognizer = KaldiRecognizer(self.model, 16000)

    def transcribe(self, audio_data: bytes) -> str:
        """Transcrit audio en texte"""
        if self.recognizer.AcceptWaveform(audio_data):
            result = json.loads(self.recognizer.Result())
            return result.get('text', '')
        return ''

    def reset(self):
        """Reset pour nouvelle session"""
        self.recognizer = KaldiRecognizer(self.model, 16000)
```

### 3. Text Output (ydotool)

```python
# text_output.py
import subprocess
import tempfile

class TextOutput:
    @staticmethod
    def type_text(text: str, delay_ms: int = 8):
        """Type le texte via ydotool"""
        # Créer fichier temporaire avec le texte
        with tempfile.NamedTemporaryFile(mode='w', delete=False) as f:
            f.write(text)
            temp_path = f.name

        # Taper via ydotool
        subprocess.run([
            'ydotool', 'type',
            '--key-delay', str(delay_ms),
            '--file', temp_path
        ])

        # Cleanup
        os.unlink(temp_path)

    @staticmethod
    def paste_from_clipboard():
        """Alternative: coller depuis presse-papier"""
        # Ctrl+V via ydotool
        subprocess.run(['ydotool', 'key', '29:1', '47:1', '47:0', '29:0'])
```

### 4. Shortcut Listener

```python
# shortcut_listener.py
from pynput import keyboard
from typing import Callable

class ShortcutListener:
    def __init__(self, on_press: Callable, on_release: Callable):
        self.on_press_callback = on_press
        self.on_release_callback = on_release
        self.target_key = keyboard.Key.space
        self.modifier = keyboard.Key.ctrl
        self.is_modifier_pressed = False

    def start(self):
        with keyboard.Listener(
            on_press=self._on_press,
            on_release=self._on_release
        ) as listener:
            listener.join()

    def _on_press(self, key):
        if key == self.modifier:
            self.is_modifier_pressed = True
        elif key == self.target_key and self.is_modifier_pressed:
            self.on_press_callback()

    def _on_release(self, key):
        if key == self.modifier:
            self.is_modifier_pressed = False
        elif key == self.target_key:
            self.on_release_callback()
```

### 5. State Manager

```python
# state_manager.py
from enum import Enum
from dataclasses import dataclass
from typing import Optional

class DictationState(Enum):
    IDLE = "idle"
    RECORDING = "recording"
    PROCESSING = "processing"
    TYPING = "typing"

@dataclass
class DictationSession:
    id: str
    state: DictationState
    start_time: float
    audio_data: Optional[bytes] = None
    transcribed_text: Optional[str] = None
    error: Optional[str] = None

class StateManager:
    def __init__(self):
        self.current_session: Optional[DictationSession] = None
        self.observers = []

    def transition_to(self, new_state: DictationState):
        """Change l'état et notifie les observers"""
        if self.current_session:
            self.current_session.state = new_state
            self._notify_observers(new_state)

    def _notify_observers(self, state: DictationState):
        for observer in self.observers:
            observer(state)
```

### 6. GTK4 Main Window

```python
# ui/main_window.py
import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, Adw, GLib

class KaraokeTyperWindow(Adw.ApplicationWindow):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.set_title("KaraokeTyper")
        self.set_default_size(600, 400)

        # Header bar
        header = Adw.HeaderBar()
        self.set_titlebar(header)

        # Status indicator
        self.status_label = Gtk.Label(label="Ready")
        self.status_indicator = Gtk.Image.new_from_icon_name("microphone-disabled")

        # Layout
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        box.set_margin_top(12)
        box.set_margin_bottom(12)
        box.set_margin_start(12)
        box.set_margin_end(12)

        box.append(self.status_indicator)
        box.append(self.status_label)

        self.set_content(box)

    def update_status(self, state: str):
        """Met à jour le status visuel"""
        GLib.idle_add(self._update_ui, state)

    def _update_ui(self, state: str):
        if state == "recording":
            self.status_label.set_text("🔴 Recording...")
            self.status_indicator.set_from_icon_name("microphone-sensitivity-high")
        elif state == "processing":
            self.status_label.set_text("⚙️ Processing...")
        else:
            self.status_label.set_text("Ready")
            self.status_indicator.set_from_icon_name("microphone-disabled")
```

## Configuration Système Requise

### Dépendances Système

```bash
# Audio
sudo apt install pipewire-audio-client-libraries libasound2-dev portaudio19-dev

# ydotool (Wayland)
sudo apt install ydotool

# GTK4 + libadwaita
sudo apt install libgtk-4-dev libadwaita-1-dev python3-gi

# Permissions udev pour /dev/uinput
echo 'KERNEL=="uinput", GROUP="input", MODE="0660"' | sudo tee /etc/udev/rules.d/80-uinput.rules
sudo usermod -a -G input $USER
```

### Dépendances Python (requirements.txt)

```txt
vosk==0.3.45
sounddevice==0.4.6
numpy==1.24.3
PyGObject==3.46.0
pynput==1.7.6
```

### Service Systemd pour ydotoold

```ini
# ~/.config/systemd/user/ydotoold.service
[Unit]
Description=ydotoold - input injection daemon for Wayland

[Service]
ExecStart=/usr/bin/ydotoold
Restart=on-failure

[Install]
WantedBy=default.target
```

Activation:
```bash
systemctl --user enable ydotoold
systemctl --user start ydotoold
```

## Installation Utilisateur

### Via Script Install (Recommandé)

```bash
# Clone repo
git clone https://github.com/username/karaoketyper.git
cd karaoketyper

# Lancer script d'installation
./install.sh
```

### Manuelle

```bash
# Créer venv
python3 -m venv ~/.local/share/karaoketyper/venv
source ~/.local/share/karaoketyper/venv/bin/activate

# Installer dépendances
pip install -r requirements.txt

# Télécharger modèle Vosk (en-us, ~300MB)
mkdir -p ~/.local/share/karaoketyper/models
cd ~/.local/share/karaoketyper/models
wget https://alphacephei.com/vosk/models/vosk-model-en-us-0.22.zip
unzip vosk-model-en-us-0.22.zip
mv vosk-model-en-us-0.22 vosk-model-en

# Créer launcher desktop
cp karaoketyper.desktop ~/.local/share/applications/
```

## Choix Techniques Justifiés

### Pourquoi Vosk plutôt que Whisper ?

**Vosk avantages:**
- Plus léger (300MB vs 1.5GB pour Whisper)
- Latence plus faible (~100ms vs ~500ms)
- Moins de CPU/RAM
- Parfait pour dictée temps réel

**Whisper avantages:**
- Meilleure précision (+5-10%)
- Support de plus de langues
- Meilleur avec accents forts

**Notre choix:** Vosk par défaut, option Whisper pour précision maximale.

### Pourquoi GTK4 plutôt qu'Electron ?

- **Poids**: 10MB vs 200MB pour Electron
- **RAM**: 50MB vs 300MB pour Electron
- **Intégration**: Native GNOME avec libadwaita
- **Performance**: Pas de overhead Chromium
- **Écologie**: Moins de ressources = moins d'énergie

### Pourquoi ydotool plutôt que xdotool ?

- **Wayland**: xdotool ne fonctionne pas sur Wayland
- **Sécurité**: ydotool respecte le modèle de sécurité Wayland
- **Futur**: Wayland devient le standard sur Linux

## Licence et Distribution

**Licence**: GPL v3 (comme nerd-dictation)

**Pourquoi GPL v3 ?**
- Garantit que le code reste libre
- Empêche fork propriétaire (à la VibeTyper)
- Compatible avec toutes nos dépendances
- Cohérent avec l'esprit "vraiment offline, vraiment libre"

## Roadmap Post-MVP

### Version 1.1
- Support français natif (modèle Vosk FR)
- Export historique en markdown
- Macros vocales personnalisées

### Version 1.2
- Intégration OpenAI Whisper local
- Correction orthographique contextuelle
- Personnalisation vocabulaire technique

### Version 1.3
- Support multi-micro (sélection device)
- Noise cancellation avancé
- Mode "push to talk" vs "voice activated"

### Version 2.0
- Support Windows/macOS (sans ydotool)
- Plugin VSCode/IntelliJ
- API REST locale pour intégration tierce

## Métriques de Succès

**Performance**
- [ ] Latence < 500ms (start → output)
- [ ] Précision > 90% (anglais clair)
- [ ] RAM < 100MB en idle
- [ ] CPU < 5% en idle

**UX**
- [ ] Installation < 5 minutes
- [ ] Premier use < 30 secondes
- [ ] Raccourci configurable en 1 clic

**Qualité**
- [ ] Tests unitaires > 80% coverage
- [ ] Zéro segfault en 100h d'utilisation
- [ ] Documentation complète (EN + FR)

## Conclusion

KaraokeTyper sera **le meilleur outil de dictée vocale offline pour Linux**, combinant:
- La légèreté de nerd-dictation
- L'UX fluide de VibeTyper
- Une architecture moderne et maintenable
- Un engagement fort pour le logiciel libre

**Prochaine étape**: Setup du projet et Phase 1.1 (Core STT Engine)
