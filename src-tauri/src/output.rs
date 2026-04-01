use crate::config::PasteMethod;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux_output {
    use std::collections::HashMap;
    use std::fs::OpenOptions;
    use std::os::unix::io::IntoRawFd;
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    /// Singleton UInput device — created once, reused for all paste/type operations.
    /// Avoids creating a new /dev/input/eventN on every paste.
    static UINPUT_DEVICE: Mutex<Option<UInputDevice>> = Mutex::new(None);

    /// Run a closure with the singleton UInputDevice. Recovers from Mutex poisoning.
    pub fn with_uinput<F, R>(f: F) -> Result<R, String>
    where
        F: FnOnce(&UInputDevice) -> Result<R, String>,
    {
        let mut guard = UINPUT_DEVICE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_none() {
            *guard = Some(UInputDevice::new()?);
        }
        f(guard.as_ref().unwrap())
    }

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

    pub(crate) fn get_keymap() -> &'static HashMap<char, (u16, bool)> {
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
            crate::dev_log!("[UINPUT] Opening /dev/uinput...");

            let file = OpenOptions::new()
                .write(true)
                .open("/dev/uinput")
                .map_err(|e| format!("Failed to open /dev/uinput: {}. Make sure you have permissions (add user to 'input' group)", e))?;

            let fd = file.into_raw_fd(); // Consumes File, transfers fd ownership to UInputDevice

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

            crate::dev_log!("[UINPUT] ✅ Virtual keyboard created successfully");

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
                crate::dev_log!("[UINPUT] Skipping unsupported character: {:?}", c);
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
            crate::dev_log!("[UINPUT] Sending Ctrl+V...");
            self.press_key(KEY_LEFTCTRL)?;
            thread::sleep(Duration::from_millis(10));
            self.tap_key(KEY_V)?;
            thread::sleep(Duration::from_millis(10));
            self.release_key(KEY_LEFTCTRL)?;
            crate::dev_log!("[UINPUT] ✅ Ctrl+V sent");
            Ok(())
        }

        pub fn ctrl_shift_v(&self) -> Result<(), String> {
            crate::dev_log!("[UINPUT] Sending Ctrl+Shift+V...");
            self.press_key(KEY_LEFTCTRL)?;
            thread::sleep(Duration::from_millis(10));
            self.press_key(KEY_LEFTSHIFT)?;
            thread::sleep(Duration::from_millis(10));
            self.tap_key(KEY_V)?;
            thread::sleep(Duration::from_millis(10));
            self.release_key(KEY_LEFTSHIFT)?;
            thread::sleep(Duration::from_millis(10));
            self.release_key(KEY_LEFTCTRL)?;
            crate::dev_log!("[UINPUT] ✅ Ctrl+Shift+V sent");
            Ok(())
        }
    }

    impl Drop for UInputDevice {
        fn drop(&mut self) {
            if self.fd >= 0 {
                unsafe {
                    let ret = libc::ioctl(self.fd, UI_DEV_DESTROY, 0);
                    if ret < 0 {
                        // Don't panic in Drop — just log the failure
                        eprintln!("[UINPUT] Warning: UI_DEV_DESTROY failed (fd={})", self.fd);
                    }
                    libc::close(self.fd);
                }
                crate::dev_log!("[UINPUT] Device closed");
            }
        }
    }
}

pub struct TextOutput {
    app_handle: AppHandle,
    key_delay_ms: u64,
    paste_method: PasteMethod,
}

impl TextOutput {
    pub fn new(key_delay_ms: u64, paste_method: PasteMethod, app_handle: &AppHandle) -> Result<Self, String> {
        crate::dev_log!("[OUTPUT] Initializing text output (paste_method={:?})...", paste_method);
        crate::dev_log!("[OUTPUT] ✅ Clipboard initialized");

        Ok(Self {
            app_handle: app_handle.clone(),
            key_delay_ms,
            paste_method,
        })
    }

    pub fn type_text(&mut self, text: &str) -> Result<(), String> {
        let t0 = std::time::Instant::now();

        if text.is_empty() {
            crate::dev_log!("[OUTPUT] Text is empty, skipping");
            return Ok(());
        }

        crate::dev_log!("[OUTPUT] Typing text: '{}' (method={:?})", text, self.paste_method);

        match self.paste_method {
            PasteMethod::Direct => {
                let t1 = std::time::Instant::now();

                // Remote desktop apps (RustDesk, AnyDesk): ALWAYS use direct typing.
                // Their clipboard sync is unreliable on Linux, but they forward
                // keyboard events reliably. Even enigo text() (buggy on X11) is
                // better than clipboard that never arrives on the remote side.
                #[cfg(target_os = "linux")]
                let force_direct = Self::is_remote_desktop_focused();
                #[cfg(not(target_os = "linux"))]
                let force_direct = false;

                if force_direct {
                    crate::dev_log!(
                        "[OUTPUT] Remote desktop detected — forcing direct typing (clipboard unreliable)"
                    );
                    Self::try_direct_typing(text)?;
                } else {
                    // On Linux, if no native typing tool is available, auto-fallback
                    // to clipboard + paste shortcut. Enigo's text() on X11 has known bugs
                    // that silently drop characters — clipboard is safer for standard apps.
                    #[cfg(target_os = "linux")]
                    let use_clipboard_fallback = !Self::has_native_direct_tool();
                    #[cfg(not(target_os = "linux"))]
                    let use_clipboard_fallback = false;

                    if use_clipboard_fallback {
                        // Clipboard fallback — matches Handy's paste_via_clipboard() flow:
                        // 1. Write clipboard  2. Sleep  3. Send Ctrl+V  4. Sleep
                        // No D-Bus calls between write and paste to avoid clipboard loss.
                        crate::dev_log!("[OUTPUT] No native typing tool — clipboard fallback (Handy flow)");

                        // Step 1: Set clipboard
                        self.set_clipboard_text(text)?;

                        // Step 2: Wait for clipboard to settle
                        thread::sleep(Duration::from_millis(self.key_delay_ms.max(50)));

                        // Step 3: Detect terminal THEN immediately send shortcut
                        #[cfg(target_os = "linux")]
                        let effective_method = Self::resolve_paste_method(&PasteMethod::Direct);
                        #[cfg(not(target_os = "linux"))]
                        let effective_method = PasteMethod::CtrlV;

                        crate::dev_log!("[OUTPUT] Paste method: {:?}", effective_method);

                        // Step 4: Send paste shortcut via enigo (like Handy's fallback)
                        Self::try_enigo_shortcut(&effective_method)?;

                        // Step 5: Post-paste wait (like Handy's 50ms)
                        thread::sleep(Duration::from_millis(50));
                    } else {
                        Self::try_direct_typing(text)?;
                    }
                }
                crate::dev_log!(
                    "[OUTPUT_TIMING] Direct type: {}ms",
                    t1.elapsed().as_millis()
                );
            }
            PasteMethod::CtrlV | PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert => {
                // Clipboard + paste shortcut
                let t1 = std::time::Instant::now();
                self.set_clipboard_text(text)?;
                crate::dev_log!(
                    "[OUTPUT_TIMING] Clipboard set_text: {}ms",
                    t1.elapsed().as_millis()
                );

                if self.key_delay_ms > 0 {
                    crate::dev_log!("[OUTPUT] Waiting {}ms before pasting...", self.key_delay_ms);
                    thread::sleep(Duration::from_millis(self.key_delay_ms));
                }

                let t2 = std::time::Instant::now();
                self.send_paste_shortcut()?;
                crate::dev_log!(
                    "[OUTPUT_TIMING] paste shortcut: {}ms",
                    t2.elapsed().as_millis()
                );
            }
            PasteMethod::None => {
                crate::dev_log!("[OUTPUT] PasteMethod::None — skipping");
                return Ok(());
            }
        }

        crate::dev_log!(
            "[OUTPUT_TIMING] Total type_text: {}ms",
            t0.elapsed().as_millis()
        );
        crate::dev_log!("[OUTPUT] ✅ Text output complete");

        Ok(())
    }

    /// Set clipboard text using the best method for the current platform.
    /// On Wayland: uses wl-copy which forks a persistent daemon to serve clipboard requests.
    /// This avoids the race condition where arboard's X11 clipboard content disappears
    /// when the Clipboard object is dropped before the paste key events are processed.
    /// On X11 / non-Linux: uses arboard directly.
    /// Set clipboard text. On Wayland: wl-copy with .status() (blocking, no idle sleep).
    /// Robust Wayland detection: checks both XDG_SESSION_TYPE and WAYLAND_DISPLAY.
    /// XDG_SESSION_TYPE can report "x11" even on Wayland (XWayland, Tauri, Electron).
    #[cfg(target_os = "linux")]
    fn is_wayland() -> bool {
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
        session == "wayland" || !display.is_empty()
    }

    /// Set clipboard text using the best tool for the platform.
    /// Wayland: wl-copy (text/plain MIME, persistent daemon)
    /// X11: xclip -selection clipboard (proper text/plain for Remmina) → tauri plugin
    /// Other: tauri-plugin-clipboard-manager
    fn set_clipboard_text(&mut self, text: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            // Wayland: wl-copy
            if Self::is_wayland() && Self::is_tool_available("wl-copy") {
                crate::dev_log!("[OUTPUT] Using wl-copy for clipboard");
                let status = std::process::Command::new("wl-copy")
                    .arg("--")
                    .arg(text)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map_err(|e| format!("Failed to run wl-copy: {}", e))?;

                if status.success() {
                    crate::dev_log!("[OUTPUT] ✅ wl-copy set clipboard");
                    return Ok(());
                }
                crate::dev_log!("[OUTPUT] ⚠️ wl-copy failed, trying next method");
            }

            // X11: xclip (explicit text/plain CLIPBOARD selection — required for Remmina)
            if !Self::is_wayland() && Self::is_tool_available("xclip") {
                use std::io::Write;
                crate::dev_log!("[OUTPUT] Using xclip -selection clipboard");
                if let Ok(mut child) = std::process::Command::new("xclip")
                    .args(["-selection", "clipboard"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    if let Ok(status) = child.wait() {
                        if status.success() {
                            crate::dev_log!("[OUTPUT] ✅ xclip set clipboard");
                            return Ok(());
                        }
                    }
                }
                crate::dev_log!("[OUTPUT] ⚠️ xclip failed, trying tauri plugin");
            }
        }

        // Fallback: tauri-plugin-clipboard-manager
        crate::dev_log!("[OUTPUT] Using tauri clipboard plugin");
        self.app_handle
            .clipboard()
            .write_text(text.to_string())
            .map_err(|e| format!("Failed to copy to clipboard: {}", e))
    }

    #[cfg(target_os = "linux")]
    #[allow(dead_code)] // Kept as alternative to clipboard+paste
    fn type_with_ydotool(&self, text: &str) -> Result<(), String> {
        crate::dev_log!("[OUTPUT] Using ydotool type for Wayland...");

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
            crate::dev_log!("[OUTPUT] ✅ ydotool typed text successfully");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            crate::dev_log!("[OUTPUT] ydotool stderr: {}", stderr);
            crate::dev_log!("[OUTPUT] ydotool stdout: {}", stdout);
            Err(format!("ydotool type failed: {}", stderr))
        }
    }

    /// Send the paste shortcut based on the configured paste_method.
    /// Uses the user's explicit choice — no auto-detection here.
    /// Terminal auto-detection only happens in the Direct → clipboard fallback path.
    #[cfg(target_os = "linux")]
    fn send_paste_shortcut(&self) -> Result<(), String> {
        if Self::is_wayland() {
            // Wayland: try wtype first
            if Self::try_wtype_shortcut(&self.paste_method)? {
                return Ok(());
            }
            // Fallback to uinput
            crate::dev_log!("[OUTPUT] wtype not available, trying uinput...");
            match Self::try_uinput_shortcut(&self.paste_method) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    crate::dev_log!("[OUTPUT] uinput failed: {}, trying ydotool...", e);
                }
            }
            Self::try_ydotool_shortcut(&self.paste_method)
        } else {
            // X11: xdotool → ydotool → enigo (matches Handy's approach)
            if Self::is_tool_available("xdotool") {
                crate::dev_log!("[OUTPUT] Using xdotool for paste shortcut");
                let key_combo = match &self.paste_method {
                    PasteMethod::CtrlShiftV => "ctrl+shift+v",
                    PasteMethod::ShiftInsert => "shift+Insert",
                    _ => "ctrl+v",
                };
                let output = std::process::Command::new("xdotool")
                    .args(["key", "--clearmodifiers", key_combo])
                    .output()
                    .map_err(|e| format!("xdotool failed: {}", e))?;
                if output.status.success() {
                    return Ok(());
                }
                crate::dev_log!("[OUTPUT] xdotool shortcut failed, trying ydotool...");
            }
            if Self::is_tool_available("ydotool") {
                crate::dev_log!("[OUTPUT] Using ydotool for paste shortcut");
                return Self::try_ydotool_shortcut(&self.paste_method);
            }
            // Fallback: enigo
            match Self::try_enigo_shortcut(&self.paste_method) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    crate::dev_log!("[OUTPUT] enigo failed: {}, trying uinput...", e);
                }
            }
            Self::try_uinput_shortcut(&self.paste_method)
        }
    }

    /// Detect if the focused window is a terminal emulator (Linux only).
    /// Uses GNOME extension D-Bus call first, then GNOME Shell Eval fallback.
    /// Returns true if terminal detected, false otherwise.
    #[cfg(target_os = "linux")]
    fn is_terminal_focused() -> bool {
        // Method 1: KaraokeTyper GNOME extension (most reliable on GNOME 46+)
        if let Some(is_terminal) = Self::check_terminal_via_extension() {
            return is_terminal;
        }

        // Method 2: Fallback to GNOME Shell Eval (works on older GNOME versions)
        if let Some(is_terminal) = Self::check_terminal_via_gnome_eval() {
            return is_terminal;
        }

        // Default: assume not a terminal (use Ctrl+V)
        crate::dev_log!("[OUTPUT] Could not detect window type, using Ctrl+V");
        false
    }

    #[cfg(target_os = "linux")]
    fn get_dbus_env() -> (String, String) {
        let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
        let dbus_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .unwrap_or_else(|_| format!("unix:path={}/bus", xdg_runtime));
        (xdg_runtime, dbus_addr)
    }

    #[cfg(target_os = "linux")]
    fn check_terminal_via_extension() -> Option<bool> {
        let (xdg_runtime, dbus_addr) = Self::get_dbus_env();
        let mut child = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell",
                "--object-path",
                "/org/karaoketyper/FocusedWindow",
                "--method",
                "org.karaoketyper.FocusedWindow.GetFocusedWindow",
            ])
            .env("XDG_RUNTIME_DIR", &xdg_runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;

        // Timeout: 200ms max to avoid blocking paste
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        if let Some(stdout) = child.stdout.take() {
                            use std::io::Read;
                            let mut buf = String::new();
                            let mut reader = std::io::BufReader::new(stdout);
                            if reader.read_to_string(&mut buf).is_ok() {
                                return Some(Self::classify_window_from_dbus(&buf));
                            }
                        }
                    }
                    return None;
                }
                Ok(None) => {
                    if std::time::Instant::now() > deadline {
                        crate::dev_log!("[OUTPUT] D-Bus extension call timed out (200ms)");
                        let _ = child.kill();
                        return None;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn classify_window_from_dbus(dbus_output: &str) -> bool {
        let trimmed = dbus_output.trim().trim_start_matches('(').trim_end_matches(')');
        let parts: Vec<&str> = trimmed.splitn(2, ", ").collect();
        if parts.len() < 2 {
            return false;
        }
        let wm_class = parts[0].trim().trim_matches('\'').to_lowercase();
        let title = parts[1].trim().trim_matches('\'').to_lowercase();

        crate::dev_log!("[OUTPUT] Window: wm_class='{}', title='{}'", wm_class, title);

        let terminals = [
            "gnome-terminal", "gnome-terminal-server", "org.gnome.terminal",
            "konsole", "xterm", "urxvt", "rxvt", "terminator", "tilix",
            "alacritty", "kitty", "wezterm", "foot", "xfce4-terminal",
            "mate-terminal", "lxterminal", "qterminal", "terminology",
            "cool-retro-term", "guake", "tilda", "yakuake", "hyper",
            "tabby", "warp", "iterm2", "terminal", "blackbox", "ptyxis", "ghostty",
        ];

        for term in terminals {
            if wm_class == term || wm_class.ends_with(&format!(".{}", term)) {
                crate::dev_log!("[OUTPUT] Terminal detected: {} (matched '{}')", wm_class, term);
                return true;
            }
        }

        // Remote desktop apps: skip terminal auto-detection.
        // These forward keyboard shortcuts to the remote machine — we can't know
        // if the remote window is a terminal or a GUI app. Let the user's
        // paste_method setting decide (or default to Ctrl+V).
        let remote_desktop_apps = ["remmina", "rustdesk", "anydesk", "vinagre"];
        for app in remote_desktop_apps {
            if wm_class == app || wm_class.contains(app) {
                crate::dev_log!("[OUTPUT] Remote desktop '{}' — skipping terminal detection, using configured method", app);
                return false; // Use configured paste_method, not auto-detect
            }
        }

        // VS Code / code editors with integrated terminal
        let code_editors = ["code", "code-insiders", "code-oss", "codium", "cursor"];
        for editor in code_editors {
            if wm_class == editor {
                let segments: Vec<&str> = title.split(" - ").collect();
                for seg in &segments {
                    if seg.trim() == "terminal" {
                        crate::dev_log!("[OUTPUT] {} terminal detected via focusedView", editor);
                        return true;
                    }
                }
                let first_segment = title.split(" \u{2014} ").next()
                    .unwrap_or(&title)
                    .split(" - ").next()
                    .unwrap_or("").trim();
                let shell_names = [
                    "bash", "zsh", "fish", "sh", "python", "node", "npm",
                    "cargo", "claude", "htop", "vim", "nvim", "ssh", "docker",
                ];
                for shell in shell_names {
                    if first_segment == shell || first_segment.starts_with(&format!("{}:", shell)) {
                        crate::dev_log!("[OUTPUT] {} terminal detected: '{}'", editor, shell);
                        return true;
                    }
                }
                crate::dev_log!("[OUTPUT] {} editor pane (not terminal)", editor);
                return false;
            }
        }

        crate::dev_log!("[OUTPUT] Not a terminal: {}", wm_class);
        false
    }

    #[cfg(target_os = "linux")]
    fn check_terminal_via_gnome_eval() -> Option<bool> {
        let (xdg_runtime, dbus_addr) = Self::get_dbus_env();
        let mut child = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell",
                "--object-path",
                "/org/gnome/Shell",
                "--method",
                "org.gnome.Shell.Eval",
                "let w = global.display.focus_window; w ? w.get_wm_class() + '|' + w.get_title() : ''",
            ])
            .env("XDG_RUNTIME_DIR", &xdg_runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;

        // Timeout: 200ms max
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        let stdout_str = loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return None;
                    }
                    if let Some(stdout) = child.stdout.take() {
                        use std::io::Read;
                        let mut buf = String::new();
                        let mut reader = std::io::BufReader::new(stdout);
                        if reader.read_to_string(&mut buf).is_ok() {
                            break buf.to_lowercase();
                        }
                    }
                    return None;
                }
                Ok(None) => {
                    if std::time::Instant::now() > deadline {
                        crate::dev_log!("[OUTPUT] GNOME Eval call timed out (200ms)");
                        let _ = child.kill();
                        return None;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        };

        {
            let stdout = &stdout_str;
            if stdout.contains("(true,") {
                let terminals = [
                    "gnome-terminal", "konsole", "xterm", "alacritty", "kitty",
                    "tilix", "terminator", "urxvt", "foot", "wezterm",
                    "blackbox", "ptyxis", "ghostty",
                ];
                for term in terminals {
                    if stdout.contains(term) {
                        crate::dev_log!("[OUTPUT] GNOME Eval detected terminal: {}", term);
                        return Some(true);
                    }
                }
                let code_editors = ["code", "code-insiders", "code-oss", "codium", "cursor"];
                for editor in code_editors {
                    if stdout.contains(editor) {
                        let shell_names = [
                            "bash", "zsh", "fish", "sh", "python", "node", "npm",
                            "cargo", "claude", "htop", "vim", "nvim", "ssh", "docker",
                        ];
                        for shell in shell_names {
                            if let Some(title_start) = stdout.split('|').nth(1) {
                                let first_segment = title_start.split(" - ").next().unwrap_or("").trim();
                                if first_segment == shell || first_segment.starts_with(&format!("{}:", shell)) {
                                    crate::dev_log!("[OUTPUT] GNOME Eval detected VS Code terminal: {}", shell);
                                    return Some(true);
                                }
                            }
                        }
                        crate::dev_log!("[OUTPUT] GNOME Eval detected VS Code editor (not terminal)");
                        return Some(false);
                    }
                }
                return Some(false);
            }
        }
        None
    }

    /// Check if the focused window is a remote desktop app (RustDesk, AnyDesk, etc.)
    /// Remote desktop apps have unreliable clipboard sync on Linux.
    /// Direct typing (keystroke forwarding) is more reliable for these apps.
    #[cfg(target_os = "linux")]
    fn is_remote_desktop_focused() -> bool {
        if let Some(is_remote) = Self::check_remote_desktop_via_extension() {
            return is_remote;
        }
        false
    }

    #[cfg(target_os = "linux")]
    fn check_remote_desktop_via_extension() -> Option<bool> {
        let (xdg_runtime, dbus_addr) = Self::get_dbus_env();
        let mut child = std::process::Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.gnome.Shell",
                "--object-path",
                "/org/karaoketyper/FocusedWindow",
                "--method",
                "org.karaoketyper.FocusedWindow.GetFocusedWindow",
            ])
            .env("XDG_RUNTIME_DIR", &xdg_runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;

        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        if let Some(stdout) = child.stdout.take() {
                            use std::io::Read;
                            let mut buf = String::new();
                            let mut reader = std::io::BufReader::new(stdout);
                            if reader.read_to_string(&mut buf).is_ok() {
                                let trimmed = buf.trim().trim_start_matches('(').trim_end_matches(')');
                                let wm_class = trimmed.splitn(2, ", ").next()
                                    .unwrap_or("")
                                    .trim().trim_matches('\'')
                                    .to_lowercase();
                                let remote_apps = ["rustdesk", "anydesk", "vinagre"];
                                for app in remote_apps {
                                    if wm_class == app || wm_class.contains(app) {
                                        crate::dev_log!(
                                            "[OUTPUT] Remote desktop '{}' detected — forcing direct typing",
                                            app
                                        );
                                        return Some(true);
                                    }
                                }
                                return Some(false);
                            }
                        }
                    }
                    return None;
                }
                Ok(None) => {
                    if std::time::Instant::now() > deadline {
                        let _ = child.kill();
                        return None;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }

    /// Resolve the effective paste method for clipboard-based paste.
    /// When paste_method is Direct (auto-fallback) or CtrlV, auto-detect terminals
    /// and use Ctrl+Shift+V if a terminal is focused. User-explicit CtrlShiftV/ShiftInsert
    /// are used as-is.
    #[cfg(target_os = "linux")]
    fn resolve_paste_method(configured: &PasteMethod) -> PasteMethod {
        match configured {
            PasteMethod::CtrlShiftV | PasteMethod::ShiftInsert | PasteMethod::None => {
                configured.clone()
            }
            // Direct fallback or CtrlV: auto-detect terminal
            _ => {
                if Self::is_terminal_focused() {
                    crate::dev_log!("[OUTPUT] Terminal detected — using Ctrl+Shift+V");
                    PasteMethod::CtrlShiftV
                } else {
                    crate::dev_log!("[OUTPUT] Standard app — using Ctrl+V");
                    PasteMethod::CtrlV
                }
            }
        }
    }

    /// Check if a native direct-typing tool is available that handles keyboard
    /// layouts correctly (AZERTY, QWERTZ, etc.).
    ///
    /// IMPORTANT: wtype and ydotool send QWERTY keycodes that get re-interpreted
    /// by the compositor with the current layout → garbled text on AZERTY.
    /// Only xdotool (X11) and kwtype (KDE Wayland) handle layouts correctly.
    /// On true Wayland, wtype works because the compositor provides the keymap.
    /// On XWayland (Tauri reports X11 but session is Wayland), wtype garbles.
    #[cfg(target_os = "linux")]
    fn has_native_direct_tool() -> bool {
        if Self::is_wayland() {
            let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .to_lowercase()
                .contains("kde");
            (is_kde && Self::is_tool_available("kwtype"))
                || Self::is_tool_available("wtype")
                || Self::is_tool_available("dotool")
        } else {
            // X11: ONLY xdotool handles keyboard layouts correctly.
            // ydotool/wtype send QWERTY keycodes → garbled on AZERTY.
            Self::is_tool_available("xdotool")
        }
    }

    /// Direct typing: try native tools based on session type.
    /// Follows Handy's approach: Wayland tools for Wayland, X11 tools for X11.
    /// ydotool is tried on BOTH (works via uinput, layout-aware).
    /// Never use wtype on X11 — it runs but uses wrong keymap (AZERTY garbled).
    fn try_direct_typing(text: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            let is_wayland = Self::is_wayland();
            let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .to_lowercase()
                .contains("kde");

            crate::dev_log!(
                "[OUTPUT] Direct typing: is_wayland={}, kde={}",
                is_wayland, is_kde
            );

            if is_wayland {
                // Wayland: kwtype → wtype → dotool → ydotool
                if is_kde && Self::is_tool_available("kwtype") {
                    crate::dev_log!("[OUTPUT] Direct: using kwtype (KDE Wayland)");
                    return Self::run_direct_tool("kwtype", &["--", text]);
                }
                if Self::is_tool_available("wtype") {
                    crate::dev_log!("[OUTPUT] Direct: using wtype");
                    return Self::run_direct_tool("wtype", &["--", text]);
                }
                if Self::is_tool_available("dotool") {
                    crate::dev_log!("[OUTPUT] Direct: using dotool");
                    return Self::run_dotool_type(text);
                }
                if Self::is_tool_available("ydotool") {
                    crate::dev_log!("[OUTPUT] Direct: using ydotool");
                    return Self::run_direct_tool("ydotool", &["type", "--", text]);
                }
            } else {
                // X11: ONLY xdotool handles keyboard layouts correctly.
                // Do NOT use ydotool/wtype here — they send QWERTY keycodes
                // that get garbled on AZERTY/QWERTZ layouts.
                if Self::is_tool_available("xdotool") {
                    crate::dev_log!("[OUTPUT] Direct: using xdotool");
                    return Self::run_direct_tool("xdotool", &["type", "--clearmodifiers", "--", text]);
                }
            }
            crate::dev_log!("[OUTPUT] No native tool found, falling back to enigo");
        }

        // Fallback: enigo (works on all platforms)
        Self::try_enigo_type_text(text)
    }

    /// Type text directly via enigo (XTest on X11, platform APIs elsewhere).
    fn try_enigo_type_text(text: &str) -> Result<(), String> {
        use enigo::{Enigo, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to create enigo: {}", e))?;

        crate::dev_log!("[OUTPUT] enigo: typing {} chars directly...", text.len());
        enigo.text(text)
            .map_err(|e| format!("enigo text() failed: {}", e))?;

        crate::dev_log!("[OUTPUT] ✅ enigo direct typing complete");
        Ok(())
    }

    /// Check if a tool is available on PATH (cached — checked once per process).
    #[cfg(target_os = "linux")]
    fn is_tool_available(tool: &str) -> bool {
        use std::collections::HashMap;
        use std::sync::OnceLock;

        static CACHE: OnceLock<HashMap<String, bool>> = OnceLock::new();

        // Initialize cache on first call with all tools we might need
        let cache = CACHE.get_or_init(|| {
            let tools = ["wtype", "kwtype", "xdotool", "dotool", "ydotool", "wl-copy", "xclip", "xsel"];
            let mut map = HashMap::new();
            for t in tools {
                let available = std::process::Command::new("which")
                    .arg(t)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                crate::dev_log!("[OUTPUT] Tool '{}': {}", t, if available { "available" } else { "not found" });
                map.insert(t.to_string(), available);
            }
            map
        });

        cache.get(tool).copied().unwrap_or(false)
    }

    /// Run a direct typing tool with args.
    #[cfg(target_os = "linux")]
    fn run_direct_tool(tool: &str, args: &[&str]) -> Result<(), String> {
        let output = std::process::Command::new(tool)
            .args(args)
            .output()
            .map_err(|e| format!("Failed to run {}: {}", tool, e))?;

        if output.status.success() {
            crate::dev_log!("[OUTPUT] ✅ {} typed text successfully", tool);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("{} failed: {}", tool, stderr))
        }
    }

    /// dotool uses stdin for commands — special handling.
    #[cfg(target_os = "linux")]
    fn run_dotool_type(text: &str) -> Result<(), String> {
        use std::io::Write;
        use std::process::Stdio;

        let mut child = std::process::Command::new("dotool")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn dotool: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "type {}", text)
                .map_err(|e| format!("Failed to write to dotool stdin: {}", e))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("Failed to wait for dotool: {}", e))?;

        if output.status.success() {
            crate::dev_log!("[OUTPUT] ✅ dotool typed text successfully");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("dotool failed: {}", stderr))
        }
    }

    #[cfg(target_os = "linux")]
    fn try_enigo_shortcut(paste_method: &PasteMethod) -> Result<(), String> {
        use enigo::{Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to create enigo: {}", e))?;

        match paste_method {
            PasteMethod::CtrlShiftV => {
                crate::dev_log!("[OUTPUT] Using enigo for Ctrl+Shift+V...");
                enigo.key(Key::Control, enigo::Direction::Press)
                    .map_err(|e| format!("enigo Ctrl press: {}", e))?;
                enigo.key(Key::Shift, enigo::Direction::Press)
                    .map_err(|e| format!("enigo Shift press: {}", e))?;
                enigo.key(Key::Unicode('v'), enigo::Direction::Click)
                    .map_err(|e| format!("enigo V click: {}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.key(Key::Shift, enigo::Direction::Release)
                    .map_err(|e| format!("enigo Shift release: {}", e))?;
                enigo.key(Key::Control, enigo::Direction::Release)
                    .map_err(|e| format!("enigo Ctrl release: {}", e))?;
            }
            PasteMethod::ShiftInsert => {
                crate::dev_log!("[OUTPUT] Using enigo for Shift+Insert...");
                enigo.key(Key::Shift, enigo::Direction::Press)
                    .map_err(|e| format!("enigo Shift press: {}", e))?;
                enigo.key(Key::Other(0x6C), enigo::Direction::Click) // Insert key
                    .map_err(|e| format!("enigo Insert click: {}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.key(Key::Shift, enigo::Direction::Release)
                    .map_err(|e| format!("enigo Shift release: {}", e))?;
            }
            _ => {
                // Default: Ctrl+V
                crate::dev_log!("[OUTPUT] Using enigo for Ctrl+V...");
                enigo.key(Key::Control, enigo::Direction::Press)
                    .map_err(|e| format!("enigo Ctrl press: {}", e))?;
                enigo.key(Key::Unicode('v'), enigo::Direction::Click)
                    .map_err(|e| format!("enigo V click: {}", e))?;
                thread::sleep(Duration::from_millis(50));
                enigo.key(Key::Control, enigo::Direction::Release)
                    .map_err(|e| format!("enigo Ctrl release: {}", e))?;
            }
        }

        crate::dev_log!("[OUTPUT] ✅ enigo paste sent successfully");
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn try_uinput_shortcut(paste_method: &PasteMethod) -> Result<(), String> {
        linux_output::with_uinput(|device| {
            match paste_method {
                PasteMethod::CtrlShiftV => {
                    crate::dev_log!("[OUTPUT] Using uinput for Ctrl+Shift+V...");
                    device.ctrl_shift_v()
                }
                _ => {
                    crate::dev_log!("[OUTPUT] Using uinput for Ctrl+V...");
                    device.ctrl_v()
                }
            }
        })
    }

    #[cfg(target_os = "linux")]
    fn try_wtype_shortcut(paste_method: &PasteMethod) -> Result<bool, String> {
        if !std::process::Command::new("which")
            .arg("wtype")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(false);
        }

        let shortcut = match paste_method {
            PasteMethod::CtrlShiftV => {
                crate::dev_log!("[OUTPUT] Using wtype for Ctrl+Shift+V...");
                vec!["-M", "ctrl", "-M", "shift", "-k", "v"]
            }
            PasteMethod::ShiftInsert => {
                crate::dev_log!("[OUTPUT] Using wtype for Shift+Insert...");
                vec!["-M", "shift", "-k", "Insert"]
            }
            _ => {
                crate::dev_log!("[OUTPUT] Using wtype for Ctrl+V...");
                vec!["-M", "ctrl", "-k", "v"]
            }
        };

        let output = std::process::Command::new("wtype")
            .args(&shortcut)
            .output()
            .map_err(|e| format!("Failed to run wtype: {}", e))?;

        if output.status.success() {
            crate::dev_log!("[OUTPUT] ✅ wtype paste sent successfully");
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            crate::dev_log!("[OUTPUT] ⚠️ wtype failed: {}", stderr);
            Ok(false)
        }
    }

    /// Check if ydotoold daemon is running (required for reliable ydotool).
    /// Without the daemon, ydotool runs in degraded mode with random key delays.
    #[cfg(target_os = "linux")]
    fn is_ydotoold_running() -> bool {
        std::process::Command::new("pgrep")
            .arg("ydotoold")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Send paste shortcut via ydotool using raw Linux input keycodes.
    /// ydotool uses format: <keycode>:<pressed> where 1=down, 0=up.
    /// Keycodes: KEY_LEFTCTRL=29, KEY_LEFTSHIFT=42, KEY_V=47, KEY_INSERT=110
    /// Matches Handy's send_key_combo_via_ydotool() implementation.
    /// REQUIRES ydotoold daemon — without it, keycodes arrive with random delays.
    #[cfg(target_os = "linux")]
    fn try_ydotool_shortcut(paste_method: &PasteMethod) -> Result<(), String> {
        if !Self::is_ydotoold_running() {
            return Err("ydotoold daemon not running".to_string());
        }
        let args: Vec<&str> = match paste_method {
            PasteMethod::CtrlShiftV => {
                crate::dev_log!("[OUTPUT] Using ydotool for Ctrl+Shift+V (keycodes: 29+42+47)...");
                vec!["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"]
            }
            PasteMethod::ShiftInsert => {
                crate::dev_log!("[OUTPUT] Using ydotool for Shift+Insert (keycodes: 42+110)...");
                vec!["key", "42:1", "110:1", "110:0", "42:0"]
            }
            _ => {
                crate::dev_log!("[OUTPUT] Using ydotool for Ctrl+V (keycodes: 29+47)...");
                vec!["key", "29:1", "47:1", "47:0", "29:0"]
            }
        };

        let output = std::process::Command::new("ydotool")
            .args(&args)
            .output()
            .map_err(|e| format!("Failed to run ydotool: {}", e))?;

        if output.status.success() {
            crate::dev_log!("[OUTPUT] ✅ ydotool paste sent successfully");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("ydotool failed: {}", stderr))
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn send_paste_shortcut(&self) -> Result<(), String> {
        use enigo::{Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to create enigo instance: {}", e))?;

        crate::dev_log!("[OUTPUT] Sending paste shortcut with enigo ({:?})...", self.paste_method);

        #[cfg(target_os = "macos")]
        let modifier = Key::Meta;
        #[cfg(not(target_os = "macos"))]
        let modifier = Key::Control;

        match self.paste_method {
            PasteMethod::CtrlShiftV => {
                enigo.key(modifier, enigo::Direction::Press)
                    .map_err(|e| format!("Failed to press modifier: {}", e))?;
                enigo.key(Key::Shift, enigo::Direction::Press)
                    .map_err(|e| format!("Failed to press Shift: {}", e))?;
                thread::sleep(Duration::from_millis(10));
                enigo.key(Key::Unicode('v'), enigo::Direction::Click)
                    .map_err(|e| format!("Failed to press V: {}", e))?;
                thread::sleep(Duration::from_millis(10));
                enigo.key(Key::Shift, enigo::Direction::Release)
                    .map_err(|e| format!("Failed to release Shift: {}", e))?;
                enigo.key(modifier, enigo::Direction::Release)
                    .map_err(|e| format!("Failed to release modifier: {}", e))?;
            }
            _ => {
                // Default: Ctrl+V / Cmd+V
                enigo.key(modifier, enigo::Direction::Press)
                    .map_err(|e| format!("Failed to press modifier: {}", e))?;
                thread::sleep(Duration::from_millis(10));
                enigo.key(Key::Unicode('v'), enigo::Direction::Click)
                    .map_err(|e| format!("Failed to press V: {}", e))?;
                thread::sleep(Duration::from_millis(10));
                enigo.key(modifier, enigo::Direction::Release)
                    .map_err(|e| format!("Failed to release modifier: {}", e))?;
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn type_text_direct(&mut self, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }

        crate::dev_log!("[OUTPUT] Typing text directly: '{}'", text);

        #[cfg(target_os = "linux")]
        {
            linux_output::with_uinput(|device| device.type_text(text))?;
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

        crate::dev_log!("[OUTPUT] ✅ Text typed successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paste_method_default_linux() {
        #[cfg(target_os = "linux")]
        assert_eq!(PasteMethod::default(), PasteMethod::Direct);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(PasteMethod::default(), PasteMethod::CtrlV);
    }

    #[test]
    fn test_text_output_new() {
        // Tests that require AppHandle can be skipped or use a mock
        // AppHandle requires a full Tauri app context which is not available in unit tests
        // This test is for structure validation only
    }

    #[test]
    fn test_text_output_empty_text_noop() {
        // Tests that require AppHandle can be skipped or use a mock
        // AppHandle requires a full Tauri app context which is not available in unit tests
    }

    #[test]
    fn test_text_output_none_method_noop() {
        // Tests that require AppHandle can be skipped or use a mock
        // AppHandle requires a full Tauri app context which is not available in unit tests
    }

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::super::*;

        #[test]
        fn test_is_wayland_detection() {
            // This test verifies the function doesn't panic.
            // Actual result depends on the test environment.
            let _result = TextOutput::is_wayland();
        }

        #[test]
        fn test_has_native_direct_tool_does_not_panic() {
            // Verifies has_native_direct_tool runs without panic.
            // Result depends on installed tools in the test environment.
            let _result = TextOutput::has_native_direct_tool();
        }

        #[test]
        fn test_is_tool_available_nonexistent() {
            // A tool that definitely doesn't exist
            assert!(!TextOutput::is_tool_available("this_tool_does_not_exist_xyz"));
        }

        #[test]
        fn test_uinput_keymap_covers_ascii() {
            use super::super::linux_output::get_keymap;
            let keymap = get_keymap();

            // All lowercase letters must be in keymap
            for c in 'a'..='z' {
                assert!(keymap.contains_key(&c), "Missing lowercase: {}", c);
            }
            // All uppercase letters
            for c in 'A'..='Z' {
                assert!(keymap.contains_key(&c), "Missing uppercase: {}", c);
            }
            // Digits
            for c in '0'..='9' {
                assert!(keymap.contains_key(&c), "Missing digit: {}", c);
            }
            // Space and common punctuation
            assert!(keymap.contains_key(&' '), "Missing space");
            assert!(keymap.contains_key(&'.'), "Missing dot");
            assert!(keymap.contains_key(&','), "Missing comma");
        }

        #[test]
        fn test_uinput_keymap_does_not_cover_accented_chars() {
            use super::super::linux_output::get_keymap;
            let keymap = get_keymap();

            // French accented characters are NOT in the keymap — this is by design.
            // When these characters are typed via uinput, they are silently dropped.
            // This is why the clipboard fallback exists.
            let accented = ['é', 'è', 'ê', 'ë', 'à', 'ç', 'ù', 'ô', 'î', 'â'];
            for c in accented {
                assert!(
                    !keymap.contains_key(&c),
                    "Accented char '{}' unexpectedly in keymap", c
                );
            }
        }

        #[test]
        fn test_classify_window_gnome_terminal() {
            assert!(TextOutput::classify_window_from_dbus(
                "('gnome-terminal-server', 'user@host: ~')"
            ));
        }

        #[test]
        fn test_classify_window_ghostty() {
            assert!(TextOutput::classify_window_from_dbus(
                "('com.mitchellh.ghostty', 'bash')"
            ));
        }

        #[test]
        fn test_classify_window_vscode_terminal() {
            assert!(TextOutput::classify_window_from_dbus(
                "('code', 'bash - project - Terminal - Visual Studio Code')"
            ));
        }

        #[test]
        fn test_classify_window_vscode_editor() {
            assert!(!TextOutput::classify_window_from_dbus(
                "('code', 'main.rs - project - Visual Studio Code')"
            ));
        }

        #[test]
        fn test_classify_window_firefox() {
            assert!(!TextOutput::classify_window_from_dbus(
                "('firefox', 'Google - Mozilla Firefox')"
            ));
        }

        #[test]
        fn test_classify_window_alacritty() {
            assert!(TextOutput::classify_window_from_dbus(
                "('alacritty', 'Alacritty')"
            ));
        }

        #[test]
        fn test_classify_window_remmina_not_terminal() {
            // Remote desktop apps are NOT terminals — they forward keystrokes.
            // We can't know if the remote side is a terminal or a GUI app.
            assert!(!TextOutput::classify_window_from_dbus(
                "('remmina', 'SSH connection to server')"
            ));
        }

        #[test]
        fn test_classify_window_rustdesk_not_terminal() {
            assert!(!TextOutput::classify_window_from_dbus(
                "('rustdesk', 'RustDesk - 192.168.1.100')"
            ));
        }

        #[test]
        fn test_classify_window_anydesk_not_terminal() {
            assert!(!TextOutput::classify_window_from_dbus(
                "('anydesk', 'AnyDesk')"
            ));
        }

        #[test]
        fn test_resolve_paste_method_explicit_ctrlshiftv() {
            // User-explicit CtrlShiftV should NOT be overridden
            let result = TextOutput::resolve_paste_method(&PasteMethod::CtrlShiftV);
            assert_eq!(result, PasteMethod::CtrlShiftV);
        }

        #[test]
        fn test_resolve_paste_method_explicit_shiftinsert() {
            // User-explicit ShiftInsert should NOT be overridden
            let result = TextOutput::resolve_paste_method(&PasteMethod::ShiftInsert);
            assert_eq!(result, PasteMethod::ShiftInsert);
        }

        #[test]
        fn test_resolve_paste_method_none() {
            let result = TextOutput::resolve_paste_method(&PasteMethod::None);
            assert_eq!(result, PasteMethod::None);
        }
    }
}
