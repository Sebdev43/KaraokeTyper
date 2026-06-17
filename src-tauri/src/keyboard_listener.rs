// On Linux, only basic imports (evdev is used instead of rdev)
#[cfg(target_os = "linux")]
use tauri::AppHandle;

// On Windows/macOS: full rdev-based implementation
#[cfg(not(target_os = "linux"))]
use rdev::{listen, Event, EventType, Key};
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "linux"))]
use std::sync::Arc;
#[cfg(not(target_os = "linux"))]
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(target_os = "linux"))]
use tokio::sync::mpsc;

/// Hotkey events for push-to-talk (used on Windows/macOS only)
#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotkeyEvent {
    PushToTalkPressed,
    PushToTalkReleased,
}

/// Start the cross-platform keyboard listener for push-to-talk
/// On Linux: Uses evdev for best Wayland compatibility
/// On Windows/macOS: Uses rdev
pub async fn start_keyboard_listener(app_handle: AppHandle) -> Result<(), String> {
    crate::dev_log!("[KEYBOARD] Starting cross-platform keyboard listener...");
    crate::dev_log!("[KEYBOARD] Platform: {}", std::env::consts::OS);

    // On Linux, use evdev for better Wayland support
    #[cfg(target_os = "linux")]
    {
        crate::dev_log!("[KEYBOARD] Linux detected - using evdev for reliable Wayland support");
        crate::evdev_listener::start_evdev_listener(app_handle).await
    }

    // On Windows/macOS, use rdev
    #[cfg(not(target_os = "linux"))]
    {
        start_rdev_listener(app_handle).await
    }
}

/// rdev-based keyboard listener for Windows/macOS
#[cfg(not(target_os = "linux"))]
async fn start_rdev_listener(app_handle: AppHandle) -> Result<(), String> {
    crate::dev_log!("[KEYBOARD] Using rdev for keyboard capture");

    // Channel for communicating hotkey events to async context
    let (tx, mut rx) = mpsc::unbounded_channel::<HotkeyEvent>();

    // Shared state for modifier keys
    let ctrl_pressed = Arc::new(AtomicBool::new(false));
    let space_pressed = Arc::new(AtomicBool::new(false));

    let ctrl_pressed_clone = ctrl_pressed.clone();
    let space_pressed_clone = space_pressed.clone();

    // Spawn the rdev listener in a blocking thread
    std::thread::spawn(move || {
        crate::dev_log!("[KEYBOARD] rdev listener thread started");

        let callback = move |event: Event| match event.event_type {
            EventType::KeyPress(key) => {
                handle_key_press(key, &ctrl_pressed_clone, &space_pressed_clone, &tx);
            }
            EventType::KeyRelease(key) => {
                handle_key_release(key, &ctrl_pressed_clone, &space_pressed_clone, &tx);
            }
            _ => {}
        };

        // Start listening - this blocks
        if let Err(e) = listen(callback) {
            crate::dev_log!("[KEYBOARD] Error in rdev listener: {:?}", e);
        }
    });

    // Spawn async task to handle hotkey events
    let app_clone = app_handle.clone();
    tokio::spawn(async move {
        crate::dev_log!("[KEYBOARD] ✅ Push-to-talk listener started!");
        crate::dev_log!("[KEYBOARD] Hold Ctrl+Space to record, release to transcribe and paste");

        while let Some(event) = rx.recv().await {
            match event {
                HotkeyEvent::PushToTalkPressed => {
                    crate::dev_log!("[KEYBOARD] 🎤 Ctrl+Space PRESSED - starting recording");

                    // Start recording using helper function
                    let state = app_clone.state::<crate::AppState>();
                    if let Err(e) = crate::do_start_recording(app_clone.clone(), &state).await {
                        crate::dev_log!("[KEYBOARD] Failed to start recording: {}", e);
                    }

                    // Show overlay centered on cursor's monitor
                    crate::show_overlay_with_state(&app_clone, "recording");
                }
                HotkeyEvent::PushToTalkReleased => {
                    crate::dev_log!("[KEYBOARD] Ctrl+Space RELEASED - stopping recording");

                    // Switch overlay to processing state
                    crate::show_overlay_with_state(&app_clone, "processing");

                    // Stop recording and transcribe using helper function
                    let state = app_clone.state::<crate::AppState>();
                    match crate::do_stop_recording(app_clone.clone(), &state).await {
                        Ok(transcription) => {
                            crate::dev_log!("[KEYBOARD] Transcription: {}", transcription);

                            // Wait for physical keys to fully release
                            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                            // Paste BEFORE hiding overlay (like Handy) — target app
                            // keeps focus while overlay is visible, no chars lost.
                            // Read paste_method from the in-memory config cache
                            // (no per-paste disk read / GPU-migration write).
                            let paste_method = state
                                .config
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .paste_method
                                .clone();
                            let paste_result =
                                crate::do_type_text(transcription, &app_clone, paste_method).await;

                            // Hide overlay AFTER paste completes
                            crate::hide_overlay_animated(&app_clone);

                            match paste_result {
                                Ok(_) => {
                                    crate::dev_log!("[KEYBOARD] Text pasted successfully");
                                }
                                Err(e) => {
                                    crate::dev_log!("[KEYBOARD] Failed to paste text: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            crate::dev_log!("[KEYBOARD] Failed to transcribe: {}", e);
                            crate::hide_overlay_animated(&app_clone);
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

/// Handle key press events (Windows/macOS only)
#[cfg(not(target_os = "linux"))]
fn handle_key_press(
    key: Key,
    ctrl_pressed: &Arc<AtomicBool>,
    space_pressed: &Arc<AtomicBool>,
    tx: &mpsc::UnboundedSender<HotkeyEvent>,
) {
    match key {
        Key::ControlLeft | Key::ControlRight => {
            ctrl_pressed.store(true, Ordering::SeqCst);
        }
        Key::Space => {
            if !space_pressed.load(Ordering::SeqCst) {
                space_pressed.store(true, Ordering::SeqCst);

                // Check if Ctrl is held
                if ctrl_pressed.load(Ordering::SeqCst) {
                    let _ = tx.send(HotkeyEvent::PushToTalkPressed);
                }
            }
        }
        _ => {}
    }
}

/// Handle key release events (Windows/macOS only)
#[cfg(not(target_os = "linux"))]
fn handle_key_release(
    key: Key,
    ctrl_pressed: &Arc<AtomicBool>,
    space_pressed: &Arc<AtomicBool>,
    tx: &mpsc::UnboundedSender<HotkeyEvent>,
) {
    match key {
        Key::ControlLeft | Key::ControlRight => {
            ctrl_pressed.store(false, Ordering::SeqCst);
        }
        Key::Space => {
            if space_pressed.load(Ordering::SeqCst) {
                // Only trigger release if Ctrl was being held (i.e., we were recording)
                let was_ctrl_pressed = ctrl_pressed.load(Ordering::SeqCst);
                space_pressed.store(false, Ordering::SeqCst);

                if was_ctrl_pressed {
                    let _ = tx.send(HotkeyEvent::PushToTalkReleased);
                }
            }
        }
        _ => {}
    }
}

/// Linux-specific: Check if user has permissions for input devices
#[cfg(target_os = "linux")]
pub fn check_linux_permissions() -> Result<(), String> {
    use std::process::Command;

    // Check if user is in input group
    let output = Command::new("groups")
        .output()
        .map_err(|e| format!("Failed to check groups: {}", e))?;

    let groups = String::from_utf8_lossy(&output.stdout);

    if !groups.contains("input") {
        return Err("User is not in 'input' group. Run:\n\
            sudo usermod -aG input $USER\n\
            Then log out and log back in."
            .to_string());
    }

    Ok(())
}

/// Windows/macOS: No special permissions needed
#[cfg(not(target_os = "linux"))]
pub fn check_linux_permissions() -> Result<(), String> {
    Ok(())
}
