use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize, Type)]
pub struct AppDataItem {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub size_mb: f64,
}

#[derive(Clone, Serialize, Type)]
pub struct AppDataInfo {
    pub items: Vec<AppDataItem>,
    pub total_size_mb: f64,
}

fn get_app_data_paths() -> Vec<(String, std::path::PathBuf)> {
    let mut paths = Vec::new();

    if let Some(config_dir) = dirs::config_dir() {
        paths.push(("Config".to_string(), config_dir.join("karaoketyper")));
    }

    if let Some(data_dir) = dirs::data_dir() {
        paths.push(("Data (venv)".to_string(), data_dir.join("karaoketyper")));
    }

    if let Some(config_dir) = dirs::config_dir() {
        paths.push((
            "Autostart".to_string(),
            config_dir.join("autostart").join("karaoketyper.desktop"),
        ));
    }

    if let Some(cache_dir) = dirs::cache_dir() {
        let hf_hub = cache_dir.join("huggingface").join("hub");
        if hf_hub.exists() {
            if let Ok(entries) = std::fs::read_dir(&hf_hub) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("models--csukuangfj--sherpa-onnx") {
                        paths.push((format!("Model: {}", name), entry.path()));
                    }
                }
            }
        }
    }

    paths
}

#[tauri::command]
#[specta::specta]
pub fn get_app_data_info() -> Result<AppDataInfo, String> {
    let paths = get_app_data_paths();
    let mut total_size: u64 = 0;
    let mut items = Vec::new();

    for (name, path) in &paths {
        let exists = path.exists();
        let size = if exists {
            calculate_dir_size(path).unwrap_or(0)
        } else {
            0
        };
        total_size += size;

        items.push(AppDataItem {
            name: name.clone(),
            path: path.to_string_lossy().to_string(),
            exists,
            size_mb: size as f64 / 1024.0 / 1024.0,
        });
    }

    Ok(AppDataInfo {
        items,
        total_size_mb: total_size as f64 / 1024.0 / 1024.0,
    })
}

fn calculate_dir_size(path: &std::path::Path) -> std::io::Result<u64> {
    let mut size = 0;
    if path.is_file() {
        return Ok(std::fs::metadata(path)?.len());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                size += std::fs::metadata(&path)?.len();
            } else if path.is_dir() {
                size += calculate_dir_size(&path)?;
            }
        }
    }
    Ok(size)
}

#[tauri::command]
#[specta::specta]
pub async fn cleanup_app_data(app_handle: AppHandle) -> Result<String, String> {
    use std::fs;

    let paths = get_app_data_paths();
    let mut cleaned = Vec::new();
    let mut errors = Vec::new();

    let _ = app_handle.emit("cleanup-progress", serde_json::json!({
        "status": "starting",
        "message": "Starting cleanup..."
    }));

    for (name, path) in paths {
        if !path.exists() {
            continue;
        }

        let _ = app_handle.emit("cleanup-progress", serde_json::json!({
            "status": "cleaning",
            "message": format!("Removing {}...", name)
        }));

        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };

        match result {
            Ok(_) => {
                crate::dev_log!("[CLEANUP] Removed: {}", path.display());
                cleaned.push(name);
            }
            Err(e) => {
                crate::dev_log!("[CLEANUP] Failed to remove {}: {}", path.display(), e);
                errors.push(format!("{}: {}", name, e));
            }
        }
    }

    let _ = app_handle.emit("cleanup-progress", serde_json::json!({
        "status": "complete",
        "message": "Cleanup complete"
    }));

    if errors.is_empty() {
        Ok(format!(
            "Successfully cleaned {} items:\n- {}",
            cleaned.len(),
            cleaned.join("\n- ")
        ))
    } else {
        Err(format!(
            "Cleaned {} items but {} errors:\n{}",
            cleaned.len(),
            errors.len(),
            errors.join("\n")
        ))
    }
}
