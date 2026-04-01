use evdev::{Device, EventType, InputEventKind, Key};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::{do_start_recording, do_stop_recording, do_type_text, AppState};

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

struct HotkeyState {
    ctrl_pressed: Arc<AtomicBool>,
    alt_pressed: Arc<AtomicBool>,
    shift_pressed: Arc<AtomicBool>,
    trigger_pressed: Arc<AtomicBool>,
    is_recording: Arc<AtomicBool>,
}

struct DeviceMonitorConfig {
    device_path: PathBuf,
    app_handle: AppHandle,
    state: HotkeyState,
    hotkey_config: Arc<HotkeyConfig>,
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

    let ctrl_pressed = Arc::new(AtomicBool::new(false));
    let alt_pressed = Arc::new(AtomicBool::new(false));
    let shift_pressed = Arc::new(AtomicBool::new(false));
    let trigger_pressed = Arc::new(AtomicBool::new(false));
    let is_recording = Arc::new(AtomicBool::new(false));
    let hotkey_config = Arc::new(hotkey);
    let device_states: Arc<Mutex<HashMap<PathBuf, Arc<DeviceState>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    for device_path in input_devices {
        let state = Arc::new(DeviceState {
            _path: device_path.clone(),
            reconnect_attempts: AtomicU32::new(0),
            is_active: AtomicBool::new(true),
        });

        device_states
            .lock()
            .await
            .insert(device_path.clone(), state.clone());

        let config = DeviceMonitorConfig {
            device_path: device_path.clone(),
            app_handle: app_handle.clone(),
            state: HotkeyState {
                ctrl_pressed: ctrl_pressed.clone(),
                alt_pressed: alt_pressed.clone(),
                shift_pressed: shift_pressed.clone(),
                trigger_pressed: trigger_pressed.clone(),
                is_recording: is_recording.clone(),
            },
            hotkey_config: hotkey_config.clone(),
            device_state: state,
        };

        spawn_device_monitor(config);
    }

    crate::dev_log!("[EVDEV] ✅ Push-to-talk listener started!");
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
                let attempts = config.device_state.reconnect_attempts.fetch_add(1, Ordering::SeqCst) + 1;

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

                // Check if this is the trigger key/button
                let is_trigger = if let Some(trigger_key) = config.hotkey_config.trigger_key {
                    key == trigger_key
                } else if let Some(btn_code) = config.hotkey_config.trigger_mouse_btn {
                    // Check mouse buttons by their code
                    let key_code = key.code();
                    key_code == btn_code
                } else {
                    false
                };

                if is_trigger {
                    // Check if all required modifiers are pressed
                    let modifiers_ok = (!config.hotkey_config.require_ctrl
                        || config.state.ctrl_pressed.load(Ordering::SeqCst))
                        && (!config.hotkey_config.require_alt || config.state.alt_pressed.load(Ordering::SeqCst))
                        && (!config.hotkey_config.require_shift || config.state.shift_pressed.load(Ordering::SeqCst));

                    if pressed && !config.state.trigger_pressed.load(Ordering::SeqCst) {
                        config.state.trigger_pressed.store(true, Ordering::SeqCst);

                        if modifiers_ok && !config.state.is_recording.load(Ordering::SeqCst) {
                            let t0 = std::time::Instant::now();
                            crate::dev_log!("[TIMING] ========== RECORDING SESSION START ==========");
                            crate::dev_log!("[TIMING] T+0ms: Hotkey PRESSED");
                            config.state.is_recording.store(true, Ordering::SeqCst);

                            // Store t0 in AppState
                            {
                                let state = config.app_handle.state::<AppState>();
                                *state.recording_start_time.lock().unwrap() = Some(t0);
                            }

                            // Show overlay centered on cursor's monitor
                            crate::show_overlay_with_state(&config.app_handle, "recording");
                            crate::dev_log!("[TIMING] T+{}ms: overlay shown", t0.elapsed().as_millis());

                            // Start recording
                            let app_clone = config.app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                crate::dev_log!(
                                    "[TIMING] T+{}ms: async spawn started",
                                    t0.elapsed().as_millis()
                                );
                                let state = app_clone.state::<AppState>();
                                match do_start_recording(app_clone.clone(), &state).await {
                                    Ok(_) => crate::dev_log!(
                                        "[TIMING] T+{}ms: do_start_recording() returned OK",
                                        t0.elapsed().as_millis()
                                    ),
                                    Err(e) => crate::dev_log!(
                                        "[TIMING] T+{}ms: do_start_recording() FAILED: {}",
                                        t0.elapsed().as_millis(),
                                        e
                                    ),
                                }
                            });
                        }
                    } else if released {
                        config.state.trigger_pressed.store(false, Ordering::SeqCst);

                        if config.state.is_recording.load(Ordering::SeqCst) {
                            let t0 = {
                                let state = config.app_handle.state::<AppState>();
                                let guard = state.recording_start_time.lock().unwrap();
                                guard.unwrap_or_else(std::time::Instant::now)
                            };
                            crate::dev_log!("[TIMING] T+{}ms: Hotkey RELEASED", t0.elapsed().as_millis());
                            config.state.is_recording.store(false, Ordering::SeqCst);

                            // Stop recording, transcribe, and type
                            let app_clone = config.app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = app_clone.state::<AppState>();
                                let t0 = state
                                    .recording_start_time
                                    .lock()
                                    .unwrap()
                                    .unwrap_or_else(std::time::Instant::now);

                                crate::dev_log!(
                                    "[TIMING] T+{}ms: calling do_stop_recording()",
                                    t0.elapsed().as_millis()
                                );

                                // Switch overlay to processing state
                                crate::show_overlay_with_state(&app_clone, "processing");

                                match do_stop_recording(app_clone.clone(), &state).await {
                                    Ok(transcription) => {
                                        crate::dev_log!(
                                            "[TIMING] T+{}ms: transcription complete: '{}'",
                                            t0.elapsed().as_millis(),
                                            transcription
                                        );
                                        if !transcription.is_empty() {
                                            // Hide overlay IMMEDIATELY (not animated) so it
                                            // releases focus before we paste. The animated
                                            // hide takes 300ms but paste happens at 150ms,
                                            // causing Ctrl+V to go to the overlay instead
                                            // of the target app.
                                            if let Some(window) = app_clone.get_webview_window("overlay") {
                                                let _ = window.hide();
                                            }
                                            crate::dev_log!(
                                                "[TIMING] T+{}ms: overlay hidden, waiting 150ms for focus",
                                                t0.elapsed().as_millis()
                                            );
                                            tokio::time::sleep(tokio::time::Duration::from_millis(
                                                150,
                                            ))
                                            .await;

                                            crate::dev_log!(
                                                "[TIMING] T+{}ms: pasting text",
                                                t0.elapsed().as_millis()
                                            );
                                            let config = AppConfig::load().unwrap_or_default();
                                            if let Err(e) = do_type_text(transcription, &app_clone, config.paste_method).await {
                                                crate::dev_log!(
                                                    "[TIMING] T+{}ms: paste FAILED: {}",
                                                    t0.elapsed().as_millis(),
                                                    e
                                                );
                                            } else {
                                                crate::dev_log!(
                                                    "[TIMING] T+{}ms: ========== SESSION COMPLETE ==========",
                                                    t0.elapsed().as_millis()
                                                );
                                            }
                                        } else {
                                            crate::dev_log!(
                                                "[TIMING] T+{}ms: no speech detected",
                                                t0.elapsed().as_millis()
                                            );
                                            crate::hide_overlay_animated(&app_clone);
                                        }
                                    }
                                    Err(e) => {
                                        crate::dev_log!(
                                            "[TIMING] T+{}ms: stop_recording FAILED: {}",
                                            t0.elapsed().as_millis(),
                                            e
                                        );
                                        crate::hide_overlay_animated(&app_clone);
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }
    }
}
