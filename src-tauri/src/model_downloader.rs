//! Model downloader for whisper-rs GGML models
//!
//! Downloads GGML models from ggerganov/whisper.cpp on HuggingFace.
//! Uses native Rust (reqwest) instead of Python subprocess.

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

/// Base URL for GGML models on HuggingFace
const GGML_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Clone, Serialize, Deserialize, Type)]
pub struct ModelInfo {
    pub name: String,
    pub display_name: String,
    pub size_mb: u32,
    pub url: String,
    pub installed: bool,
    pub description: String,
}

#[derive(Clone, Serialize, Type)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub percent: f64,
    pub status: String,
}

/// Get available Whisper GGML models
fn get_whisper_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "ggml-tiny".to_string(),
            display_name: "Whisper Tiny".to_string(),
            size_mb: 75,
            url: format!("{}/ggml-tiny.bin", GGML_BASE_URL),
            installed: false,
            description: "75 MB • 99 langues • Rapide mais basique".to_string(),
        },
        ModelInfo {
            name: "ggml-base".to_string(),
            display_name: "Whisper Base".to_string(),
            size_mb: 142,
            url: format!("{}/ggml-base.bin", GGML_BASE_URL),
            installed: false,
            description: "142 MB • 99 langues • Bon compromis vitesse/qualité".to_string(),
        },
        ModelInfo {
            name: "ggml-small".to_string(),
            display_name: "Whisper Small ⭐ Recommandé".to_string(),
            size_mb: 466,
            url: format!("{}/ggml-small.bin", GGML_BASE_URL),
            installed: false,
            description: "466 MB • 99 langues • Meilleure précision pour le français".to_string(),
        },
        ModelInfo {
            name: "ggml-medium".to_string(),
            display_name: "Whisper Medium".to_string(),
            size_mb: 1500,
            url: format!("{}/ggml-medium.bin", GGML_BASE_URL),
            installed: false,
            description: "1.5 GB • 99 langues • Excellente qualité, plus lent".to_string(),
        },
        ModelInfo {
            name: "ggml-large-v3".to_string(),
            display_name: "Whisper Large-v3".to_string(),
            size_mb: 3100,
            url: format!("{}/ggml-large-v3.bin", GGML_BASE_URL),
            installed: false,
            description: "3.1 GB • 99 langues • Précision maximale, très lent".to_string(),
        },
    ]
}

/// Get the models directory path
pub fn get_models_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("karaoketyper")
        .join("models")
}

/// Get the path to a specific model file
pub fn get_model_file_path(model_name: &str) -> PathBuf {
    get_models_dir().join(format!("{}.bin", model_name))
}

/// Get all available models
pub fn get_available_models() -> Vec<ModelInfo> {
    get_whisper_models()
}

/// Check if a specific model is installed
fn is_model_installed(model: &ModelInfo) -> bool {
    let model_path = get_model_file_path(&model.name);
    if model_path.exists() {
        // Verify file size is reasonable (at least 50% of expected size)
        if let Ok(metadata) = std::fs::metadata(&model_path) {
            let expected_bytes = model.size_mb as u64 * 1024 * 1024;
            let actual_bytes = metadata.len();
            return actual_bytes >= expected_bytes / 2;
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

/// Get the path to a downloaded model (returns None if not installed)
pub fn get_model_path(model_name: &str) -> Option<String> {
    let models = get_available_models();
    let model = models.iter().find(|m| m.name == model_name)?;

    if is_model_installed(model) {
        Some(get_model_file_path(model_name).to_string_lossy().to_string())
    } else {
        None
    }
}

/// Download a model using native Rust (reqwest)
pub async fn download_model(app_handle: AppHandle, model_name: String) -> Result<(), String> {
    let models = get_available_models();
    let model_info = models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Model '{}' not found", model_name))?;

    crate::dev_log!(
        "[MODEL] 📥 Starting download of {} ({} MB)...",
        model_name,
        model_info.size_mb
    );

    // Emit starting progress
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

    // Ensure models directory exists
    let models_dir = get_models_dir();
    tokio::fs::create_dir_all(&models_dir)
        .await
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let model_path = get_model_file_path(&model_name);
    let temp_path = model_path.with_extension("bin.tmp");

    // Create HTTP client
    let client = reqwest::Client::builder()
        .user_agent("KaraokeTyper/1.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Start download
    let response = client
        .get(&model_info.url)
        .send()
        .await
        .map_err(|e| format!("Failed to start download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    // Get content length if available
    let total_size = response
        .content_length()
        .unwrap_or(model_info.size_mb as u64 * 1024 * 1024);

    // Create temp file for download
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_progress_percent: i32 = -1;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Download error: {}", e))?;

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;

        downloaded += chunk.len() as u64;

        // Calculate progress
        let percent = (downloaded as f64 / total_size as f64 * 100.0).min(100.0);
        let current_percent = percent as i32;

        // Only emit progress every 1% to avoid flooding
        if current_percent > last_progress_percent {
            last_progress_percent = current_percent;

            let _ = app_handle.emit(
                "model-download-progress",
                DownloadProgress {
                    model: model_name.clone(),
                    downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
                    total_mb: total_size as f64 / (1024.0 * 1024.0),
                    percent,
                    status: "downloading".to_string(),
                },
            );
        }
    }

    // Flush and close temp file
    file.flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    // Rename temp file to final path
    tokio::fs::rename(&temp_path, &model_path)
        .await
        .map_err(|e| format!("Failed to rename temp file: {}", e))?;

    crate::dev_log!(
        "[MODEL] ✅ Downloaded {} successfully to {}",
        model_name,
        model_path.display()
    );

    // Emit completion
    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model: model_name.clone(),
            downloaded_mb: total_size as f64 / (1024.0 * 1024.0),
            total_mb: total_size as f64 / (1024.0 * 1024.0),
            percent: 100.0,
            status: "complete".to_string(),
        },
    );

    let _ = app_handle.emit(
        "model-download-complete",
        serde_json::json!({
            "model": model_name,
            "path": model_path.to_string_lossy()
        }),
    );

    Ok(())
}

/// Delete a downloaded model
#[allow(dead_code)]
pub async fn delete_model(model_name: &str) -> Result<(), String> {
    let model_path = get_model_file_path(model_name);
    if model_path.exists() {
        tokio::fs::remove_file(&model_path)
            .await
            .map_err(|e| format!("Failed to delete model: {}", e))?;
        crate::dev_log!("[MODEL] 🗑️ Deleted model: {}", model_name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_models_dir() {
        let dir = get_models_dir();
        assert!(dir.to_string_lossy().contains("karaoketyper"));
        assert!(dir.to_string_lossy().contains("models"));
    }

    #[test]
    fn test_model_file_path() {
        let path = get_model_file_path("ggml-base");
        assert!(path.to_string_lossy().ends_with("ggml-base.bin"));
    }

    #[test]
    fn test_available_models() {
        let models = get_available_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.name == "ggml-base"));
        assert!(models.iter().any(|m| m.name == "ggml-small"));
    }
}
