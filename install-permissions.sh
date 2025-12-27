#!/bin/bash

set -e

RULES_FILE="/etc/udev/rules.d/70-karaoketyper.rules"

echo "KaraokeTyper - Installing permissions for push-to-talk..."
echo ""

# Get the real user (not root from pkexec)
if [ -n "$PKEXEC_UID" ]; then
    REAL_USER=$(id -un "$PKEXEC_UID")
else
    REAL_USER="$USER"
fi

echo "Configuring permissions for user: $REAL_USER"
echo ""

# 1. Add user to 'input' group (for text typing via uinput)
echo "[1/3] Adding user to 'input' group..."
if usermod -a -G input "$REAL_USER"; then
    echo "✅ User added to 'input' group"
else
    echo "❌ Failed to add user to 'input' group"
    exit 1
fi

# 2. Create udev rules (for keyboard event detection)
echo ""
echo "[2/3] Creating udev rules..."
RULES_CONTENT='# KaraokeTyper - Allow access to keyboard input devices for push-to-talk
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"
'

echo "$RULES_CONTENT" | tee "$RULES_FILE" > /dev/null

if [ $? -eq 0 ]; then
    echo "✅ Udev rules created successfully"
else
    echo "❌ Failed to create udev rules"
    exit 1
fi

# 3. Reload udev rules
echo ""
echo "[3/3] Reloading udev rules..."
udevadm control --reload-rules
udevadm trigger

if [ $? -eq 0 ]; then
    echo "✅ Udev rules reloaded successfully"
else
    echo "⚠️  Warning: Failed to reload udev rules"
    exit 1
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Installation complete!"
echo ""
echo "KaraokeTyper will now restart automatically."
echo "After restart, push-to-talk will work with Ctrl+Space."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
