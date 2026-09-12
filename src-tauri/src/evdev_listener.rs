use evdev::{Device, EventType, InputEventKind, Key};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

use crate::config::AppConfig;
use crate::shortcut_activation::{KeyEvent, SharedMachine};
use crate::shortcut_session::{feed_key, spawn_effect_executor, EffectQueue};

/// Shared listener state so `reconfigure()` can swap the hotkey AND spawn
/// additional device monitors when the new shortcut targets a different
/// device class (e.g. keyboard-only → mouse-only). Populated once by
/// `start_evdev_listener`.
struct ListenerHandle {
    hotkey: std::sync::RwLock<HotkeyConfig>,
    state: HotkeyState,
    app_handle: AppHandle,
    device_paths: std::sync::Mutex<HashSet<PathBuf>>,
}

static ACTIVE_LISTENER: std::sync::OnceLock<Arc<ListenerHandle>> = std::sync::OnceLock::new();

/// Replace the live hotkey configuration with the one parsed from `shortcut`
/// and spawn monitors for any `/dev/input/event*` device that matches the new
/// shortcut but isn't already being watched. No-op if the evdev listener has
/// not been started yet.
pub(crate) fn reconfigure(shortcut: &str) {
    let Some(handle) = ACTIVE_LISTENER.get() else {
        crate::dev_log!("[EVDEV] ⚠️ reconfigure called before listener start — ignored");
        return;
    };

    let new_cfg = HotkeyConfig::parse(shortcut);

    match handle.hotkey.write() {
        Ok(mut guard) => *guard = new_cfg.clone(),
        Err(poisoned) => *poisoned.into_inner() = new_cfg.clone(),
    }
    crate::dev_log!("[EVDEV] 🔁 Hotkey reconfigured live: {}", shortcut);

    // Cover device-class transitions (keyboard-only ↔ mouse-only): spawn
    // monitors for newly-matching devices that aren't watched yet. Existing
    // threads keep running and pick up the new hotkey via the RwLock snapshot.
    match find_input_devices(&new_cfg) {
        Ok(paths) => {
            let mut watched = match handle.device_paths.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            for path in paths {
                if watched.insert(path.clone()) {
                    crate::dev_log!(
                        "[EVDEV] ➕ Watching new device for reconfigured hotkey: {}",
                        path.display()
                    );
                    let device_state = Arc::new(DeviceState {
                        _path: path.clone(),
                        reconnect_attempts: AtomicU32::new(0),
                        is_active: AtomicBool::new(true),
                    });
                    spawn_device_monitor(DeviceMonitorConfig {
                        device_path: path,
                        app_handle: handle.app_handle.clone(),
                        state: handle.state.clone(),
                        hotkey_config: Arc::clone(handle),
                        device_state,
                    });
                }
            }
        }
        Err(e) => crate::dev_log!("[EVDEV] ⚠️ reconfigure re-enumerate failed: {}", e),
    }
}

/// Parsed hotkey configuration
#[derive(Debug, Clone)]
struct HotkeyConfig {
    require_ctrl: bool,
    require_alt: bool,
    require_shift: bool,
    trigger_key: Option<Key>,       // For keyboard keys
    trigger_mouse_btn: Option<u16>, // For mouse buttons (BTN_SIDE=0x114, BTN_EXTRA=0x115, etc.)
}

impl HotkeyConfig {
    fn parse(shortcut: &str) -> Self {
        let parts: Vec<&str> = shortcut.split('+').collect();
        let mut config = HotkeyConfig {
            require_ctrl: false,
            require_alt: false,
            require_shift: false,
            trigger_key: None,
            trigger_mouse_btn: None,
        };

        for part in parts {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" | "commandorcontrol" => config.require_ctrl = true,
                "alt" => config.require_alt = true,
                "shift" => config.require_shift = true,
                "super" | "meta" => {} // Ignore Super/Meta for now
                "space" => config.trigger_key = Some(Key::KEY_SPACE),
                "mouse4" => config.trigger_mouse_btn = Some(0x115), // BTN_EXTRA
                "mouse5" => config.trigger_mouse_btn = Some(0x114), // BTN_SIDE
                "mouseright" => config.trigger_mouse_btn = Some(0x111), // BTN_RIGHT
                "mousemiddle" => config.trigger_mouse_btn = Some(0x112), // BTN_MIDDLE
                "mouseleft" => config.trigger_mouse_btn = Some(0x110), // BTN_LEFT
                key => {
                    // Try to map single character keys
                    if key.len() == 1 {
                        if let Some(c) = key.chars().next() {
                            config.trigger_key = char_to_key(c);
                        }
                    }
                }
            }
        }

        // Default to Ctrl+Space if nothing parsed
        if config.trigger_key.is_none() && config.trigger_mouse_btn.is_none() {
            config.require_ctrl = true;
            config.trigger_key = Some(Key::KEY_SPACE);
        }

        crate::dev_log!("[EVDEV] Parsed hotkey config: {:?}", config);
        config
    }
}

/// Convert a character to evdev Key
fn char_to_key(c: char) -> Option<Key> {
    match c.to_ascii_uppercase() {
        'A' => Some(Key::KEY_A),
        'B' => Some(Key::KEY_B),
        'C' => Some(Key::KEY_C),
        'D' => Some(Key::KEY_D),
        'E' => Some(Key::KEY_E),
        'F' => Some(Key::KEY_F),
        'G' => Some(Key::KEY_G),
        'H' => Some(Key::KEY_H),
        'I' => Some(Key::KEY_I),
        'J' => Some(Key::KEY_J),
        'K' => Some(Key::KEY_K),
        'L' => Some(Key::KEY_L),
        'M' => Some(Key::KEY_M),
        'N' => Some(Key::KEY_N),
        'O' => Some(Key::KEY_O),
        'P' => Some(Key::KEY_P),
        'Q' => Some(Key::KEY_Q),
        'R' => Some(Key::KEY_R),
        'S' => Some(Key::KEY_S),
        'T' => Some(Key::KEY_T),
        'U' => Some(Key::KEY_U),
        'V' => Some(Key::KEY_V),
        'W' => Some(Key::KEY_W),
        'X' => Some(Key::KEY_X),
        'Y' => Some(Key::KEY_Y),
        'Z' => Some(Key::KEY_Z),
        '0' => Some(Key::KEY_0),
        '1' => Some(Key::KEY_1),
        '2' => Some(Key::KEY_2),
        '3' => Some(Key::KEY_3),
        '4' => Some(Key::KEY_4),
        '5' => Some(Key::KEY_5),
        '6' => Some(Key::KEY_6),
        '7' => Some(Key::KEY_7),
        '8' => Some(Key::KEY_8),
        '9' => Some(Key::KEY_9),
        _ => None,
    }
}

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY_MS: u64 = 2000;

struct DeviceState {
    _path: PathBuf,
    reconnect_attempts: AtomicU32,
    is_active: AtomicBool,
}

#[derive(Clone)]
struct HotkeyState {
    ctrl_pressed: Arc<AtomicBool>,
    alt_pressed: Arc<AtomicBool>,
    shift_pressed: Arc<AtomicBool>,
    trigger_pressed: Arc<AtomicBool>,
    /// Decides what a press or release means under the configured activation mode.
    machine: SharedMachine,
    /// Queue of the sequential effect executor (see `shortcut_session`).
    effects: EffectQueue,
}

struct DeviceMonitorConfig {
    device_path: PathBuf,
    app_handle: AppHandle,
    state: HotkeyState,
    hotkey_config: Arc<ListenerHandle>,
    device_state: Arc<DeviceState>,
}

/// Find input devices that can handle our hotkey
fn find_input_devices(hotkey: &HotkeyConfig) -> Result<Vec<PathBuf>, String> {
    let mut devices = Vec::new();

    for entry in
        std::fs::read_dir("/dev/input").map_err(|e| format!("Failed to read /dev/input: {}", e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();

        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("event"))
            .unwrap_or(false)
        {
            continue;
        }

        if let Ok(device) = Device::open(&path) {
            let device_name = device.name().unwrap_or("unknown").to_string();
            let supports_keys = device.supported_events().contains(EventType::KEY);

            if !supports_keys {
                continue;
            }

            // Check if device supports the trigger key/button
            let has_trigger = if let Some(trigger_key) = hotkey.trigger_key {
                device
                    .supported_keys()
                    .map(|keys| keys.contains(trigger_key))
                    .unwrap_or(false)
            } else if let Some(btn_code) = hotkey.trigger_mouse_btn {
                // For mouse buttons, check if it's a mouse device
                device
                    .supported_keys()
                    .map(|keys| {
                        // Check for mouse buttons (BTN_LEFT through BTN_TASK range: 0x110-0x11f)
                        keys.contains(Key::BTN_LEFT)
                            || keys.contains(Key::BTN_RIGHT)
                            || keys.contains(Key::BTN_MIDDLE)
                            || keys.contains(Key::BTN_SIDE)
                            || keys.contains(Key::BTN_EXTRA)
                            || (0x110..=0x11f).contains(&btn_code)
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            // Check if device supports required modifiers
            let has_modifiers = if hotkey.require_ctrl || hotkey.require_alt || hotkey.require_shift
            {
                device
                    .supported_keys()
                    .map(|keys| {
                        let has_ctrl = !hotkey.require_ctrl
                            || keys.contains(Key::KEY_LEFTCTRL)
                            || keys.contains(Key::KEY_RIGHTCTRL);
                        let has_alt = !hotkey.require_alt
                            || keys.contains(Key::KEY_LEFTALT)
                            || keys.contains(Key::KEY_RIGHTALT);
                        let has_shift = !hotkey.require_shift
                            || keys.contains(Key::KEY_LEFTSHIFT)
                            || keys.contains(Key::KEY_RIGHTSHIFT);
                        has_ctrl && has_alt && has_shift
                    })
                    .unwrap_or(false)
            } else {
                true // No modifiers required
            };

            // Include device if:
            // 1. It has the trigger key/button, OR
            // 2. It has modifiers (for keyboard+mouse combos, we need both devices)
            if has_trigger || has_modifiers {
                crate::dev_log!(
                    "[EVDEV] Found input device: {} ({}) - trigger: {}, modifiers: {}",
                    path.display(),
                    device_name,
                    has_trigger,
                    has_modifiers
                );
                devices.push(path);
            }
        }
    }

    if devices.is_empty() {
        return Err("No input devices found for configured hotkey".to_string());
    }

    Ok(devices)
}

pub async fn start_evdev_listener(app_handle: AppHandle) -> Result<(), String> {
    crate::dev_log!("[EVDEV] Starting input listener for push-to-talk...");

    // Load hotkey configuration
    let config = AppConfig::load().unwrap_or_default();
    let hotkey = HotkeyConfig::parse(&config.shortcut);
    crate::dev_log!("[EVDEV] Configured hotkey: {}", config.shortcut);

    let input_devices = find_input_devices(&hotkey)?;
    crate::dev_log!("[EVDEV] Monitoring {} input device(s)", input_devices.len());

    let machine = SharedMachine::default();
    let hotkey_state = HotkeyState {
        ctrl_pressed: Arc::new(AtomicBool::new(false)),
        alt_pressed: Arc::new(AtomicBool::new(false)),
        shift_pressed: Arc::new(AtomicBool::new(false)),
        trigger_pressed: Arc::new(AtomicBool::new(false)),
        effects: spawn_effect_executor(app_handle.clone(), machine.clone()),
        machine,
    };

    let handle = ACTIVE_LISTENER.get_or_init(|| {
        Arc::new(ListenerHandle {
            hotkey: std::sync::RwLock::new(hotkey.clone()),
            state: hotkey_state.clone(),
            app_handle: app_handle.clone(),
            device_paths: std::sync::Mutex::new(HashSet::new()),
        })
    });
    // If re-entered (paranoia), overwrite the stored hotkey so the newer
    // startup config wins; existing monitors pick it up on next event.
    if let Ok(mut guard) = handle.hotkey.write() {
        *guard = hotkey;
    }

    for device_path in input_devices {
        // Record the path under the handle's lock so reconfigure() knows it's
        // already watched.
        let mut watched = match handle.device_paths.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !watched.insert(device_path.clone()) {
            continue;
        }
        drop(watched);

        let device_state = Arc::new(DeviceState {
            _path: device_path.clone(),
            reconnect_attempts: AtomicU32::new(0),
            is_active: AtomicBool::new(true),
        });

        spawn_device_monitor(DeviceMonitorConfig {
            device_path,
            app_handle: app_handle.clone(),
            state: handle.state.clone(),
            hotkey_config: Arc::clone(handle),
            device_state,
        });
    }

    crate::dev_log!("[EVDEV] ✅ Shortcut listener started!");
    crate::dev_log!("[EVDEV] Hotkey: {}", config.shortcut);

    Ok(())
}

fn spawn_device_monitor(config: DeviceMonitorConfig) {
    // Use std::thread::spawn instead of tokio::task::spawn_blocking.
    // These are infinite loops that never return — spawn_blocking is designed
    // for tasks that "eventually finish" and would exhaust tokio's blocking pool.
    std::thread::spawn(move || {
        monitor_device_with_reconnect(config);
    });
}

fn monitor_device_with_reconnect(config: DeviceMonitorConfig) {
    loop {
        if !config.device_state.is_active.load(Ordering::SeqCst) {
            crate::dev_log!(
                "[EVDEV] Device {} marked inactive, stopping monitor",
                config.device_path.display()
            );
            return;
        }

        match monitor_input_device(&config) {
            Ok(()) => {
                crate::dev_log!(
                    "[EVDEV] Device {} monitor exited cleanly",
                    config.device_path.display()
                );
                return;
            }
            Err(e) => {
                let attempts = config
                    .device_state
                    .reconnect_attempts
                    .fetch_add(1, Ordering::SeqCst)
                    + 1;

                if attempts > MAX_RECONNECT_ATTEMPTS {
                    crate::dev_log!(
                        "[EVDEV] Max reconnect attempts ({}) reached for {}, giving up",
                        MAX_RECONNECT_ATTEMPTS,
                        config.device_path.display()
                    );
                    config.device_state.is_active.store(false, Ordering::SeqCst);
                    return;
                }

                let delay = RECONNECT_DELAY_MS * 2u64.pow(attempts - 1);
                crate::dev_log!(
                    "[EVDEV] Device {} error: {}. Reconnecting in {}ms (attempt {}/{})",
                    config.device_path.display(),
                    e,
                    delay,
                    attempts,
                    MAX_RECONNECT_ATTEMPTS
                );

                std::thread::sleep(Duration::from_millis(delay));

                if !config.device_path.exists() {
                    crate::dev_log!(
                        "[EVDEV] Device {} no longer exists, waiting for reconnect...",
                        config.device_path.display()
                    );
                    continue;
                }

                crate::dev_log!(
                    "[EVDEV] Attempting to reconnect to {}...",
                    config.device_path.display()
                );
            }
        }
    }
}

fn monitor_input_device(config: &DeviceMonitorConfig) -> Result<(), String> {
    let mut device = Device::open(&config.device_path)
        .map_err(|e| format!("Failed to open {}: {}", config.device_path.display(), e))?;

    let device_name = device.name().unwrap_or("unknown").to_string();
    crate::dev_log!(
        "[EVDEV] Connected to: {} ({})",
        config.device_path.display(),
        device_name
    );

    loop {
        let events = device
            .fetch_events()
            .map_err(|e| format!("Device {} lost: {}", device_name, e))?;

        for event in events {
            if let InputEventKind::Key(key) = event.kind() {
                let pressed = event.value() == 1;
                let released = event.value() == 0;

                // Track modifier keys
                match key {
                    Key::KEY_LEFTCTRL | Key::KEY_RIGHTCTRL => {
                        if pressed {
                            config.state.ctrl_pressed.store(true, Ordering::SeqCst);
                        } else if released {
                            config.state.ctrl_pressed.store(false, Ordering::SeqCst);
                        }
                    }
                    Key::KEY_LEFTALT | Key::KEY_RIGHTALT => {
                        if pressed {
                            config.state.alt_pressed.store(true, Ordering::SeqCst);
                        } else if released {
                            config.state.alt_pressed.store(false, Ordering::SeqCst);
                        }
                    }
                    Key::KEY_LEFTSHIFT | Key::KEY_RIGHTSHIFT => {
                        if pressed {
                            config.state.shift_pressed.store(true, Ordering::SeqCst);
                        } else if released {
                            config.state.shift_pressed.store(false, Ordering::SeqCst);
                        }
                    }
                    _ => {}
                }

                // Snapshot the current hotkey config under a read lock so
                // `reconfigure()` can swap it between events without blocking
                // the hot path for the rest of the match.
                let hotkey_snapshot = match config.hotkey_config.hotkey.read() {
                    Ok(guard) => guard.clone(),
                    Err(poisoned) => poisoned.into_inner().clone(),
                };

                // Check if this is the trigger key/button
                let is_trigger = if let Some(trigger_key) = hotkey_snapshot.trigger_key {
                    key == trigger_key
                } else if let Some(btn_code) = hotkey_snapshot.trigger_mouse_btn {
                    // Check mouse buttons by their code
                    let key_code = key.code();
                    key_code == btn_code
                } else {
                    false
                };

                if is_trigger {
                    // Check if all required modifiers are pressed
                    let modifiers_ok = (!hotkey_snapshot.require_ctrl
                        || config.state.ctrl_pressed.load(Ordering::SeqCst))
                        && (!hotkey_snapshot.require_alt
                            || config.state.alt_pressed.load(Ordering::SeqCst))
                        && (!hotkey_snapshot.require_shift
                            || config.state.shift_pressed.load(Ordering::SeqCst));

                    if pressed && !config.state.trigger_pressed.load(Ordering::SeqCst) {
                        config.state.trigger_pressed.store(true, Ordering::SeqCst);
                        if modifiers_ok {
                            feed_key(
                                &config.app_handle,
                                &config.state.machine,
                                &config.state.effects,
                                KeyEvent::Pressed,
                            );
                        }
                    } else if released {
                        config.state.trigger_pressed.store(false, Ordering::SeqCst);
                        // Fed regardless of modifiers: Ctrl is often let go before Space.
                        feed_key(
                            &config.app_handle,
                            &config.state.machine,
                            &config.state.effects,
                            KeyEvent::Released,
                        );
                    }
                }
            }
        }
    }
}
