/**
 * Focused Window D-Bus Extension for KaraokeTyper
 *
 * Exposes the focused window's WM_CLASS via D-Bus so that
 * KaraokeTyper can detect if a terminal is focused and use
 * the correct paste shortcut (Ctrl+Shift+V vs Ctrl+V).
 *
 * D-Bus Interface: org.karaoketyper.FocusedWindow
 * Object Path: /org/karaoketyper/FocusedWindow
 * Method: GetFocusedWindow() -> (s wm_class, s title)
 */

import Gio from 'gi://Gio';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

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

let dbusService = null;

export function enable() {
    dbusService = new FocusedWindowDBus();
    dbusService.export();
    console.log('[KaraokeTyper] Focused Window D-Bus service enabled');
}

export function disable() {
    if (dbusService) {
        dbusService.unexport();
        dbusService = null;
    }
    console.log('[KaraokeTyper] Focused Window D-Bus service disabled');
}
