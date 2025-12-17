#!/bin/bash
# Install system dependencies for KaraokeTyper

set -e

echo "🎤 KaraokeTyper - System Dependencies Installer"
echo "==============================================="
echo ""

# Detect package manager
if command -v apt &> /dev/null; then
    PKG_MANAGER="apt"
    INSTALL_CMD="sudo apt install -y"
elif command -v dnf &> /dev/null; then
    PKG_MANAGER="dnf"
    INSTALL_CMD="sudo dnf install -y"
elif command -v pacman &> /dev/null; then
    PKG_MANAGER="pacman"
    INSTALL_CMD="sudo pacman -S --noconfirm"
else
    echo "❌ Could not detect package manager (apt/dnf/pacman)"
    exit 1
fi

echo "Detected package manager: $PKG_MANAGER"
echo ""

# Install dependencies based on package manager
case $PKG_MANAGER in
    apt)
        echo "Installing dependencies for Debian/Ubuntu..."
        $INSTALL_CMD \
            build-essential \
            cmake \
            pkg-config \
            libasound2-dev \
            libwebkit2gtk-4.1-dev \
            librsvg2-dev \
            libayatana-appindicator3-dev \
            libgtk-3-dev \
            libpango1.0-dev \
            libjavascriptcoregtk-4.1-dev \
            libgdk-pixbuf2.0-dev \
            libsoup-3.0-dev \
            clang
        ;;
    dnf)
        echo "Installing dependencies for Fedora/RHEL..."
        $INSTALL_CMD \
            pkgconf-pkg-config \
            alsa-lib-devel \
            webkit2gtk4.1-devel \
            librsvg2-devel \
            libappindicator-gtk3-devel
        ;;
    pacman)
        echo "Installing dependencies for Arch Linux..."
        $INSTALL_CMD \
            pkgconf \
            alsa-lib \
            webkit2gtk-4.1 \
            librsvg \
            libappindicator-gtk3
        ;;
esac

echo ""
echo "✓ All dependencies installed successfully!"
echo ""
echo "You can now build KaraokeTyper with:"
echo "  npm run tauri dev    # Development mode"
echo "  npm run tauri build  # Production build"
