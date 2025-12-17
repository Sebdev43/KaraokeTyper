use evdev::{Device, EventType, InputEventKind, Key};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::{AppState, do_start_recording, do_stop_recording, do_type_text};

const KEY_SPACE: Key = Key::KEY_SPACE;
const KEY_LEFTCTRL: Key = Key::KEY_LEFTCTRL;
const KEY_RIGHTCTRL: Key = Key::KEY_RIGHTCTRL;

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY_MS: u64 = 2000;

struct DeviceState {
    path: PathBuf,
    reconnect_attempts: AtomicU32,
    is_active: AtomicBool,
}

fn find_keyboard_devices() -> Result<Vec<PathBuf>, String> {
    let mut keyboards = Vec::new();

    for entry in std::fs::read_dir("/dev/input")
        .map_err(|e| format!("Failed to read /dev/input: {}", e))?
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
            if device.supported_events().contains(EventType::KEY) {
                if let Some(keys) = device.supported_keys() {
                    if keys.contains(KEY_SPACE)
                        && (keys.contains(KEY_LEFTCTRL) || keys.contains(KEY_RIGHTCTRL))
                    {
                        println!(
                            "[EVDEV] Found keyboard: {} ({})",
                            path.display(),
                            device.name().unwrap_or("unknown")
                        );
                        keyboards.push(path);
                    }
                }
            }
        }
    }

    if keyboards.is_empty() {
        return Err("No keyboard devices found with required keys".to_string());
    }

    Ok(keyboards)
}

pub async fn start_evdev_listener(app_handle: AppHandle) -> Result<(), String> {
    println!("[EVDEV] Starting keyboard listener for push-to-talk...");

    let keyboards = find_keyboard_devices()?;
    println!("[EVDEV] Monitoring {} keyboard device(s)", keyboards.len());

    let ctrl_pressed = Arc::new(AtomicBool::new(false));
    let space_pressed = Arc::new(AtomicBool::new(false));
    let is_recording = Arc::new(AtomicBool::new(false)); // Track recording state
    let device_states: Arc<Mutex<HashMap<PathBuf, Arc<DeviceState>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    for keyboard_path in keyboards {
        let state = Arc::new(DeviceState {
            path: keyboard_path.clone(),
            reconnect_attempts: AtomicU32::new(0),
            is_active: AtomicBool::new(true),
        });

        device_states
            .lock()
            .await
            .insert(keyboard_path.clone(), state.clone());

        spawn_device_monitor(
            keyboard_path,
            app_handle.clone(),
            ctrl_pressed.clone(),
            space_pressed.clone(),
            is_recording.clone(),
            state,
            device_states.clone(),
        );
    }

    println!("[EVDEV] ✅ Push-to-talk listener started!");
    println!("[EVDEV] Hold Ctrl+Space to record, release to transcribe and paste");

    Ok(())
}

fn spawn_device_monitor(
    device_path: PathBuf,
    app_handle: AppHandle,
    ctrl_pressed: Arc<AtomicBool>,
    space_pressed: Arc<AtomicBool>,
    is_recording: Arc<AtomicBool>,
    state: Arc<DeviceState>,
    device_states: Arc<Mutex<HashMap<PathBuf, Arc<DeviceState>>>>,
) {
    let path_clone = device_path.clone();

    tokio::task::spawn_blocking(move || {
        monitor_keyboard_device_with_reconnect(
            path_clone,
            app_handle,
            ctrl_pressed,
            space_pressed,
            is_recording,
            state,
            device_states,
        );
    });
}

fn monitor_keyboard_device_with_reconnect(
    device_path: PathBuf,
    app_handle: AppHandle,
    ctrl_pressed: Arc<AtomicBool>,
    space_pressed: Arc<AtomicBool>,
    is_recording: Arc<AtomicBool>,
    state: Arc<DeviceState>,
    device_states: Arc<Mutex<HashMap<PathBuf, Arc<DeviceState>>>>,
) {
    loop {
        if !state.is_active.load(Ordering::SeqCst) {
            println!("[EVDEV] Device {} marked inactive, stopping monitor", device_path.display());
            return;
        }

        match monitor_keyboard_device(
            &device_path,
            &app_handle,
            &ctrl_pressed,
            &space_pressed,
            &is_recording,
        ) {
            Ok(()) => {
                println!("[EVDEV] Device {} monitor exited cleanly", device_path.display());
                return;
            }
            Err(e) => {
                let attempts = state.reconnect_attempts.fetch_add(1, Ordering::SeqCst) + 1;

                if attempts > MAX_RECONNECT_ATTEMPTS {
                    eprintln!(
                        "[EVDEV] Max reconnect attempts ({}) reached for {}, giving up",
                        MAX_RECONNECT_ATTEMPTS,
                        device_path.display()
                    );
                    state.is_active.store(false, Ordering::SeqCst);
                    return;
                }

                let delay = RECONNECT_DELAY_MS * 2u64.pow(attempts - 1);
                println!(
                    "[EVDEV] Device {} error: {}. Reconnecting in {}ms (attempt {}/{})",
                    device_path.display(),
                    e,
                    delay,
                    attempts,
                    MAX_RECONNECT_ATTEMPTS
                );

                // DON'T reset any shared state - other keyboards are still active!
                // The key states and recording state are shared across all devices

                std::thread::sleep(Duration::from_millis(delay));

                if !device_path.exists() {
                    println!(
                        "[EVDEV] Device {} no longer exists, waiting for reconnect...",
                        device_path.display()
                    );
                    continue;
                }

                println!("[EVDEV] Attempting to reconnect to {}...", device_path.display());
            }
        }
    }
}

fn monitor_keyboard_device(
    device_path: &PathBuf,
    app_handle: &AppHandle,
    ctrl_pressed: &Arc<AtomicBool>,
    space_pressed: &Arc<AtomicBool>,
    is_recording: &Arc<AtomicBool>,
) -> Result<(), String> {
    let mut device = Device::open(device_path)
        .map_err(|e| format!("Failed to open {}: {}", device_path.display(), e))?;

    let device_name = device.name().unwrap_or("unknown").to_string();
    println!(
        "[EVDEV] Connected to: {} ({})",
        device_path.display(),
        device_name
    );

    loop {
        let events = device.fetch_events().map_err(|e| {
            format!("Device {} lost: {}", device_name, e)
        })?;

        for event in events {
            if let InputEventKind::Key(key) = event.kind() {
                let pressed = event.value() == 1;
                let released = event.value() == 0;

                if key == KEY_LEFTCTRL || key == KEY_RIGHTCTRL {
                    if pressed {
                        ctrl_pressed.store(true, Ordering::SeqCst);
                    } else if released {
                        ctrl_pressed.store(false, Ordering::SeqCst);
                    }
                }

                if key == KEY_SPACE {
                    let ctrl_is_pressed = ctrl_pressed.load(Ordering::SeqCst);

                    if pressed && !space_pressed.load(Ordering::SeqCst) {
                        space_pressed.store(true, Ordering::SeqCst);

                        if ctrl_is_pressed && !is_recording.load(Ordering::SeqCst) {
                            println!("[EVDEV] Ctrl+Space PRESSED - starting cpal recording");
                            is_recording.store(true, Ordering::SeqCst);

                            // Show overlay
                            if let Some(overlay_win) = app_handle.get_webview_window("overlay") {
                                let _ = overlay_win.show();
                                let _ = overlay_win.set_focus();
                            }

                            // Start recording via cpal
                            let app_clone = app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = app_clone.state::<AppState>();
                                match do_start_recording(app_clone.clone(), &state).await {
                                    Ok(_) => println!("[EVDEV] ✅ cpal recording started"),
                                    Err(e) => println!("[EVDEV] ❌ Failed to start recording: {}", e),
                                }
                            });
                        }
                    } else if released {
                        space_pressed.store(false, Ordering::SeqCst);

                        // Stop recording if active (check is_recording, not key states)
                        if is_recording.load(Ordering::SeqCst) {
                            println!("[EVDEV] Space RELEASED while recording - stopping cpal recording");
                            is_recording.store(false, Ordering::SeqCst);

                            // Stop recording, transcribe, and type
                            let app_clone = app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = app_clone.state::<AppState>();

                                match do_stop_recording(app_clone.clone(), &state).await {
                                    Ok(transcription) => {
                                        println!("[EVDEV] ✅ Transcription: '{}'", transcription);
                                        if !transcription.is_empty() {
                                            // Hide overlay before typing
                                            if let Some(overlay_win) = app_clone.get_webview_window("overlay") {
                                                let _ = overlay_win.hide();
                                            }
                                            // Longer delay for Wayland focus to return to previous window
                                            println!("[EVDEV] 💤 Waiting 400ms for focus to return...");
                                            tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;

                                            // Type the text
                                            println!("[EVDEV] 📝 Pasting text now...");
                                            if let Err(e) = do_type_text(transcription).await {
                                                println!("[EVDEV] ❌ Failed to type text: {}", e);
                                            } else {
                                                println!("[EVDEV] ✅ Text pasted successfully!");
                                            }
                                        } else {
                                            println!("[EVDEV] No speech detected");
                                            if let Some(overlay_win) = app_clone.get_webview_window("overlay") {
                                                let _ = overlay_win.hide();
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        println!("[EVDEV] ❌ Failed to stop recording: {}", e);
                                        if let Some(overlay_win) = app_clone.get_webview_window("overlay") {
                                            let _ = overlay_win.hide();
                                        }
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
