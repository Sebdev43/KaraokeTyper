use rdev::{listen, Event, EventType, Key};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

/// Cross-platform keyboard listener using rdev
/// Works on Linux (X11/Wayland*), Windows, and macOS
///
/// *Note: On Wayland, rdev may have limitations. For full Wayland support,
/// consider using evdev as a fallback (Linux-only, requires input group permissions)
#[allow(dead_code)] // Used on Windows/macOS, Linux uses evdev_listener
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HotkeyEvent {
    PushToTalkPressed,
    PushToTalkReleased,
}

/// Start the cross-platform keyboard listener for push-to-talk
#[allow(dead_code)] // Used on Windows/macOS, Linux uses evdev_listener
pub async fn start_keyboard_listener(app_handle: AppHandle) -> Result<(), String> {
    println!("[KEYBOARD] Starting cross-platform keyboard listener...");
    println!("[KEYBOARD] Platform: {}", std::env::consts::OS);

    // Channel for communicating hotkey events to async context
    let (tx, mut rx) = mpsc::unbounded_channel::<HotkeyEvent>();

    // Shared state for modifier keys
    let ctrl_pressed = Arc::new(AtomicBool::new(false));
    let space_pressed = Arc::new(AtomicBool::new(false));

    let ctrl_pressed_clone = ctrl_pressed.clone();
    let space_pressed_clone = space_pressed.clone();

    // Spawn the rdev listener in a blocking thread
    std::thread::spawn(move || {
        println!("[KEYBOARD] rdev listener thread started");

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
            eprintln!("[KEYBOARD] Error in rdev listener: {:?}", e);

            // On Linux Wayland, rdev might fail. Log helpful message.
            #[cfg(target_os = "linux")]
            {
                eprintln!("[KEYBOARD] ⚠️  If on Wayland, you may need to:");
                eprintln!(
                    "[KEYBOARD]    1. Add user to 'input' group: sudo usermod -aG input $USER"
                );
                eprintln!("[KEYBOARD]    2. Log out and log back in");
                eprintln!("[KEYBOARD]    3. Or use X11 session instead of Wayland");
            }
        }
    });

    // Spawn async task to handle hotkey events
    let app_clone = app_handle.clone();
    tokio::spawn(async move {
        println!("[KEYBOARD] ✅ Push-to-talk listener started!");
        println!("[KEYBOARD] Hold Ctrl+Space to record, release to transcribe and paste");

        while let Some(event) = rx.recv().await {
            match event {
                HotkeyEvent::PushToTalkPressed => {
                    println!("[KEYBOARD] 🎤 Ctrl+Space PRESSED - starting recording");

                    // Start recording using helper function
                    let state = app_clone.state::<crate::AppState>();
                    if let Err(e) = crate::do_start_recording(app_clone.clone(), &state).await {
                        eprintln!("[KEYBOARD] Failed to start recording: {}", e);
                    }

                    // Show overlay
                    if let Some(overlay_win) = app_clone.get_webview_window("overlay") {
                        let _ = overlay_win.show();
                        let _ = overlay_win.set_focus();
                        let _ = overlay_win.emit("recording-started", ());
                    }
                }
                HotkeyEvent::PushToTalkReleased => {
                    println!("[KEYBOARD] 🛑 Ctrl+Space RELEASED - stopping recording");

                    // Stop recording and transcribe using helper function
                    let state = app_clone.state::<crate::AppState>();
                    match crate::do_stop_recording(app_clone.clone(), &state).await {
                        Ok(transcription) => {
                            println!("[KEYBOARD] Transcription: {}", transcription);

                            // Wait for physical keys to fully release
                            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                            // Type the text using helper function
                            match crate::do_type_text(transcription).await {
                                Ok(_) => {
                                    println!("[KEYBOARD] ✅ Text pasted successfully");
                                    if let Some(overlay_win) =
                                        app_clone.get_webview_window("overlay")
                                    {
                                        let _ = overlay_win.emit("recording-stopped", ());
                                        let _ = overlay_win.hide();
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[KEYBOARD] Failed to paste text: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[KEYBOARD] Failed to transcribe: {}", e);
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

/// Handle key press events
#[allow(dead_code)] // Used on Windows/macOS
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

/// Handle key release events
#[allow(dead_code)] // Used on Windows/macOS
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
            Then log out and log back in.".to_string());
    }

    Ok(())
}

/// Windows/macOS: No special permissions needed
#[cfg(not(target_os = "linux"))]
pub fn check_linux_permissions() -> Result<(), String> {
    Ok(())
}
