use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub display_name: String,
    pub size_mb: u64,
    pub url: String,
    pub installed: bool,
    pub description: String,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub percent: f64,
    pub status: String,
}

/// Get available Parakeet models (sherpa-onnx)
fn get_parakeet_models() -> Vec<ModelInfo> {
    vec![
        // Whisper multilingual models - Support 99 languages including French
        ModelInfo {
            name: "sherpa-onnx-whisper-tiny".to_string(),
            display_name: "Whisper Tiny (Multilingual)".to_string(),
            size_mb: 75,
            url: "csukuangfj/sherpa-onnx-whisper-tiny".to_string(),
            installed: false,
            description: "75 MB • 99 langues (FR, EN, ES, DE...) • Rapide mais basique".to_string(),
        },
        ModelInfo {
            name: "sherpa-onnx-whisper-base".to_string(),
            display_name: "Whisper Base (Multilingual)".to_string(),
            size_mb: 150,
            url: "csukuangfj/sherpa-onnx-whisper-base".to_string(),
            installed: false,
            description: "150 MB • 99 langues (FR, EN, ES, DE...) • Bon compromis vitesse/qualité"
                .to_string(),
        },
        ModelInfo {
            name: "sherpa-onnx-whisper-small".to_string(),
            display_name: "Whisper Small (Multilingual) ⭐ Recommandé".to_string(),
            size_mb: 400,
            url: "csukuangfj/sherpa-onnx-whisper-small".to_string(),
            installed: false,
            description:
                "400 MB • 99 langues (FR, EN, ES, DE...) • Meilleure précision pour le français"
                    .to_string(),
        },
        ModelInfo {
            name: "sherpa-onnx-whisper-medium".to_string(),
            display_name: "Whisper Medium (Multilingual)".to_string(),
            size_mb: 1000,
            url: "csukuangfj/sherpa-onnx-whisper-medium".to_string(),
            installed: false,
            description: "1 GB • 99 langues (FR, EN, ES, DE...) • Excellente qualité, plus lent"
                .to_string(),
        },
        ModelInfo {
            name: "sherpa-onnx-whisper-large-v3".to_string(),
            display_name: "Whisper Large-v3 (Multilingual)".to_string(),
            size_mb: 3000,
            url: "csukuangfj/sherpa-onnx-whisper-large-v3".to_string(),
            installed: false,
            description:
                "3 GB • 99 langues (FR, EN, ES, DE...) • Meilleure précision maximale, très lent"
                    .to_string(),
        },
    ]
}

/// Get all available models
pub fn get_available_models() -> Vec<ModelInfo> {
    get_parakeet_models()
}

/// Get HuggingFace cache directory for a model
fn get_model_cache_path(model: &ModelInfo) -> Option<std::path::PathBuf> {
    let cache_dir = dirs::cache_dir()?;
    let hf_cache = cache_dir.join("huggingface").join("hub");

    let parts: Vec<&str> = model.url.split('/').collect();
    if parts.len() >= 2 {
        let cache_name = format!("models--{}--{}", parts[0], parts[1]);
        return Some(hf_cache.join(cache_name));
    }

    None
}

/// Check if a specific model is installed
fn is_model_installed(model: &ModelInfo) -> bool {
    if let Some(model_path) = get_model_cache_path(model) {
        let snapshots = model_path.join("snapshots");
        if snapshots.exists() && snapshots.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&snapshots) {
                return entries.count() > 0;
            }
        }
    }
    false
}

/// Check installed models and return list with installation status
pub fn check_installed_models() -> Result<Vec<ModelInfo>, String> {
    let mut models = get_available_models();

    for model in &mut models {
        model.installed = is_model_installed(model);
    }

    Ok(models)
}

/// Get the path to a downloaded model
#[allow(dead_code)]
pub fn get_model_path(model_name: &str) -> Option<String> {
    let models = get_available_models();
    let model = models.iter().find(|m| m.name == model_name)?;

    if let Some(cache_path) = get_model_cache_path(model) {
        let snapshots = cache_path.join("snapshots");
        if snapshots.exists() {
            if let Ok(mut entries) = std::fs::read_dir(&snapshots) {
                if let Some(Ok(entry)) = entries.next() {
                    return Some(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Download a model using parakeet_service.py
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

    let venv_python = crate::get_venv_python();
    let python_cmd = if venv_python.exists() {
        venv_python
    } else {
        std::path::PathBuf::from("python3")
    };

    let python_script = if cfg!(debug_assertions) {
        let cwd = std::env::current_dir().unwrap();
        let path1 = cwd
            .join("src-tauri")
            .join("python")
            .join("parakeet_service.py");
        let path2 = cwd.join("python").join("parakeet_service.py");
        let path3 = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("python")
            .join("parakeet_service.py");

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
            .join("parakeet_service.py")
    };

    if !python_script.exists() {
        return Err(format!(
            "Python service not found at: {}",
            python_script.display()
        ));
    }

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

    let mut lines = reader.lines();
    if let Some(Ok(line)) = lines.next() {
        let response: serde_json::Value =
            serde_json::from_str(&line).map_err(|e| format!("Invalid ready signal: {}", e))?;
        if !response
            .get("ready")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let error = response
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Service not ready");
            return Err(error.to_string());
        }
    }

    let cmd = serde_json::json!({
        "command": "download_model",
        "model_name": model_name
    });
    writeln!(stdin, "{}", cmd).map_err(|e| format!("Failed to send command: {}", e))?;
    stdin
        .flush()
        .map_err(|e| format!("Failed to flush: {}", e))?;

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

    let shutdown_cmd = serde_json::json!({"command": "shutdown"});
    let _ = writeln!(stdin, "{}", shutdown_cmd);
    let _ = child.wait();

    match final_result {
        Some(result) => {
            if result
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                println!("[MODEL] ✅ Downloaded {} successfully", model_name);

                let cache_dir = result
                    .get("cache_dir")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

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

                let _ = app_handle.emit(
                    "model-download-complete",
                    serde_json::json!({
                        "model": model_name,
                        "cache_dir": cache_dir
                    }),
                );

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
