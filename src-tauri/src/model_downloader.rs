use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub size_mb: u64,
    pub url: String,
    pub installed: bool,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub percent: f64,
    pub status: String,
}

pub fn get_available_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "canary-1b-flash".to_string(),
            size_mb: 700,
            url: "nvidia/canary-1b-flash".to_string(),
            installed: false,
        },
        ModelInfo {
            name: "canary-1b".to_string(),
            size_mb: 3500,
            url: "nvidia/canary-1b".to_string(),
            installed: false,
        },
    ]
}

pub fn check_installed_models() -> Result<Vec<ModelInfo>, String> {
    let mut models = get_available_models();

    // Check HuggingFace cache for installed models
    if let Some(cache_dir) = dirs::cache_dir() {
        let hf_cache = cache_dir.join("huggingface").join("hub");

        for model in &mut models {
            let model_cache_name = format!("models--nvidia--{}", model.name);
            let model_path = hf_cache.join(&model_cache_name);

            // Check if model directory exists and has snapshots
            if model_path.exists() {
                let snapshots = model_path.join("snapshots");
                if snapshots.exists() && snapshots.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&snapshots) {
                        if entries.count() > 0 {
                            model.installed = true;
                        }
                    }
                }
            }
        }
    }

    Ok(models)
}

pub async fn download_model(app_handle: AppHandle, model_name: String) -> Result<(), String> {
    let models = get_available_models();
    let model_info = models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Model '{}' not found", model_name))?;

    println!(
        "[MODEL] 📥 Starting download of {} ({} MB)...",
        model_name, model_info.size_mb
    );

    // Emit initial progress
    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model: model_name.clone(),
            downloaded_mb: 0.0,
            total_mb: model_info.size_mb as f64,
            percent: 0.0,
            status: "starting".to_string(),
        },
    );

    // Get Python path (venv or system)
    let venv_python = crate::get_venv_python();
    let python_cmd = if venv_python.exists() {
        venv_python
    } else {
        std::path::PathBuf::from("python3")
    };

    // Find Python script
    let python_script = if cfg!(debug_assertions) {
        let cwd = std::env::current_dir().unwrap();
        let path1 = cwd
            .join("src-tauri")
            .join("python")
            .join("canary_service.py");
        let path2 = cwd.join("python").join("canary_service.py");
        let path3 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("python")
            .join("canary_service.py");

        if path1.exists() {
            path1
        } else if path2.exists() {
            path2
        } else {
            path3
        }
    } else {
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("python")
            .join("canary_service.py")
    };

    // Spawn Python process
    let mut child = Command::new(&python_cmd)
        .arg(&python_script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to start Python: {}", e))?;

    let mut stdin = child.stdin.take().ok_or("Failed to get stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
    let reader = BufReader::new(stdout);

    // Wait for ready signal
    let mut lines = reader.lines();
    if let Some(Ok(line)) = lines.next() {
        let response: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| format!("Invalid ready signal: {}", e))?;
        if !response
            .get("ready")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err("Service not ready".to_string());
        }
    }

    // Send download command
    let cmd = serde_json::json!({
        "command": "download_model",
        "model_name": model_name
    });
    writeln!(stdin, "{}", cmd).map_err(|e| format!("Failed to send command: {}", e))?;
    stdin
        .flush()
        .map_err(|e| format!("Failed to flush: {}", e))?;

    // Process responses (progress updates and final result)
    let mut final_result: Option<serde_json::Value> = None;

    for line in lines {
        let line = line.map_err(|e| format!("Read error: {}", e))?;
        if line.is_empty() {
            continue;
        }

        let response: serde_json::Value = serde_json::from_str(&line)
            .map_err(|e| format!("Invalid JSON: {} - line: {}", e, line))?;

        if let Some(msg_type) = response.get("type").and_then(|v| v.as_str()) {
            if msg_type == "download_progress" {
                let percent = response
                    .get("percent")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let downloaded = response
                    .get("downloaded_bytes")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    / (1024.0 * 1024.0);
                let total = response
                    .get("total_bytes")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
                    / (1024.0 * 1024.0);

                let _ = app_handle.emit(
                    "model-download-progress",
                    DownloadProgress {
                        model: model_name.clone(),
                        downloaded_mb: downloaded,
                        total_mb: if total > 0.0 {
                            total
                        } else {
                            model_info.size_mb as f64
                        },
                        percent,
                        status: "downloading".to_string(),
                    },
                );
            }
        } else if response.get("success").is_some() {
            final_result = Some(response);
            break;
        }
    }

    // Shutdown service
    let shutdown_cmd = serde_json::json!({"command": "shutdown"});
    let _ = writeln!(stdin, "{}", shutdown_cmd);
    let _ = child.wait();

    // Check result
    match final_result {
        Some(result) => {
            if result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                println!("[MODEL] ✅ Downloaded {} successfully", model_name);
                let _ = app_handle.emit(
                    "model-download-progress",
                    DownloadProgress {
                        model: model_name.clone(),
                        downloaded_mb: model_info.size_mb as f64,
                        total_mb: model_info.size_mb as f64,
                        percent: 100.0,
                        status: "complete".to_string(),
                    },
                );
                let _ = app_handle.emit("model-download-complete", model_name);
                Ok(())
            } else {
                let error = result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                Err(format!("Download failed: {}", error))
            }
        }
        None => Err("No response from download service".to_string()),
    }
}
