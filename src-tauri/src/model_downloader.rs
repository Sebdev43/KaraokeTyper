//! Multi-backend model registry + downloader.
//!
//! Models are served by transcribe-rs (Whisper GGML + optional ONNX engines).
//! The registry is engine-aware; already-downloaded files in
//! `~/.local/share/karaoketyper/models/` are preserved across upgrades via
//! `is_model_installed()` which only checks on-disk size.

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

use crate::stt::EngineType;

/// Base URL for Whisper GGML models on HuggingFace.
const GGML_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Clone, Serialize, Deserialize, Type)]
pub struct ModelInfo {
    /// Stable identifier used as config.model and filename stem.
    pub name: String,
    pub display_name: String,
    pub size_mb: u32,
    /// Remote URL. Empty for engines without a downloadable artifact (e.g. cloud APIs).
    pub url: String,
    pub installed: bool,
    pub description: String,
    /// Engine that consumes this model.
    #[serde(default)]
    pub engine_type: EngineType,
    /// True for ONNX engines that expect a directory of files rather than a single blob.
    #[serde(default)]
    pub is_directory: bool,
    /// Languages this model can transcribe — empty means "universal/auto".
    #[serde(default)]
    pub supported_languages: Vec<String>,
    /// Hint for the UI — the headline recommendation for new users.
    #[serde(default)]
    pub is_recommended: bool,
    /// Indicative scores (0.0 — 1.0) for sorting in the dashboard.
    #[serde(default)]
    pub accuracy_score: f32,
    #[serde(default)]
    pub speed_score: f32,
    /// True when the backend is not yet executable (transcribe-rs feature disabled).
    /// UI can grey out the entry and show a "Bientôt" badge.
    #[serde(default)]
    pub coming_soon: bool,
}

#[derive(Clone, Serialize, Type)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub percent: f64,
    pub status: String,
}

fn whisper_languages() -> Vec<String> {
    // 99-language set trimmed to the codes we actually expose in the UI.
    [
        "en", "fr", "es", "de", "it", "pt", "nl", "pl", "ru", "ar", "zh", "ja", "ko",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

fn get_whisper_models() -> Vec<ModelInfo> {
    let langs = whisper_languages();
    vec![
        ModelInfo {
            name: "ggml-tiny".to_string(),
            display_name: "Whisper Tiny".to_string(),
            size_mb: 75,
            url: format!("{}/ggml-tiny.bin", GGML_BASE_URL),
            installed: false,
            description: "75 MB • 99 langues • Rapide mais basique".to_string(),
            engine_type: EngineType::Whisper,
            is_directory: false,
            supported_languages: langs.clone(),
            is_recommended: false,
            accuracy_score: 0.30,
            speed_score: 0.95,
            coming_soon: false,
        },
        ModelInfo {
            name: "ggml-base".to_string(),
            display_name: "Whisper Base".to_string(),
            size_mb: 142,
            url: format!("{}/ggml-base.bin", GGML_BASE_URL),
            installed: false,
            description: "142 MB • 99 langues • Bon compromis vitesse/qualité".to_string(),
            engine_type: EngineType::Whisper,
            is_directory: false,
            supported_languages: langs.clone(),
            is_recommended: false,
            accuracy_score: 0.45,
            speed_score: 0.90,
            coming_soon: false,
        },
        ModelInfo {
            name: "ggml-small".to_string(),
            display_name: "Whisper Small ⭐ Recommandé".to_string(),
            size_mb: 466,
            url: format!("{}/ggml-small.bin", GGML_BASE_URL),
            installed: false,
            description: "466 MB • 99 langues • Meilleure précision pour le français".to_string(),
            engine_type: EngineType::Whisper,
            is_directory: false,
            supported_languages: langs.clone(),
            is_recommended: true,
            accuracy_score: 0.65,
            speed_score: 0.80,
            coming_soon: false,
        },
        ModelInfo {
            name: "ggml-medium".to_string(),
            display_name: "Whisper Medium".to_string(),
            size_mb: 1500,
            url: format!("{}/ggml-medium.bin", GGML_BASE_URL),
            installed: false,
            description: "1.5 GB • 99 langues • Excellente qualité, plus lent".to_string(),
            engine_type: EngineType::Whisper,
            is_directory: false,
            supported_languages: langs.clone(),
            is_recommended: false,
            accuracy_score: 0.80,
            speed_score: 0.55,
            coming_soon: false,
        },
        ModelInfo {
            name: "ggml-large-v3".to_string(),
            display_name: "Whisper Large-v3".to_string(),
            size_mb: 3100,
            url: format!("{}/ggml-large-v3.bin", GGML_BASE_URL),
            installed: false,
            description: "3.1 GB • 99 langues • Précision maximale, très lent".to_string(),
            engine_type: EngineType::Whisper,
            is_directory: false,
            supported_languages: langs,
            is_recommended: false,
            accuracy_score: 0.92,
            speed_score: 0.30,
            coming_soon: false,
        },
    ]
}

/// ONNX engines not yet wired into transcribe-rs feature set — listed to
/// advertise them in the dashboard. Downloads + runtime are unlocked in
/// a later lot.
fn get_onnx_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "sense-voice-int8".to_string(),
            display_name: "SenseVoice (multilingue int8)".to_string(),
            size_mb: 234,
            url: String::new(),
            installed: false,
            description: "234 MB • FR/EN/ES/… • Rapide, bonne qualité français".to_string(),
            engine_type: EngineType::SenseVoice,
            is_directory: true,
            supported_languages: vec!["fr".into(), "en".into(), "es".into(), "de".into()],
            is_recommended: false,
            accuracy_score: 0.72,
            speed_score: 0.88,
            coming_soon: true,
        },
        ModelInfo {
            name: "moonshine-base".to_string(),
            display_name: "Moonshine Base (EN)".to_string(),
            size_mb: 80,
            url: String::new(),
            installed: false,
            description: "80 MB • Anglais • Ultra léger, streaming".to_string(),
            engine_type: EngineType::Moonshine,
            is_directory: true,
            supported_languages: vec!["en".into()],
            is_recommended: false,
            accuracy_score: 0.60,
            speed_score: 0.95,
            coming_soon: true,
        },
    ]
}

pub fn get_models_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("karaoketyper")
        .join("models")
}

pub fn get_model_file_path(model_name: &str) -> PathBuf {
    get_models_dir().join(format!("{}.bin", model_name))
}

pub fn get_available_models() -> Vec<ModelInfo> {
    let mut all = get_whisper_models();
    all.extend(get_onnx_models());
    all
}

/// File-based detection — matches legacy behavior so previously downloaded
/// Whisper GGMLs (`ggml-small.bin`, `ggml-large-v3.bin`, …) are picked up
/// automatically without re-downloading.
fn is_model_installed(model: &ModelInfo) -> bool {
    if model.is_directory {
        let dir = get_models_dir().join(&model.name);
        return dir.is_dir();
    }
    let model_path = get_model_file_path(&model.name);
    if model_path.exists() {
        if let Ok(metadata) = std::fs::metadata(&model_path) {
            let expected_bytes = model.size_mb as u64 * 1024 * 1024;
            let actual_bytes = metadata.len();
            return actual_bytes >= expected_bytes / 2;
        }
    }
    false
}

pub fn check_installed_models() -> Result<Vec<ModelInfo>, String> {
    let mut models = get_available_models();
    for model in &mut models {
        model.installed = is_model_installed(model);
    }
    Ok(models)
}

pub fn get_model_path(model_name: &str) -> Option<String> {
    let models = get_available_models();
    let model = models.iter().find(|m| m.name == model_name)?;
    if !is_model_installed(model) {
        return None;
    }
    let path = if model.is_directory {
        get_models_dir().join(&model.name)
    } else {
        get_model_file_path(model_name)
    };
    Some(path.to_string_lossy().to_string())
}

pub async fn download_model(app_handle: AppHandle, model_name: String) -> Result<(), String> {
    let models = get_available_models();
    let model_info = models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Model '{}' not found", model_name))?;

    if model_info.coming_soon {
        return Err(format!(
            "Backend {:?} not yet available — feature will land in a future update",
            model_info.engine_type
        ));
    }
    if model_info.is_directory {
        return Err(
            "Directory-based models (ONNX) require archive extraction, which is not yet \
             implemented. This will be added in a later lot."
                .to_string(),
        );
    }
    if model_info.url.is_empty() {
        return Err(format!("Model '{}' has no downloadable URL", model_name));
    }

    crate::dev_log!(
        "[MODEL] 📥 Starting download of {} ({} MB)...",
        model_name,
        model_info.size_mb
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

    let models_dir = get_models_dir();
    tokio::fs::create_dir_all(&models_dir)
        .await
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let model_path = get_model_file_path(&model_name);
    let temp_path = model_path.with_extension("bin.tmp");

    let client = reqwest::Client::builder()
        .user_agent("KaraokeTyper/1.0")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

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

    let total_size = response
        .content_length()
        .unwrap_or(model_info.size_mb as u64 * 1024 * 1024);

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

        let percent = (downloaded as f64 / total_size as f64 * 100.0).min(100.0);
        let current_percent = percent as i32;

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

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {}", e))?;
    drop(file);

    tokio::fs::rename(&temp_path, &model_path)
        .await
        .map_err(|e| format!("Failed to rename temp file: {}", e))?;

    crate::dev_log!(
        "[MODEL] ✅ Downloaded {} successfully to {}",
        model_name,
        model_path.display()
    );

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
    fn test_available_models_has_every_local_engine() {
        let models = get_available_models();
        assert!(models
            .iter()
            .any(|m| m.name == "ggml-small" && m.engine_type == EngineType::Whisper));
        assert!(models
            .iter()
            .any(|m| m.engine_type == EngineType::SenseVoice));
        assert!(models
            .iter()
            .any(|m| m.engine_type == EngineType::Moonshine));
    }

    #[test]
    fn test_onnx_entries_marked_coming_soon() {
        let models = get_onnx_models();
        assert!(models.iter().all(|m| m.coming_soon));
    }
}
