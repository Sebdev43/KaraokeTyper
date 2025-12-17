use tauri::{AppHandle, Emitter, Manager};
use zbus::{interface, Connection, Result};
use zbus::fdo;

pub struct KaraokeTyperService {
    app_handle: AppHandle,
}

#[interface(name = "com.karaoketyper.Control")]
impl KaraokeTyperService {
    /// Start push-to-talk recording
    async fn start_recording(&self) -> fdo::Result<String> {
        println!("[DBUS] start_recording called");

        if let Some(overlay_win) = self.app_handle.get_webview_window("overlay") {
            let _ = overlay_win.show();
            let _ = overlay_win.set_focus();
            let _ = overlay_win.emit("recording-started", ());

            // Start recording via Tauri command
            let app_clone = self.app_handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::start_recording(app_clone.clone(), app_clone.state::<crate::AppState>()).await {
                    eprintln!("[DBUS] Failed to start recording: {}", e);
                }
            });

            Ok("recording_started".to_string())
        } else {
            Err(fdo::Error::Failed("Overlay window not found".to_string()))
        }
    }

    /// Stop recording and transcribe
    async fn stop_recording(&self) -> fdo::Result<String> {
        println!("[DBUS] stop_recording called");

        let app_clone = self.app_handle.clone();

        tauri::async_runtime::spawn(async move {
            match crate::stop_recording(app_clone.clone(), app_clone.state::<crate::AppState>()).await {
                Ok(transcription) => {
                    println!("[DBUS] Transcription: {}", transcription);

                    // Type the text
                    match crate::type_text(transcription).await {
                        Ok(_) => {
                            println!("[DBUS] Text typed successfully");
                            if let Some(overlay_win) = app_clone.get_webview_window("overlay") {
                                let _ = overlay_win.emit("recording-stopped", ());
                                let _ = overlay_win.hide();
                            }
                        }
                        Err(e) => eprintln!("[DBUS] Failed to type text: {}", e),
                    }
                }
                Err(e) => eprintln!("[DBUS] Failed to transcribe: {}", e),
            }
        });

        Ok("recording_stopped".to_string())
    }

    /// Toggle the overlay window visibility (legacy - kept for backward compatibility)
    /// Now uses push-to-talk mode: starts recording if not visible, stops if visible
    async fn toggle_overlay(&self) -> fdo::Result<String> {
        println!("[DBUS] toggle_overlay called - using push-to-talk mode");

        if let Some(overlay_win) = self.app_handle.get_webview_window("overlay") {
            match overlay_win.is_visible() {
                Ok(is_visible) => {
                    if is_visible {
                        // Already recording, stop it
                        self.stop_recording().await
                    } else {
                        // Start recording
                        self.start_recording().await
                    }
                }
                Err(e) => {
                    eprintln!("[DBUS] Failed to get overlay visibility: {}", e);
                    Err(fdo::Error::Failed(format!("Failed to get visibility: {}", e)))
                }
            }
        } else {
            eprintln!("[DBUS] Overlay window not found");
            Err(fdo::Error::Failed("Overlay window not found".to_string()))
        }
    }

    /// Show the overlay window (legacy - kept for backward compatibility)
    async fn show_overlay(&self) -> fdo::Result<String> {
        println!("[DBUS] show_overlay called - redirecting to start_recording");
        self.start_recording().await
    }

    /// Hide the overlay window (legacy - kept for backward compatibility)
    async fn hide_overlay(&self) -> fdo::Result<String> {
        println!("[DBUS] hide_overlay called - redirecting to stop_recording");
        self.stop_recording().await
    }
}

/// Start the DBus service in a background task
pub async fn start_dbus_service(app_handle: AppHandle) -> Result<()> {
    println!("[DBUS] Starting DBus service...");

    let connection = Connection::session().await?;
    println!("[DBUS] Connected to session bus");

    let service = KaraokeTyperService {
        app_handle: app_handle.clone(),
    };

    // Request well-known name
    connection
        .object_server()
        .at("/com/karaoketyper/Control", service)
        .await?;

    connection
        .request_name("com.karaoketyper.Control")
        .await?;

    println!("[DBUS] ✅ DBus service registered at com.karaoketyper.Control");
    println!("[DBUS] Test with: busctl --user call com.karaoketyper.Control /com/karaoketyper/Control com.karaoketyper.Control ToggleOverlay");

    // Keep the connection alive
    std::mem::forget(connection);

    Ok(())
}
