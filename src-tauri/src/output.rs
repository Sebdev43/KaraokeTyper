use arboard::Clipboard;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux_output {
    use std::collections::HashMap;
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;
    use std::sync::OnceLock;
    use std::thread;
    use std::time::Duration;

    const UI_SET_EVBIT: u64 = 0x40045564;
    const UI_SET_KEYBIT: u64 = 0x40045565;
    const UI_DEV_SETUP: u64 = 0x405c5503;
    const UI_DEV_CREATE: u64 = 0x5501;
    const UI_DEV_DESTROY: u64 = 0x5502;

    const EV_SYN: u16 = 0x00;
    const EV_KEY: u16 = 0x01;
    const SYN_REPORT: u16 = 0x00;

    const KEY_LEFTCTRL: u16 = 0x1d;
    const KEY_LEFTSHIFT: u16 = 0x2a;
    const KEY_V: u16 = 0x2f;
    const KEY_SPACE: u16 = 0x39;
    const KEY_ENTER: u16 = 0x1c;
    const KEY_BACKSPACE: u16 = 0x0e;
    const KEY_TAB: u16 = 0x0f;

    const KEY_1: u16 = 0x02;
    const KEY_2: u16 = 0x03;
    const KEY_3: u16 = 0x04;
    const KEY_4: u16 = 0x05;
    const KEY_5: u16 = 0x06;
    const KEY_6: u16 = 0x07;
    const KEY_7: u16 = 0x08;
    const KEY_8: u16 = 0x09;
    const KEY_9: u16 = 0x0a;
    const KEY_0: u16 = 0x0b;

    const KEY_Q: u16 = 0x10;
    const KEY_W: u16 = 0x11;
    const KEY_E: u16 = 0x12;
    const KEY_R: u16 = 0x13;
    const KEY_T: u16 = 0x14;
    const KEY_Y: u16 = 0x15;
    const KEY_U: u16 = 0x16;
    const KEY_I: u16 = 0x17;
    const KEY_O: u16 = 0x18;
    const KEY_P: u16 = 0x19;
    const KEY_A: u16 = 0x1e;
    const KEY_S: u16 = 0x1f;
    const KEY_D: u16 = 0x20;
    const KEY_F: u16 = 0x21;
    const KEY_G: u16 = 0x22;
    const KEY_H: u16 = 0x23;
    const KEY_J: u16 = 0x24;
    const KEY_K: u16 = 0x25;
    const KEY_L: u16 = 0x26;
    const KEY_Z: u16 = 0x2c;
    const KEY_X: u16 = 0x2d;
    const KEY_C: u16 = 0x2e;
    const KEY_B: u16 = 0x30;
    const KEY_N: u16 = 0x31;
    const KEY_M: u16 = 0x32;

    const KEY_MINUS: u16 = 0x0c;
    const KEY_EQUAL: u16 = 0x0d;
    const KEY_LEFTBRACE: u16 = 0x1a;
    const KEY_RIGHTBRACE: u16 = 0x1b;
    const KEY_SEMICOLON: u16 = 0x27;
    const KEY_APOSTROPHE: u16 = 0x28;
    const KEY_GRAVE: u16 = 0x29;
    const KEY_BACKSLASH: u16 = 0x2b;
    const KEY_COMMA: u16 = 0x33;
    const KEY_DOT: u16 = 0x34;
    const KEY_SLASH: u16 = 0x35;

    #[repr(C)]
    struct InputId {
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
    }

    #[repr(C)]
    struct UinputSetup {
        id: InputId,
        name: [u8; 80],
        ff_effects_max: u32,
    }

    #[repr(C)]
    struct InputEvent {
        tv_sec: i64,
        tv_usec: i64,
        type_: u16,
        code: u16,
        value: i32,
    }

    // Thread-safe keymap using OnceLock (no unsafe needed)
    static CHAR_TO_KEY: OnceLock<HashMap<char, (u16, bool)>> = OnceLock::new();

    fn get_keymap() -> &'static HashMap<char, (u16, bool)> {
        CHAR_TO_KEY.get_or_init(|| {
            let mut map = HashMap::new();

            // Lowercase letters
            map.insert('a', (KEY_A, false));
            map.insert('b', (KEY_B, false));
            map.insert('c', (KEY_C, false));
            map.insert('d', (KEY_D, false));
            map.insert('e', (KEY_E, false));
            map.insert('f', (KEY_F, false));
            map.insert('g', (KEY_G, false));
            map.insert('h', (KEY_H, false));
            map.insert('i', (KEY_I, false));
            map.insert('j', (KEY_J, false));
            map.insert('k', (KEY_K, false));
            map.insert('l', (KEY_L, false));
            map.insert('m', (KEY_M, false));
            map.insert('n', (KEY_N, false));
            map.insert('o', (KEY_O, false));
            map.insert('p', (KEY_P, false));
            map.insert('q', (KEY_Q, false));
            map.insert('r', (KEY_R, false));
            map.insert('s', (KEY_S, false));
            map.insert('t', (KEY_T, false));
            map.insert('u', (KEY_U, false));
            map.insert('v', (KEY_V, false));
            map.insert('w', (KEY_W, false));
            map.insert('x', (KEY_X, false));
            map.insert('y', (KEY_Y, false));
            map.insert('z', (KEY_Z, false));

            // Uppercase letters (shift required)
            map.insert('A', (KEY_A, true));
            map.insert('B', (KEY_B, true));
            map.insert('C', (KEY_C, true));
            map.insert('D', (KEY_D, true));
            map.insert('E', (KEY_E, true));
            map.insert('F', (KEY_F, true));
            map.insert('G', (KEY_G, true));
            map.insert('H', (KEY_H, true));
            map.insert('I', (KEY_I, true));
            map.insert('J', (KEY_J, true));
            map.insert('K', (KEY_K, true));
            map.insert('L', (KEY_L, true));
            map.insert('M', (KEY_M, true));
            map.insert('N', (KEY_N, true));
            map.insert('O', (KEY_O, true));
            map.insert('P', (KEY_P, true));
            map.insert('Q', (KEY_Q, true));
            map.insert('R', (KEY_R, true));
            map.insert('S', (KEY_S, true));
            map.insert('T', (KEY_T, true));
            map.insert('U', (KEY_U, true));
            map.insert('V', (KEY_V, true));
            map.insert('W', (KEY_W, true));
            map.insert('X', (KEY_X, true));
            map.insert('Y', (KEY_Y, true));
            map.insert('Z', (KEY_Z, true));

            // Numbers
            map.insert('1', (KEY_1, false));
            map.insert('2', (KEY_2, false));
            map.insert('3', (KEY_3, false));
            map.insert('4', (KEY_4, false));
            map.insert('5', (KEY_5, false));
            map.insert('6', (KEY_6, false));
            map.insert('7', (KEY_7, false));
            map.insert('8', (KEY_8, false));
            map.insert('9', (KEY_9, false));
            map.insert('0', (KEY_0, false));

            // Special characters
            map.insert(' ', (KEY_SPACE, false));
            map.insert('\n', (KEY_ENTER, false));
            map.insert('\t', (KEY_TAB, false));
            map.insert('-', (KEY_MINUS, false));
            map.insert('=', (KEY_EQUAL, false));
            map.insert('[', (KEY_LEFTBRACE, false));
            map.insert(']', (KEY_RIGHTBRACE, false));
            map.insert(';', (KEY_SEMICOLON, false));
            map.insert('\'', (KEY_APOSTROPHE, false));
            map.insert('`', (KEY_GRAVE, false));
            map.insert('\\', (KEY_BACKSLASH, false));
            map.insert(',', (KEY_COMMA, false));
            map.insert('.', (KEY_DOT, false));
            map.insert('/', (KEY_SLASH, false));

            // Shifted symbols
            map.insert('!', (KEY_1, true));
            map.insert('@', (KEY_2, true));
            map.insert('#', (KEY_3, true));
            map.insert('$', (KEY_4, true));
            map.insert('%', (KEY_5, true));
            map.insert('^', (KEY_6, true));
            map.insert('&', (KEY_7, true));
            map.insert('*', (KEY_8, true));
            map.insert('(', (KEY_9, true));
            map.insert(')', (KEY_0, true));
            map.insert('_', (KEY_MINUS, true));
            map.insert('+', (KEY_EQUAL, true));
            map.insert('{', (KEY_LEFTBRACE, true));
            map.insert('}', (KEY_RIGHTBRACE, true));
            map.insert(':', (KEY_SEMICOLON, true));
            map.insert('"', (KEY_APOSTROPHE, true));
            map.insert('~', (KEY_GRAVE, true));
            map.insert('|', (KEY_BACKSLASH, true));
            map.insert('<', (KEY_COMMA, true));
            map.insert('>', (KEY_DOT, true));
            map.insert('?', (KEY_SLASH, true));

            map
        })
    }

    fn get_key_for_char(c: char) -> Option<(u16, bool)> {
        get_keymap().get(&c).copied()
    }

    pub struct UInputDevice {
        fd: i32,
    }

    impl UInputDevice {
        pub fn new() -> Result<Self, String> {
            println!("[UINPUT] Opening /dev/uinput...");

            let file = OpenOptions::new()
                .write(true)
                .open("/dev/uinput")
                .map_err(|e| format!("Failed to open /dev/uinput: {}. Make sure you have permissions (add user to 'input' group)", e))?;

            let fd = file.as_raw_fd();
            std::mem::forget(file); // Keep file descriptor open

            // Enable EV_KEY events
            let ret = unsafe { libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as i32) };
            if ret < 0 {
                return Err(format!(
                    "Failed to set EV_KEY bit: {}",
                    std::io::Error::last_os_error()
                ));
            }

            // Enable all keys we need
            let keys = [
                KEY_LEFTCTRL,
                KEY_LEFTSHIFT,
                KEY_V,
                KEY_SPACE,
                KEY_ENTER,
                KEY_BACKSPACE,
                KEY_TAB,
                KEY_1,
                KEY_2,
                KEY_3,
                KEY_4,
                KEY_5,
                KEY_6,
                KEY_7,
                KEY_8,
                KEY_9,
                KEY_0,
                KEY_Q,
                KEY_W,
                KEY_E,
                KEY_R,
                KEY_T,
                KEY_Y,
                KEY_U,
                KEY_I,
                KEY_O,
                KEY_P,
                KEY_A,
                KEY_S,
                KEY_D,
                KEY_F,
                KEY_G,
                KEY_H,
                KEY_J,
                KEY_K,
                KEY_L,
                KEY_Z,
                KEY_X,
                KEY_C,
                KEY_B,
                KEY_N,
                KEY_M,
                KEY_MINUS,
                KEY_EQUAL,
                KEY_LEFTBRACE,
                KEY_RIGHTBRACE,
                KEY_SEMICOLON,
                KEY_APOSTROPHE,
                KEY_GRAVE,
                KEY_BACKSLASH,
                KEY_COMMA,
                KEY_DOT,
                KEY_SLASH,
            ];

            for key in keys {
                let ret = unsafe { libc::ioctl(fd, UI_SET_KEYBIT, key as i32) };
                if ret < 0 {
                    return Err(format!(
                        "Failed to set key bit {}: {}",
                        key,
                        std::io::Error::last_os_error()
                    ));
                }
            }

            // Setup device info
            let mut setup = UinputSetup {
                id: InputId {
                    bustype: 0x03, // BUS_USB
                    vendor: 0x1234,
                    product: 0x5678,
                    version: 0x0001,
                },
                name: [0u8; 80],
                ff_effects_max: 0,
            };

            let name = b"KaraokeTyper Virtual Keyboard";
            setup.name[..name.len()].copy_from_slice(name);

            let ret = unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) };
            if ret < 0 {
                return Err(format!(
                    "Failed to setup device: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let ret = unsafe { libc::ioctl(fd, UI_DEV_CREATE, 0) };
            if ret < 0 {
                return Err(format!(
                    "Failed to create device: {}",
                    std::io::Error::last_os_error()
                ));
            }

            // Wait for device to be recognized
            thread::sleep(Duration::from_millis(100));

            println!("[UINPUT] ✅ Virtual keyboard created successfully");

            Ok(Self { fd })
        }

        fn write_event(&self, type_: u16, code: u16, value: i32) -> Result<(), String> {
            let event = InputEvent {
                tv_sec: 0,
                tv_usec: 0,
                type_,
                code,
                value,
            };

            let ptr = &event as *const InputEvent as *const u8;
            let size = std::mem::size_of::<InputEvent>();

            let written = unsafe { libc::write(self.fd, ptr as *const libc::c_void, size) };

            if written as usize != size {
                return Err(format!(
                    "Failed to write event: wrote {} of {} bytes",
                    written, size
                ));
            }

            Ok(())
        }

        fn sync(&self) -> Result<(), String> {
            self.write_event(EV_SYN, SYN_REPORT, 0)
        }

        pub fn press_key(&self, key: u16) -> Result<(), String> {
            self.write_event(EV_KEY, key, 1)?;
            self.sync()
        }

        pub fn release_key(&self, key: u16) -> Result<(), String> {
            self.write_event(EV_KEY, key, 0)?;
            self.sync()
        }

        pub fn tap_key(&self, key: u16) -> Result<(), String> {
            self.press_key(key)?;
            thread::sleep(Duration::from_millis(5));
            self.release_key(key)
        }

        pub fn type_char(&self, c: char) -> Result<(), String> {
            if let Some((key, shift)) = get_key_for_char(c) {
                if shift {
                    self.press_key(KEY_LEFTSHIFT)?;
                    thread::sleep(Duration::from_millis(2));
                }

                self.tap_key(key)?;

                if shift {
                    thread::sleep(Duration::from_millis(2));
                    self.release_key(KEY_LEFTSHIFT)?;
                }

                Ok(())
            } else {
                println!("[UINPUT] Skipping unsupported character: {:?}", c);
                Ok(())
            }
        }

        pub fn type_text(&self, text: &str) -> Result<(), String> {
            for c in text.chars() {
                self.type_char(c)?;
                thread::sleep(Duration::from_millis(8));
            }
            Ok(())
        }

        pub fn ctrl_v(&self) -> Result<(), String> {
            println!("[UINPUT] Sending Ctrl+V...");
            self.press_key(KEY_LEFTCTRL)?;
            thread::sleep(Duration::from_millis(10));
            self.tap_key(KEY_V)?;
            thread::sleep(Duration::from_millis(10));
            self.release_key(KEY_LEFTCTRL)?;
            println!("[UINPUT] ✅ Ctrl+V sent");
            Ok(())
        }
    }

    impl Drop for UInputDevice {
        fn drop(&mut self) {
            if self.fd >= 0 {
                unsafe {
                    libc::ioctl(self.fd, UI_DEV_DESTROY, 0);
                    libc::close(self.fd);
                }
                println!("[UINPUT] Device closed");
            }
        }
    }
}

pub struct TextOutput {
    clipboard: Clipboard,
    key_delay_ms: u64,
}

impl TextOutput {
    pub fn new(key_delay_ms: u64) -> Result<Self, String> {
        println!("[OUTPUT] Initializing text output...");

        let clipboard =
            Clipboard::new().map_err(|e| format!("Failed to create clipboard: {}", e))?;

        println!("[OUTPUT] ✅ Clipboard initialized");

        Ok(Self {
            clipboard,
            key_delay_ms,
        })
    }

    pub fn type_text(&mut self, text: &str) -> Result<(), String> {
        if text.is_empty() {
            println!("[OUTPUT] Text is empty, skipping");
            return Ok(());
        }

        println!("[OUTPUT] Typing text: '{}'", text);

        // ALWAYS use clipboard + paste - this respects keyboard layout (AZERTY, QWERTZ, etc.)
        // ydotool type uses raw scancodes which breaks non-QWERTY layouts
        self.clipboard
            .set_text(text)
            .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;

        println!("[OUTPUT] ✅ Text copied to clipboard");

        if self.key_delay_ms > 0 {
            println!("[OUTPUT] Waiting {}ms before pasting...", self.key_delay_ms);
            thread::sleep(Duration::from_millis(self.key_delay_ms));
        }

        self.simulate_paste()?;

        println!("[OUTPUT] ✅ Text pasted successfully");

        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[allow(dead_code)] // Kept as alternative to clipboard+paste
    fn type_with_ydotool(&self, text: &str) -> Result<(), String> {
        println!("[OUTPUT] Using ydotool type for Wayland...");

        // ydotool type with key-delay for reliability
        let output = std::process::Command::new("ydotool")
            .args(["type", "--key-delay", "8", "--", text])
            .output()
            .map_err(|e| {
                format!(
                    "Failed to run ydotool: {}. Install with: sudo apt install ydotool",
                    e
                )
            })?;

        if output.status.success() {
            println!("[OUTPUT] ✅ ydotool typed text successfully");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            println!("[OUTPUT] ydotool stderr: {}", stderr);
            println!("[OUTPUT] ydotool stdout: {}", stdout);
            Err(format!("ydotool type failed: {}", stderr))
        }
    }

    #[cfg(target_os = "linux")]
    fn simulate_paste(&self) -> Result<(), String> {
        // Check if we're on Wayland
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();

        if session_type == "wayland" {
            // Detect if the focused window is a terminal
            let is_terminal = Self::is_terminal_focused();

            // Try wtype first (better Wayland integration), then fall back to ydotool
            if Self::try_wtype_paste(is_terminal)? {
                return Ok(());
            }

            // Fallback to ydotool
            Self::try_ydotool_paste(is_terminal)
        } else {
            // Use uinput for X11
            println!("[OUTPUT] Using uinput for X11 paste...");
            let device = linux_output::UInputDevice::new()?;
            device.ctrl_v()
        }
    }

    #[cfg(target_os = "linux")]
    fn is_terminal_focused() -> bool {
        // Method 1: Try our KaraokeTyper GNOME extension first (most reliable on GNOME 46+)
        if let Some(is_terminal) = Self::check_terminal_via_extension() {
            return is_terminal;
        }

        // Method 2: Fallback to GNOME Shell Eval (blocked on GNOME 46+ but works on older versions)
        if let Some(is_terminal) = Self::check_terminal_via_gnome_eval() {
            return is_terminal;
        }

        // Default: assume not a terminal (use Ctrl+V)
        println!("[OUTPUT] Could not detect window type, using Ctrl+V");
        false
    }

    #[cfg(target_os = "linux")]
    fn check_terminal_via_extension() -> Option<bool> {
        // Call our KaraokeTyper GNOME extension via D-Bus
        let result = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.karaoketyper.FocusedWindow",
                "--object-path",
                "/org/karaoketyper/FocusedWindow",
                "--method",
                "org.karaoketyper.FocusedWindow.IsTerminal",
            ])
            .output();

        if let Ok(output) = result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // D-Bus returns "(true,)" or "(false,)"
                if stdout.contains("true") {
                    println!("[OUTPUT] 🖥️ Extension detected: terminal focused");
                    return Some(true);
                } else if stdout.contains("false") {
                    println!("[OUTPUT] 📝 Extension detected: not a terminal");
                    return Some(false);
                }
            }
        }

        // Extension not available or error
        None
    }

    #[cfg(target_os = "linux")]
    fn check_terminal_via_gnome_eval() -> Option<bool> {
        // Fallback: Try GNOME Shell.Eval (blocked on GNOME 46+ for security)
        let result = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell",
                "--object-path",
                "/org/gnome/Shell",
                "--method",
                "org.gnome.Shell.Eval",
                "global.display.focus_window ? global.display.focus_window.get_wm_class() : ''",
            ])
            .output();

        if let Ok(output) = result {
            let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
            // Check if we got a valid response (not empty or error)
            if stdout.contains("(true,") {
                // Common terminal emulator WM classes
                let terminals = [
                    "gnome-terminal",
                    "konsole",
                    "xterm",
                    "alacritty",
                    "kitty",
                    "tilix",
                    "terminator",
                    "urxvt",
                    "st",
                    "foot",
                    "wezterm",
                    "terminal",
                    "term",
                    "blackbox",
                    "ptyxis",
                ];

                for term in terminals {
                    if stdout.contains(term) {
                        println!("[OUTPUT] 🖥️ GNOME Eval detected terminal: {}", term);
                        return Some(true);
                    }
                }

                // Got a response but it's not a terminal
                return Some(false);
            }
        }

        // GNOME Eval not available or blocked
        None
    }

    #[cfg(target_os = "linux")]
    fn try_wtype_paste(is_terminal: bool) -> Result<bool, String> {
        // Check if wtype is available
        if std::process::Command::new("which")
            .arg("wtype")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            let shortcut = if is_terminal {
                println!("[OUTPUT] Using wtype for terminal paste (Ctrl+Shift+V)...");
                vec!["-M", "ctrl", "-M", "shift", "-k", "v"]
            } else {
                println!("[OUTPUT] Using wtype for paste (Ctrl+V)...");
                vec!["-M", "ctrl", "-k", "v"]
            };

            let output = std::process::Command::new("wtype")
                .args(&shortcut)
                .output()
                .map_err(|e| format!("Failed to run wtype: {}", e))?;

            if output.status.success() {
                println!("[OUTPUT] ✅ wtype paste sent successfully");
                return Ok(true);
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!("[OUTPUT] ⚠️ wtype failed: {}, trying ydotool...", stderr);
            }
        }

        Ok(false)
    }

    #[cfg(target_os = "linux")]
    fn try_ydotool_paste(is_terminal: bool) -> Result<(), String> {
        let shortcut = if is_terminal {
            println!("[OUTPUT] Using ydotool for terminal paste (Ctrl+Shift+V)...");
            "ctrl+shift+v"
        } else {
            println!("[OUTPUT] Using ydotool for paste (Ctrl+V)...");
            "ctrl+v"
        };

        let output = std::process::Command::new("ydotool")
            .args(["key", shortcut])
            .output()
            .map_err(|e| {
                format!(
                    "Failed to run ydotool: {}. Install with: sudo apt install ydotool",
                    e
                )
            })?;

        if output.status.success() {
            println!("[OUTPUT] ✅ ydotool paste sent successfully");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("ydotool failed: {}", stderr))
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn simulate_paste(&self) -> Result<(), String> {
        use enigo::{Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to create enigo instance: {}", e))?;

        println!("[OUTPUT] Simulating paste shortcut with enigo...");

        #[cfg(target_os = "macos")]
        let modifier = Key::Meta;
        #[cfg(not(target_os = "macos"))]
        let modifier = Key::Control;

        enigo
            .key(modifier, enigo::Direction::Press)
            .map_err(|e| format!("Failed to press modifier: {}", e))?;
        thread::sleep(Duration::from_millis(10));
        enigo
            .key(Key::Unicode('v'), enigo::Direction::Click)
            .map_err(|e| format!("Failed to press V: {}", e))?;
        thread::sleep(Duration::from_millis(10));
        enigo
            .key(modifier, enigo::Direction::Release)
            .map_err(|e| format!("Failed to release modifier: {}", e))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn type_text_direct(&mut self, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }

        println!("[OUTPUT] Typing text directly: '{}'", text);

        #[cfg(target_os = "linux")]
        {
            let device = linux_output::UInputDevice::new()?;
            device.type_text(text)?;
        }

        #[cfg(not(target_os = "linux"))]
        {
            use enigo::{Enigo, Keyboard, Settings};
            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|e| format!("Failed to create enigo: {}", e))?;
            enigo
                .text(text)
                .map_err(|e| format!("Failed to type text: {}", e))?;
        }

        println!("[OUTPUT] ✅ Text typed successfully");
        Ok(())
    }
}
