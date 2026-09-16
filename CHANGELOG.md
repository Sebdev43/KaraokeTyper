# Changelog

All notable changes to KaraokeTyper will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.8.0] - 2026-09-16

### Modifié

- **Moteur de transcription** : transcribe-rs 0.3.11 — whisper.cpp 1.7.4 → 1.8.3 (whisper-rs 0.16) et ONNX Runtime 1.22 → 1.24.2 (Parakeet)
- **Réglage GPU effectif** : l'interrupteur « GPU » et l'index de device sont désormais transmis à whisper.cpp (ils étaient ignorés) ; le GPU auto-désactivé après échecs l'est réellement au chargement, et un modèle manquant ne compte plus comme un échec GPU
- **Features Cargo `gpu-vulkan` / `gpu-metal`** : activent réellement le backend GPU de whisper.cpp (un `cargo build` nu est CPU seul ; les builds publiés passent la feature)
- **Dépendances** : mise à jour complète Rust (tauri 2.11.5, reqwest 0.13, evdev 0.13, sha2 0.11, ollama-rs 0.3 en rustls, zip 8, dirs 6, thiserror 2, jemalloc 0.7) et npm (Vite 8, TypeScript 7, Biome 2.5, lucide 1, React 19.3, Tailwind 4.3)

### Corrigé

- **Sécurité** : avis corrigés par les mises à jour (tauri CVE-2026-42184, `tar` ×3, 7 avis npm) ; plus d'OpenSSL embarqué avec la feature `ollama`
- **À propos** affiche la version réelle de l'application (1.5.0 était codée en dur)
- **Ollama** : plus de chemin de panique si le client HTTP ne peut pas être construit
- **Journal** : `[APP] Exit requested` indique désormais pourquoi l'application se ferme (dernière fenêtre détruite ou sortie explicite)

## [1.7.0] - 2026-09-13

### Ajouté

- **Mode STT hybride Parakeet/Whisper** : les dictées courtes passent par Parakeet (bien plus rapide), les longues restent sur Whisper ; seuil de bascule réglable, avec rechargement de moteur sécurisé
- **Modes d'activation du raccourci** : *Maintenir* (push-to-talk, comportement historique par défaut), *Basculer* (un appui démarre, le suivant arrête) et *Maintenir ou basculer* (appui long = push-to-talk, appui bref = bascule, seuil réglable)

### Modifié

- **Refonte visuelle** de la fenêtre de paramètres
- **Callback micro temps-réel** : plus aucune allocation sur le thread audio (ring SPSC `rtrb`)
- **Message de statut** adapté au mode d'activation (« relâchez » vs « appuyez de nouveau ») et sans touche codée en dur

### Corrigé

- **xdotool** : plus de modificateur (Ctrl…) latché sur le clavier XTEST après un collage ou une frappe (`--clearmodifiers`)
- **Micro** : le flux de capture est rebâti après une perte de périphérique (débranchement, dropout) au lieu d'enregistrer du silence jusqu'au redémarrage
- **Wayland/GNOME** : `wtype` n'est plus tenté sous Mutter (qui ne l'implémente pas), évitant un échec systématique par dictée

## [1.5.2] - 2026-06-17

### Changed

- **Téléchargeur de modèles durci** : un seul watchdog anti-blocage partagé par tous les chemins de téléchargement (mono-fichier, multi-fichiers, archive), et nettoyage systématique des fichiers/dossiers partiels en cas d'interruption
- **Protection disque** : le téléchargement d'un modèle mono-fichier (Whisper) est désormais plafonné à 1,5× la taille annoncée (défense contre un miroir qui streamerait des gigaoctets pour saturer le disque)
- **Anti-concurrence** : impossible de lancer deux téléchargements simultanés du même modèle (ils se corrompaient mutuellement)

## [1.5.1] - 2026-06-17

### Fixed

- **Fuite presse-papier** : le presse-papier est désormais nettoyé sur **tous** les chemins de collage, y compris en cas d'échec du raccourci de collage (la transcription ne reste plus dans le presse-papier)
- **Ollama sans timeout** : `correct_text()` borne l'appel `generate()` à 30s — un serveur Ollama figé ne bloque plus la dictée indéfiniment
- **Robustesse transcription** : isolation des panics du moteur STT (`catch_unwind`, plus d'empoisonnement de mutex ni de réutilisation d'un moteur corrompu), libération du verrou pendant l'inférence et sérialisation des transcriptions concurrentes
- **Téléchargements modèles** : `connect_timeout` + watchdog anti-stall par chunk (sans timeout global qui casserait les gros modèles), nettoyage des fichiers partiels en cas de blocage
- **Casse / accents** : matching des mots personnalisés en comptage de caractères (et non d'octets) pour le français accentué, détection ALL-CAPS corrigée sur les caractères non-cassés

### Changed

- Lecture de `paste_method` depuis le cache mémoire sur le chemin de collage (plus de lecture/écriture de `settings.json` à chaque dictée)
- `detect_gpu()` mis en cache (`OnceLock`) — plus de sous-processus relancés à chaque ouverture des réglages ; mécanisme d'auto-désactivation GPU recâblé (builds GPU)
- Optimisations allocations : matching des mots personnalisés hoisté, traitement VAD sans allocation par frame, copies audio évitées
- Frappe clavier déportée hors du thread async ; dépendance `ringbuf` inutilisée retirée

## [1.5.0] - 2026-04-22

### Added

- **Moteur STT Parakeet V3** (NVIDIA, int8 ONNX via `transcribe-rs`): 25 langues UE dont le français, auto-détection de la langue, très bonne précision sur voix lente et rapide. Sélectionnable dans **Paramètres → Reconnaissance vocale**
- **Téléchargement sécurisé multi-fichiers** (`model_downloader.rs`): 6 fichiers ONNX récupérés depuis HuggingFace (`istupakov/parakeet-tdt-0.6b-v3-onnx`, commit pinné), **chaque fichier vérifié contre un SHA-256 hardcodé** avant installation. Si un fichier est modifié en amont, l'install est refusée
- **Protection anti-concurrence** : `Mutex<HashSet>` global empêche deux téléchargements simultanés du même modèle de se corrompre mutuellement
- **Replace atomique avec backup** : le modèle existant est renommé `.old` avant installation du nouveau, restauré en cas d'échec — plus de perte de modèle sur crash
- **Cap taille téléchargement** : abort automatique au-delà de 1.5× la taille déclarée (défense anti-zip-bomb / serveur malveillant)

### Changed

- `EngineType::Parakeet` ajouté à l'enum STT, suivi de `supported_languages()` qui expose la liste des codes ISO par moteur
- `ModelInfo` gagne un champ `files: Vec<ModelFile>` (filename + url + sha256) pour les modèles multi-fichiers
- Déps: `sha2 = "0.10"` pour l'intégrité, `tar = "0.4"` + `flate2 = "1"` pour les futurs engines archive-based

### Security

- Validation stricte des manifests modèles : rejet des filenames contenant `..`, chemins absolus, NUL bytes
- Validation stricte des SHA-256 : exactement 64 caractères hexadécimaux
- Architecture sans dépendance tar.gz pour Parakeet (évite les attaques tar-slip / symlink escape)

## [1.4.0] - 2026-04-22

### Added

- **Auto-update multi-OS** (`updater.rs`, `UpdateChecker.tsx`): vérification + installation automatique de nouvelles versions depuis GitHub Releases. Intégré dans **Paramètres → À propos → Rechercher**
  - **AppImage / MSI / EXE / DMG** : via `tauri-plugin-updater` 2.10 (download, replace binary, restart — tout silencieux)
  - **.deb** : chemin custom (`pkexec apt-get install` sur une copie privée 0700 dans `$XDG_CACHE_HOME`, compatible avec le pattern PolicyKit déjà utilisé pour `install-permissions.sh`)
  - Détection runtime du type d'install : `$APPIMAGE`, `/var/lib/dpkg/info/karaoketyper.list`, `/Applications`, etc.
  - Fallback "Voir la page GitHub" si le type d'install n'est pas auto-compatible
- **Script `scripts/generate-latest-json.sh`** : agrège les signatures minisign de chaque plateforme en un seul manifest `latest.json` uploadé sur la Release
- **Job CI `generate-updater-manifest`** : appelle le script après les builds, publie `latest.json` pour l'endpoint de l'updater
- **Crate `tauri-plugin-updater = "2.10"`** + `@tauri-apps/plugin-updater@^2.10`

### Security

- **Téléchargement `.deb` dans un dir privé 0700** (au lieu de `/tmp`) pour éliminer un TOCTOU swap entre download et `pkexec apt install`
- **Match exact du nom `.deb`** (pas de fallback suffixe) : empêche l'install d'un asset tiers au nom compatible
- **`is_newer`** traite les composants semver manquants comme `0` et refuse les inputs non parseables → plus de boucle de MAJ
- **`generate-latest-json.sh`** : VERSION validée par regex stricte, JSON construit 100 % via `jq --arg/--argjson` (pas de concat de strings)
- **Messages d'erreur `pkexec`** : codes 126/127 mappés en messages FR explicites (annulation utilisateur vs PolicyKit indisponible)

### Known limitations

- **`.deb` non signé minisign** : le `.deb` est téléchargé sur HTTPS mais sans vérification de signature côté client (contrairement aux autres bundles). Mitigation : HTTPS + GitHub Releases. À traiter en 1.4.x (ajout d'un `.deb.sig` publié par le CI)
- **Pas de capability gating dédiée sur `install_update_cmd`** : mitigé par la CSP restrictive et l'absence de webview user-content. Fix recommandé : capability window-restricted + `dialog::confirm` natif avant `pkexec`

## [1.3.2] - 2026-04-19

### Changed

- **CI: GitHub Actions bumpées vers versions Node 24 compatibles**: `actions/checkout@v4→v5`, `actions/setup-node@v4→v5`, `actions/github-script@v7→v8`. Node runtime des builds: `20→22` (LTS). Préparation de la deadline GitHub du 16 septembre 2026 (suppression totale de Node 20 des runners)
- **Logs release 100% silencieux**: Conversion des 11 `println!`/`eprintln!` résiduels (`config.rs`, `output.rs`, `autostart.rs`) en `crate::dev_log!` — en release builds, **aucune** sortie console côté Rust (seuls les messages de la lib ALSA système restent, non contrôlables)

## [1.3.1] - 2026-04-19

### Added

- **Blacklist d'hallucinations Whisper multilingue** (`text_processing.rs`): 38 phrases hallucinées sur 7 langues (FR, EN, ES, DE, IT, PT, NL). Source : Reddit r/LocalLLaMA, dataset `sachaarbonel/whisper-hallucinations` (HuggingFace), papier "Careless Whisper" (FAccT 2024). Supprime les outros YouTube, watermarks de sous-titres, appels à s'abonner qui apparaissent sur silence ou audio ambigu
- **Premier pass de strip hallucinations**: `strip_hallucinated_phrases()` applique un match case-insensitive avec double-pass pour les émissions répétées consécutives du modèle

### Fixed

- **Reconfiguration hotkey evdev à chaud**: `reconfigure()` couvre désormais les transitions de classe de device (clavier → souris), ce qui corrige le raccourci qui ne se mettait pas à jour au save_settings quand on passait d'un type d'input à un autre

## [1.3.0] - 2026-03-31

### Added

- **Text processing pipeline** (`text_processing.rs`): Filler word removal (euh, um, ah), stutter collapse (je je je → je), custom words fuzzy matching (Levenshtein + Soundex phonétique)
- **Whisper initial_prompt**: Construit automatiquement depuis les custom words pour biaiser la reconnaissance vers le vocabulaire de l'utilisateur
- **Custom words config**: `custom_words`, `custom_words_threshold`, `filler_removal_enabled`, `stutter_collapse_enabled` dans les settings
- **Terminal auto-detection**: Détection GNOME D-Bus (extension + Shell Eval) pour choisir automatiquement Ctrl+V ou Ctrl+Shift+V
- **Remote desktop detection**: RustDesk, AnyDesk, Remmina, Vinagre détectés via D-Bus — skip terminal auto-detection, utilise la méthode configurée
- **Dépendance strsim**: Distance de Levenshtein normalisée pour le fuzzy matching des custom words

### Changed

- **Clipboard: arboard → tauri-plugin-clipboard-manager**: Migration vers le plugin Tauri officiel (comme Handy), résout le problème "image invalide" dans Remmina où arboard posait le clipboard dans un format incorrect
- **Output fallback intelligent**: Quand `paste_method: Direct` et aucun outil natif X11 (xdotool), auto-fallback vers clipboard + paste shortcut au lieu d'enigo text() buggy
- **D-Bus timeout 200ms**: Les appels D-Bus pour la détection de fenêtre ne bloquent plus indéfiniment

### Fixed

- **Enigo text() X11 — mots mangés**: Auto-fallback clipboard quand xdotool absent, enigo text() n'est plus utilisé pour le texte (uniquement pour les raccourcis clavier)
- **AZERTY keyboard garbling**: wtype/ydotool ne sont plus utilisés pour le direct typing sur X11 (keycodes QWERTY incompatibles avec AZERTY), seul xdotool est fiable sur X11
- **Overlay focus — paste vers mauvaise fenêtre**: L'overlay est caché immédiatement (pas d'animation 300ms) avant le paste pour que le focus revienne à l'app cible
- **Terminal detection timing**: La détection terminal se fait APRÈS le clipboard set + délai focus (pas avant), pour détecter la vraie fenêtre cible au lieu de KaraokeTyper

## [1.2.0] - 2026-03-11

### Changed

- **Architecture audio worker thread**: Réécriture complète du module audio capture (`audio.rs`) en architecture worker thread avec channels mpsc, inspirée de Handy. Le stream CPAL tourne en continu dans un thread dédié, les commandes Start/Stop sont sérialisées via un channel — élimine toutes les race conditions PTT
- **StopHandle pattern**: `stop_recording()` retourne un handle non-bloquant, le Mutex est relâché avant d'attendre la réponse du worker (évite de bloquer le thread pool Tauri pendant 5s max)

### Fixed

- **Race condition PTT premier appui**: L'ouverture ALSA (~2.2s) ne cause plus de perte audio — les commandes sont queueées et traitées après que le stream est prêt
- **Worker panic safety**: ScopeGuard RAII garantit que `is_open` est remis à false même en cas de panic du worker thread
- **Bluetooth auto-close**: Le timer BT met correctement `is_open=false` avant d'envoyer Shutdown, évitant un état incohérent
- **Worker Shutdown bloqué**: `recv_timeout(500ms)` au lieu de `recv()` bloquant — les commandes Shutdown sont traitées même si le device audio ne produit plus de samples

## [1.1.0] - 2026-03-07

### Added

- **Contrôle du volume des sons**: Bouton mute avec mémoire du volume précédent dans les paramètres

### Changed

- **Interface 100% française**: Traduction complète de toutes les strings UI (48+ strings) — paramètres, modèles, système, Ollama, à propos
- **Version corrigée dans À propos**: Affiche désormais la version réelle (v1.1.0) au lieu de v0.1.0

### Fixed

- **Debug WAV en production**: Sauvegarde audio debug (`/tmp/karaoketyper_debug.wav`) désormais gatée par `#[cfg(debug_assertions)]` — n'écrit plus en release
- **CSP sécurisée**: Remplacement de `csp: null` par une politique restrictive (`default-src 'self'`)

## [1.0.9] - 2026-03-07

### Changed

- **Overlay rewrite**: Overlay purement visuel (CSS pur, plus de Tailwind) avec fade-in/fade-out 300ms
- **Overlay programmatique**: Fenetre overlay creee en Rust (WebviewWindowBuilder) au lieu de tauri.conf.json — shadow(false), focused(false), closable(false)
- **Positionnement dynamique**: Overlay centre sur l'ecran du curseur a chaque affichage
- **Paste cross-platform**: Detection intelligente des apps problematiques (Remmina, RustDesk) pour basculer entre clipboard+paste et direct typing via enigo
- **enigo 0.6**: Migration de enigo 0.2 vers 0.6 (XTest protocol level au lieu de uinput kernel level)

### Fixed

- **Overlay double couche**: Suppression du flash entre etats recording/processing (mecanisme fade-in/fade-out identique a Handy)
- **Overlay invisible**: globals.css (Tailwind) ecrasait la transparence de l'overlay — supprime de l'overlay entry point
- **Overlay vol de focus**: Suppression de tous les set_focus() sur l'overlay
- **Mutex poisoning**: Remplacement de tous les .unwrap() par .unwrap_or_else(|e| e.into_inner()) sur les locks audio et STT
- **Paste Remmina**: Utilisation de enigo direct typing (XTest) pour les apps remote desktop au lieu de clipboard+paste

## [1.0.8] - 2026-03-05

### Added

- **Accélération GPU Vulkan**: Auto-détection GPU au démarrage, activation automatique si compilé avec support GPU
- **Détection VRAM**: Détection automatique de la mémoire vidéo (NVIDIA via nvidia-smi, AMD via sysfs, macOS via system_profiler)
- **Recommandation de modèle**: Suggestion automatique du meilleur modèle Whisper selon la VRAM disponible (marge de sécurité 30%)
- **Auto-désactivation GPU**: Fallback automatique vers CPU après 3 échecs GPU consécutifs
- **Rechargement à chaud GPU**: Changement GPU/CPU sans redémarrage de l'application

### Changed

- **Greedy sur GPU**: Utilisation de Greedy decoding sur GPU (réduit VRAM ~60%) vs BeamSearch sur CPU
- **Interface GPU en français**: Section GPU complète dans les paramètres (VRAM, recommandations, alertes)
- **Migration config automatique**: Les anciennes configs avec `use_gpu: false` sont auto-migrées si GPU compilé

### Fixed

- **Build CI**: Correction du build TypeScript strict (`noUnusedLocals`) pour les bindings auto-générés

## [1.0.7] - 2026-02-28

### Fixed

- **Validation d'entrée**: Validation des noms de périphériques avant les commandes pactl (prévention injection)
- **Sécurité Mutex**: Gestion du poisoning Mutex dans le callback audio et UInput
- **Ordre des locks**: Séparation des scopes dans save_settings pour éviter les deadlocks
- **Rechargement à chaud**: Application des changements modèle/langue dans save_settings sans redémarrage

## [1.0.6] - 2026-02-20

### Fixed

- **Détection Bluetooth A2DP**: Détection des casques BT via `pactl list cards` (visible même en mode A2DP)
- **Fermeture auto stream BT**: Fermeture du stream audio après 10s d'inactivité uniquement pour le Bluetooth (libère le profil HFP)
- **UInput singleton**: Prévention de l'accumulation de `/dev/input/eventN`
- **PTT instantané**: Les périphériques non-BT (ALSA, USB) gardent le stream ouvert (en pause) pour un PTT sans délai

## [1.0.5] - 2026-02-15

### Fixed

- **PTT instantané**: Stream audio maintenu ouvert (en pause) entre les enregistrements
- **Pré-ouverture stream**: Le stream audio s'ouvre à l'init STT pour éviter le délai de 2s au premier appui
- **Paste terminal VS Code**: Détection du terminal VS Code via `${focusedView}` dans le titre de fenêtre
- **wl-copy non bloquant**: Utilisation de spawn non-bloquant pour éviter le freeze

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
