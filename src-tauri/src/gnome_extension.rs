use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Type)]
pub struct GnomeExtensionStatus {
    pub installed: bool,
    pub enabled: bool,
    pub is_gnome: bool,
    pub is_wayland: bool,
    pub path: String,
    pub needs_restart: bool,
}

/// Write the two-file archive that `gnome-extensions install` expects.
///
/// Split out of `install_focused_window_extension` so it can be tested: the
/// surrounding command shells out to `gnome-extensions` and needs a GNOME
/// session, while this part is pure file writing and is the only thing in the
/// project that uses the `zip` crate — a silent change in how it stores entries
/// would leave an archive GNOME rejects, with no test to catch it.
///
/// Entries are stored uncompressed: the payload is two small text files, and
/// GNOME reads the archive once at install time.
#[cfg(target_os = "linux")]
fn write_extension_zip(
    zip_path: &std::path::Path,
    extension_js: &str,
    metadata_json: &str,
) -> Result<(), String> {
    use std::io::Write;

    let zip_file =
        std::fs::File::create(zip_path).map_err(|e| format!("Failed to create zip: {}", e))?;
    let mut zip = zip::ZipWriter::new(zip_file);

    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for (name, contents) in [
        ("extension.js", extension_js),
        ("metadata.json", metadata_json),
    ] {
        zip.start_file(name, options)
            .map_err(|e| format!("Failed to add {} to zip: {}", name, e))?;
        zip.write_all(contents.as_bytes())
            .map_err(|e| format!("Failed to write {}: {}", name, e))?;
    }

    zip.finish()
        .map_err(|e| format!("Failed to finish zip: {}", e))?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn get_gnome_extension_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gnome-shell")
        .join("extensions")
        .join("focused-window-dbus@karaoketyper")
}

#[tauri::command]
#[specta::specta]
pub async fn check_focused_window_extension() -> Result<GnomeExtensionStatus, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            use std::process::Command;

            let extension_path = get_gnome_extension_path();
            let is_installed = extension_path.join("extension.js").exists();

            let is_enabled = if is_installed {
                let (xdg_runtime, dbus_addr) = crate::get_dbus_env();
                Command::new("gnome-extensions")
                    .args(["info", "focused-window-dbus@karaoketyper"])
                    .env("XDG_RUNTIME_DIR", &xdg_runtime)
                    .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                    .output()
                    .map(|o| {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        stdout.contains("State: ACTIVE") || stdout.contains("State: ENABLED")
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            let is_gnome = std::env::var("XDG_CURRENT_DESKTOP")
                .map(|d| d.to_lowercase().contains("gnome"))
                .unwrap_or(false);

            let is_wayland = std::env::var("XDG_SESSION_TYPE")
                .map(|s| s == "wayland")
                .unwrap_or(false);

            GnomeExtensionStatus {
                installed: is_installed,
                enabled: is_enabled,
                is_gnome,
                is_wayland,
                path: extension_path.to_string_lossy().to_string(),
                needs_restart: is_installed && !is_enabled,
            }
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(GnomeExtensionStatus {
            installed: false,
            enabled: false,
            is_gnome: false,
            is_wayland: false,
            path: String::new(),
            needs_restart: false,
        })
    }
}

#[tauri::command]
#[specta::specta]
pub async fn install_focused_window_extension(app_handle: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;

        let resource_path = app_handle
            .path()
            .resource_dir()
            .map_err(|e| format!("Failed to get resource dir: {}", e))?
            .join("focused-window-dbus");

        let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("focused-window-dbus");

        let source_path = if resource_path.exists() {
            resource_path.clone()
        } else if dev_path.exists() {
            dev_path.clone()
        } else {
            return Err(format!(
                "Extension files not found. Checked:\n  - {}\n  - {}",
                resource_path.display(),
                dev_path.display()
            ));
        };

        tokio::task::spawn_blocking(move || {
            let extension_js = fs::read_to_string(source_path.join("extension.js"))
                .map_err(|e| format!("Failed to read extension.js: {}", e))?;
            let metadata_json = fs::read_to_string(source_path.join("metadata.json"))
                .map_err(|e| format!("Failed to read metadata.json: {}", e))?;

            let temp_dir = std::env::temp_dir();
            let zip_path = temp_dir.join("focused-window-dbus@karaoketyper.zip");

            write_extension_zip(&zip_path, &extension_js, &metadata_json)?;

            crate::dev_log!("[GNOME EXT] Created zip at: {}", zip_path.display());

            let (xdg_runtime, dbus_addr) = crate::get_dbus_env();

            let install_result = std::process::Command::new("gnome-extensions")
                .args(["install", "--force", &zip_path.to_string_lossy()])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output()
                .map_err(|e| format!("Failed to run gnome-extensions install: {}", e))?;

            let _ = fs::remove_file(&zip_path);

            if !install_result.status.success() {
                let stderr = String::from_utf8_lossy(&install_result.stderr);
                return Err(format!("gnome-extensions install failed: {}", stderr));
            }

            crate::dev_log!("[GNOME EXT] Extension installed via gnome-extensions");

            std::thread::sleep(std::time::Duration::from_millis(500));

            let enable_result = std::process::Command::new("gnome-extensions")
                .args(["enable", "focused-window-dbus@karaoketyper"])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output();

            let cli_enabled = matches!(&enable_result, Ok(output) if output.status.success());

            let ext_uuid = "focused-window-dbus@karaoketyper";
            let current = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.shell", "enabled-extensions"])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| "@as []".to_string());

            if !current.contains(ext_uuid) {
                let trimmed = current.trim();
                let new_list = if trimmed == "@as []" || trimmed == "[]" {
                    format!("['{}']", ext_uuid)
                } else {
                    let without_bracket = trimmed.trim_end_matches(']');
                    format!("{}, '{}']", without_bracket, ext_uuid)
                };

                crate::dev_log!("[GNOME EXT] Setting enabled-extensions to: {}", new_list);
                let _ = std::process::Command::new("gsettings")
                    .args(["set", "org.gnome.shell", "enabled-extensions", &new_list])
                    .env("XDG_RUNTIME_DIR", &xdg_runtime)
                    .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                    .output();
            }

            if cli_enabled {
                crate::dev_log!("[GNOME EXT] Extension enabled");
                let logout_result = std::process::Command::new("gnome-session-quit")
                    .args(["--logout", "--no-prompt"])
                    .env("XDG_RUNTIME_DIR", &xdg_runtime)
                    .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                    .spawn();

                match logout_result {
                    Ok(_) => Ok(crate::i18n::msg::gnome_installed_logging_out().to_string()),
                    Err(e) => {
                        crate::dev_log!("[GNOME EXT] Logout failed: {}", e);
                        Ok(crate::i18n::msg::gnome_installed_log_out_manually().to_string())
                    }
                }
            } else {
                Ok(crate::i18n::msg::gnome_installed_relogin().to_string())
            }
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))?
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = app_handle;
        Err("GNOME extensions are only available on Linux".to_string())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn remove_focused_window_extension() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            use std::fs;

            let extension_path = get_gnome_extension_path();

            if !extension_path.exists() {
                return Ok("Extension was not installed".to_string());
            }

            let (xdg_runtime, dbus_addr) = crate::get_dbus_env();
            let _ = std::process::Command::new("gnome-extensions")
                .args(["disable", "focused-window-dbus@karaoketyper"])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output();

            fs::remove_dir_all(&extension_path)
                .map_err(|e| format!("Failed to remove extension: {}", e))?;

            crate::dev_log!(
                "[GNOME EXT] Extension removed: {}",
                extension_path.display()
            );

            Ok("Extension removed!\n\nLog out and back in to complete the removal.".to_string())
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))?
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("GNOME extensions are only available on Linux".to_string())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::write_extension_zip;
    use std::io::Read;

    /// The archive GNOME receives must hold both files, under exactly these
    /// names and byte for byte. Remove either `start_file` call and this fails.
    #[test]
    fn the_archive_holds_both_files_unchanged() {
        let dir = std::env::temp_dir().join(format!("karaoketyper-ziptest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let zip_path = dir.join("extension.zip");

        let js = "// accents: é à ü, and a brace }\nlet x = 1;\n";
        let meta = r#"{"uuid":"focused-window-dbus@karaoketyper","name":"Focused Window"}"#;

        write_extension_zip(&zip_path, js, meta).expect("writing the archive");

        let file = std::fs::File::open(&zip_path).expect("reopening the archive");
        let mut archive = zip::ZipArchive::new(file).expect("the archive is readable");

        let names: Vec<String> = archive.file_names().map(str::to_string).collect();
        assert_eq!(names.len(), 2, "exactly two entries, got {:?}", names);
        assert!(names.contains(&"extension.js".to_string()), "{:?}", names);
        assert!(names.contains(&"metadata.json".to_string()), "{:?}", names);

        for (name, expected) in [("extension.js", js), ("metadata.json", meta)] {
            let mut entry = archive.by_name(name).expect(name);
            let mut got = String::new();
            entry.read_to_string(&mut got).expect("reading the entry");
            assert_eq!(got, expected, "{} came back altered", name);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A path that cannot be created has to surface as an error rather than
    /// leaving the caller to install an archive that was never written.
    #[test]
    fn an_unwritable_path_is_reported() {
        let err = write_extension_zip(
            std::path::Path::new("/proc/self/definitely-not-writable/x.zip"),
            "js",
            "{}",
        )
        .expect_err("writing under /proc must fail");
        assert!(err.contains("Failed to create zip"), "{}", err);
    }
}
