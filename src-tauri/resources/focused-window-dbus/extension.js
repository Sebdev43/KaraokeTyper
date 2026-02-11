/**
 * Focused Window D-Bus Extension for KaraokeTyper
 *
 * GNOME 45+ compatible (ESM class-based extension)
 *
 * Exposes the focused window's WM_CLASS via D-Bus so that
 * KaraokeTyper can detect if a terminal is focused and use
 * the correct paste shortcut (Ctrl+Shift+V vs Ctrl+V).
 *
 * D-Bus Interface: org.karaoketyper.FocusedWindow
 * Object Path: /org/karaoketyper/FocusedWindow
 * Method: GetFocusedWindow() -> (s wm_class, s title)
 * Method: IsTerminal() -> (b is_terminal)
 */

import Gio from 'gi://Gio';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const DBUS_INTERFACE = `
<node>
  <interface name="org.karaoketyper.FocusedWindow">
    <method name="GetFocusedWindow">
      <arg type="s" direction="out" name="wm_class"/>
      <arg type="s" direction="out" name="title"/>
    </method>
    <method name="IsTerminal">
      <arg type="b" direction="out" name="is_terminal"/>
    </method>
  </interface>
</node>`;

// Common terminal WM_CLASS patterns
const TERMINAL_CLASSES = [
    'gnome-terminal',
    'gnome-terminal-server',
    'org.gnome.Terminal',
    'konsole',
    'xterm',
    'urxvt',
    'rxvt',
    'terminator',
    'tilix',
    'alacritty',
    'kitty',
    'wezterm',
    'foot',
    'xfce4-terminal',
    'mate-terminal',
    'lxterminal',
    'qterminal',
    'terminology',
    'cool-retro-term',
    'guake',
    'tilda',
    'yakuake',
    'hyper',
    'tabby',
    'warp',
    'iterm2',
    'terminal',
    'blackbox',
    'ptyxis',
    'ghostty',
];

// Editors with integrated terminals — need title-based detection
// When the terminal pane is focused, the window title starts with the shell/process name
const CODE_EDITOR_CLASSES = ['code', 'code-insiders', 'code-oss', 'codium', 'cursor'];

// Shell/process names that appear in VS Code title when terminal is focused
const TERMINAL_TITLE_PATTERNS = [
    'bash', 'zsh', 'fish', 'sh', 'csh', 'tcsh', 'ksh', 'dash',
    'python', 'python3', 'node', 'npm', 'npx', 'cargo', 'ruby', 'perl',
    'claude', 'claude code', 'htop', 'top', 'vim', 'nvim', 'nano',
    'ssh', 'docker', 'podman', 'make', 'git',
    'powershell', 'pwsh', 'cmd',
];

class FocusedWindowDBus {
    constructor() {
        this._dbusImpl = Gio.DBusExportedObject.wrapJSObject(DBUS_INTERFACE, this);
    }

    GetFocusedWindow() {
        const focusedWindow = global.display.focus_window;

        if (!focusedWindow) {
            return ['', ''];
        }

        const wmClass = focusedWindow.get_wm_class() || '';
        const title = focusedWindow.get_title() || '';

        return [wmClass, title];
    }

    IsTerminal() {
        const [wmClass, title] = this.GetFocusedWindow();

        if (!wmClass) {
            return false;
        }

        const wmClassLower = wmClass.toLowerCase();

        // Check if WM_CLASS matches any known terminal
        // Supports exact match and reverse-domain format (e.g. com.mitchellh.ghostty)
        for (const termClass of TERMINAL_CLASSES) {
            const tc = termClass.toLowerCase();
            if (wmClassLower === tc || wmClassLower.endsWith('.' + tc)) {
                return true;
            }
        }

        // Check if it's a code editor with integrated terminal
        // Detect terminal focus via window title heuristic
        for (const editorClass of CODE_EDITOR_CLASSES) {
            if (wmClassLower === editorClass.toLowerCase()) {
                // VS Code title format:
                //   Editor: "filename.ext - workspace - Visual Studio Code"
                //   Terminal: "bash - workspace - Visual Studio Code"
                // Extract the first segment (before first " - ")
                const firstSegment = (title || '').split(' \u2014 ')[0].split(' - ')[0].trim().toLowerCase();

                for (const pattern of TERMINAL_TITLE_PATTERNS) {
                    if (firstSegment === pattern || firstSegment.startsWith(pattern + ':')) {
                        return true;
                    }
                }

                return false;
            }
        }

        return false;
    }

    export() {
        // Export the object on GNOME Shell's bus
        // Extensions cannot create their own D-Bus services
        this._dbusImpl.export(
            Gio.DBus.session,
            '/org/karaoketyper/FocusedWindow'
        );
        console.log('[KaraokeTyper] D-Bus object exported at /org/karaoketyper/FocusedWindow');
    }

    unexport() {
        this._dbusImpl.unexport();
    }
}

// GNOME 45+ Extension class
export default class FocusedWindowExtension extends Extension {
    enable() {
        this._dbusService = new FocusedWindowDBus();
        this._dbusService.export();
        console.log('[KaraokeTyper] Focused Window D-Bus service enabled');
    }

    disable() {
        if (this._dbusService) {
            this._dbusService.unexport();
            this._dbusService = null;
        }
        console.log('[KaraokeTyper] Focused Window D-Bus service disabled');
    }
}
