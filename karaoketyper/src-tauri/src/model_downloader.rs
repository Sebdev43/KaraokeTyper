use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use serde::{Deserialize, Serialize};

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
}

pub fn get_available_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "tiny".to_string(),
            size_mb: 75,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin".to_string(),
            installed: false,
        },
        ModelInfo {
            name: "base".to_string(),
            size_mb: 142,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin".to_string(),
            installed: false,
        },
        ModelInfo {
            name: "small".to_string(),
            size_mb: 466,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin".to_string(),
            installed: false,
        },
        ModelInfo {
            name: "medium".to_string(),
            size_mb: 1520,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin".to_string(),
            installed: false,
        },
        ModelInfo {
            name: "large".to_string(),
            size_mb: 2960,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v2.bin".to_string(),
            installed: false,
        },
        ModelInfo {
            name: "large-v3".to_string(),
            size_mb: 3100,
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin".to_string(),
            installed: false,
        },
    ]
}

pub fn get_model_path(app_handle: &AppHandle, model_name: &str) -> Result<PathBuf, String> {
    let model_filename = format!("ggml-{}.bin", model_name);
    let model_path = app_handle
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource dir: {}", e))?
        .join("models")
        .join(&model_filename);

    Ok(model_path)
}

pub fn check_installed_models(app_handle: &AppHandle) -> Result<Vec<ModelInfo>, String> {
    let mut models = get_available_models();

    for model in &mut models {
        let path = get_model_path(app_handle, &model.name)?;

        // Check if file exists AND has correct size (±10% tolerance for compression)
        model.installed = if path.exists() {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let file_size_mb = metadata.len() / (1024 * 1024);
                let expected_size = model.size_mb;
                let tolerance = (expected_size as f64 * 0.10) as u64; // 10% tolerance

                // File must be within ±10% of expected size
                file_size_mb >= expected_size.saturating_sub(tolerance)
                    && file_size_mb <= expected_size + tolerance
            } else {
                false
            }
        } else {
            false
        };
    }

    Ok(models)
}

pub async fn download_model(
    app_handle: AppHandle,
    model_name: String,
) -> Result<(), String> {
    let models = get_available_models();
    let model_info = models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Model '{}' not found", model_name))?;

    let model_path = get_model_path(&app_handle, &model_name)?;

    // Create models directory if it doesn't exist
    if let Some(parent) = model_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create models directory: {}", e))?;
    }

    println!("[MODEL] 📥 Downloading {} ({} MB)...", model_name, model_info.size_mb);

    // Download with reqwest and report progress
    let client = reqwest::Client::new();
    let response = client
        .get(&model_info.url)
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    use futures::StreamExt;
    use std::io::Write;

    let mut file = std::fs::File::create(&model_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error: {}", e))?;
        file.write_all(&chunk)
            .map_err(|e| format!("Failed to write to file: {}", e))?;

        downloaded += chunk.len() as u64;

        // Emit progress every 5 MB
        if downloaded % (5 * 1024 * 1024) == 0 || downloaded == total_size {
            let progress = DownloadProgress {
                model: model_name.clone(),
                downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
                total_mb: total_size as f64 / (1024.0 * 1024.0),
                percent: if total_size > 0 {
                    (downloaded as f64 / total_size as f64) * 100.0
                } else {
                    0.0
                },
            };

            let _ = app_handle.emit("model-download-progress", progress);
        }
    }

    println!("[MODEL] ✅ Downloaded {} successfully", model_name);

    // Emit completion
    let _ = app_handle.emit("model-download-complete", model_name.clone());

    Ok(())
}
