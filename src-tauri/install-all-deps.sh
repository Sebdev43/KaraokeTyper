#!/bin/bash
# Installation complète des dépendances pour KaraokeTyper

set -e

echo "🎤 Installation complète des dépendances KaraokeTyper"
echo "======================================================"
echo ""

# Installer toutes les dépendances système
echo "📦 Installation des dépendances système..."
sudo apt install -y \
    build-essential \
    cmake \
    clang \
    pkg-config \
    libasound2-dev \
    libwebkit2gtk-4.1-dev \
    librsvg2-dev \
    libayatana-appindicator3-dev \
    libgtk-3-dev \
    libpango1.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libgdk-pixbuf2.0-dev \
    libsoup-3.0-dev

echo ""
echo "✓ Dépendances système installées!"
echo ""

# Vérifier Rust
if ! command -v cargo &> /dev/null; then
    echo "🦀 Installation de Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "✓ Rust installé!"
else
    echo "✓ Rust déjà installé"
fi

echo ""
echo "======================================================"
echo "✅ Installation terminée!"
echo ""
echo "Vous pouvez maintenant lancer l'application avec:"
echo "  source \$HOME/.cargo/env"
echo "  npm run tauri dev"
