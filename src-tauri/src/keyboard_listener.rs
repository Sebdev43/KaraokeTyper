//! Platform dispatch for the recording shortcut: evdev on Linux, rdev elsewhere. Both
//! hand their key events to `shortcut_session`, which owns everything after that.

use tauri::AppHandle;

// On Windows/macOS: rdev-based implementation
#[cfg(not(target_os = "linux"))]
use crate::shortcut_activation::{KeyEvent, SharedMachine};
#[cfg(not(target_os = "linux"))]
use crate::shortcut_session::{feed_key, spawn_effect_executor};
#[cfg(not(target_os = "linux"))]
use rdev::{listen, Event, EventType, Key};
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "linux"))]
use std::sync::Arc;

/// Start the cross-platform keyboard listener
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

/// rdev-based keyboard listener for Windows/macOS: Ctrl+Space, fed to the machine from
/// the rdev thread itself so every edge carries its real timestamp.
#[cfg(not(target_os = "linux"))]
async fn start_rdev_listener(app_handle: AppHandle) -> Result<(), String> {
    crate::dev_log!("[KEYBOARD] Using rdev for keyboard capture");

    let machine = SharedMachine::default();
    let effects = spawn_effect_executor(app_handle.clone(), machine.clone());

    // Shared state for modifier keys
    let ctrl_pressed = Arc::new(AtomicBool::new(false));
    let space_pressed = Arc::new(AtomicBool::new(false));

    // Spawn the rdev listener in a blocking thread
    std::thread::spawn(move || {
        crate::dev_log!("[KEYBOARD] rdev listener thread started");

        let callback = move |event: Event| {
            let key_event = match event.event_type {
                EventType::KeyPress(key) => handle_key_press(key, &ctrl_pressed, &space_pressed),
                EventType::KeyRelease(key) => {
                    handle_key_release(key, &ctrl_pressed, &space_pressed)
                }
                _ => None,
            };
            if let Some(key_event) = key_event {
                feed_key(&app_handle, &machine, &effects, key_event);
            }
        };

        // Start listening - this blocks
        if let Err(e) = listen(callback) {
            crate::dev_log!("[KEYBOARD] Error in rdev listener: {:?}", e);
        }
    });

    crate::dev_log!("[KEYBOARD] ✅ Shortcut listener started!");
    Ok(())
}

/// Handle key press events (Windows/macOS only)
#[cfg(not(target_os = "linux"))]
fn handle_key_press(
    key: Key,
    ctrl_pressed: &Arc<AtomicBool>,
    space_pressed: &Arc<AtomicBool>,
) -> Option<KeyEvent> {
    match key {
        Key::ControlLeft | Key::ControlRight => {
            ctrl_pressed.store(true, Ordering::SeqCst);
            None
        }
        Key::Space => {
            if space_pressed.load(Ordering::SeqCst) {
                return None; // auto-repeat
            }
            space_pressed.store(true, Ordering::SeqCst);
            // Only Ctrl+Space is the shortcut
            ctrl_pressed
                .load(Ordering::SeqCst)
                .then_some(KeyEvent::Pressed)
        }
        _ => None,
    }
}

/// Handle key release events (Windows/macOS only)
#[cfg(not(target_os = "linux"))]
fn handle_key_release(
    key: Key,
    ctrl_pressed: &Arc<AtomicBool>,
    space_pressed: &Arc<AtomicBool>,
) -> Option<KeyEvent> {
    match key {
        Key::ControlLeft | Key::ControlRight => {
            ctrl_pressed.store(false, Ordering::SeqCst);
            None
        }
        Key::Space => {
            if !space_pressed.load(Ordering::SeqCst) {
                return None;
            }
            space_pressed.store(false, Ordering::SeqCst);
            // Sent regardless of Ctrl (often let go before Space): the machine ignores a
            // release that has nothing to end, and a hold-or-toggle recording must see
            // its release even after Ctrl went up.
            Some(KeyEvent::Released)
        }
        _ => None,
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
