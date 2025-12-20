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
    'st',
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
        const [wmClass, _title] = this.GetFocusedWindow();

        if (!wmClass) {
            return false;
        }

        const wmClassLower = wmClass.toLowerCase();

        // Check if WM_CLASS matches any known terminal
        for (const termClass of TERMINAL_CLASSES) {
            if (wmClassLower.includes(termClass.toLowerCase())) {
                return true;
            }
        }

        return false;
    }

    export() {
        this._dbusImpl.export(
            Gio.DBus.session,
            '/org/karaoketyper/FocusedWindow'
        );
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
