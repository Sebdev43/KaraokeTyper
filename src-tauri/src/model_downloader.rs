//! Multi-backend model registry + downloader.
//!
//! Models are served by transcribe-rs (Whisper GGML + optional ONNX engines).
//! The registry is engine-aware; already-downloaded files in
//! `~/.local/share/karaoketyper/models/` are preserved across upgrades via
//! `is_model_installed()` which only checks on-disk size.

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

/// Max time to establish the TCP/TLS connection to a model mirror.
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Max time to wait for the *next* chunk of a download before treating the
/// connection as stalled. We deliberately do NOT set a global request timeout
/// (`reqwest::ClientBuilder::timeout`) because that would abort legitimate
/// multi-GB downloads on slow links; a per-chunk stall watchdog catches a wedged
/// connection without penalizing large-but-progressing transfers.
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the shared HTTP client for model downloads: a connect timeout plus a
/// versioned user-agent. Per-chunk stall protection is applied at the call site.
fn build_download_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!("KaraokeTyper/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))
}

use crate::stt::EngineType;

/// Global guard against concurrent downloads of the same model. Parallel calls
/// to `download_model(name)` would otherwise share the same staging path and
/// corrupt each other's extraction.
static IN_PROGRESS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn claim_download_slot(name: &str) -> Result<(), String> {
    let mut guard = IN_PROGRESS
        .lock()
        .map_err(|e| format!("IN_PROGRESS lock poisoned: {}", e))?;
    let set = guard.get_or_insert_with(HashSet::new);
    if !set.insert(name.to_string()) {
        return Err(format!(
            "Téléchargement déjà en cours pour '{}' — patientez.",
            name
        ));
    }
    Ok(())
}

fn release_download_slot(name: &str) {
    if let Ok(mut guard) = IN_PROGRESS.lock() {
        if let Some(set) = guard.as_mut() {
            set.remove(name);
        }
    }
}

/// RAII guard releasing the in-progress download slot on drop (including on
/// early `?`/return). Used by all download paths.
struct DownloadSlotGuard<'a>(&'a str);
impl Drop for DownloadSlotGuard<'_> {
    fn drop(&mut self) {
        release_download_slot(self.0);
    }
}

/// Read the next chunk from a download byte-stream, bounding the wait by
/// [`DOWNLOAD_STALL_TIMEOUT`]. Returns `Ok(None)` at end of stream, `Ok(Some)`
/// for a chunk, and `Err` on a read error or stall. Partial-file cleanup is left
/// to the caller because it differs per path (temp file vs staging dir).
async fn next_chunk_or_stall<S, B, E>(stream: &mut S, label: &str) -> Result<Option<B>, String>
where
    S: futures::Stream<Item = Result<B, E>> + Unpin,
    E: std::fmt::Display,
{
    match tokio::time::timeout(DOWNLOAD_STALL_TIMEOUT, stream.next()).await {
        Ok(Some(Ok(chunk))) => Ok(Some(chunk)),
        Ok(Some(Err(e))) => Err(format!("Download error on {}: {}", label, e)),
        Ok(None) => Ok(None),
        Err(_) => Err(format!(
            "Download stalled on {}: no data for {}s",
            label,
            DOWNLOAD_STALL_TIMEOUT.as_secs()
        )),
    }
}

/// Base URL for Whisper GGML models on HuggingFace.
const GGML_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// One file that ships as part of a multi-file model bundle (e.g. Parakeet V3).
/// Hash is mandatory — the download is rejected if the content doesn't match.
#[derive(Clone, Serialize, Deserialize, Type)]
pub struct ModelFile {
    /// Filename inside the model directory (e.g. `encoder-model.int8.onnx`).
    pub filename: String,
    /// Absolute URL to fetch.
    pub url: String,
    /// Expected SHA-256 of the file contents, lowercase hex.
    pub sha256: String,
}

#[derive(Clone, Serialize, Deserialize, Type)]
pub struct ModelInfo {
    /// Stable identifier used as config.model and filename stem.
    pub name: String,
    pub display_name: String,
    pub size_mb: u32,
    /// Remote URL for a single-file model (Whisper .bin). Empty for multi-file
    /// bundles — those use `files` instead.
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
    /// Individual files for multi-file bundles (Parakeet V3). Each entry is
    /// fetched separately and verified against its SHA-256. If non-empty,
    /// `url` is ignored. The directory `{models_dir}/{name}/` is populated.
    #[serde(default)]
    pub files: Vec<ModelFile>,
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
            files: vec![],
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
            files: vec![],
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
            files: vec![],
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
            files: vec![],
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
            files: vec![],
        },
    ]
}

/// Base HuggingFace URL for the Parakeet V3 int8 ONNX bundle.
///
/// Pinned to commit `8f23f0c0…` so the SHA-256 table below remains accurate
/// even if the repo owner force-pushes later. HF serves files at
/// `/resolve/<commit>/<filename>` which locks the content.
const PARAKEET_V3_BASE: &str =
    "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce";

/// Files that compose the Parakeet V3 model directory. Each file is
/// downloaded, checksummed, and written to `{models_dir}/{name}/{filename}`.
/// If any SHA-256 mismatches, the whole install is aborted and rolled back.
fn parakeet_v3_files() -> Vec<ModelFile> {
    let f = |name: &str, sha: &str| ModelFile {
        filename: name.to_string(),
        url: format!("{}/{}", PARAKEET_V3_BASE, name),
        sha256: sha.to_string(),
    };
    vec![
        f(
            "config.json",
            "666903c76b9798caf2c210afd4f6cd60b08a8dbf9800ec8d7a3bc0d2148ac466",
        ),
        f(
            "vocab.txt",
            "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        ),
        f(
            "encoder-model.int8.onnx",
            "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
        ),
        // encoder-model.onnx.data (2.3 GB fp32 weights) is deliberately NOT
        // downloaded — it's the external weights blob referenced by
        // encoder-model.onnx (fp32). transcribe-rs loads encoder-model.int8.onnx
        // (weights inline in the int8 file), so the fp32 data is dead weight.
        f(
            "decoder_joint-model.int8.onnx",
            "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
        ),
        f(
            "nemo128.onnx",
            "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
        ),
    ]
}

/// Languages supported by Parakeet V3 — mirror the enum in `stt::types`.
fn parakeet_v3_languages() -> Vec<String> {
    [
        "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv",
        "mt", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// ONNX engines that ship as a `.tar.gz` archive extracted into a directory.
fn get_onnx_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "parakeet-tdt-0.6b-v3-int8".to_string(),
            display_name: "Parakeet V3 (FR + 24 langues UE) ⭐ Recommandé FR".to_string(),
            // 640 MB = 622 (encoder int8) + 17 (decoder int8) + <1 (tiny files).
            // The 2.3 GB encoder-model.onnx.data (fp32) is intentionally excluded.
            size_mb: 640,
            url: String::new(),
            installed: false,
            description: "640 MB • 25 langues UE dont français • CPU, ~3-5× temps réel".to_string(),
            engine_type: EngineType::Parakeet,
            is_directory: true,
            supported_languages: parakeet_v3_languages(),
            is_recommended: true,
            accuracy_score: 0.82,
            speed_score: 0.75,
            coming_soon: false,
            files: parakeet_v3_files(),
        },
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
            files: vec![],
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
            files: vec![],
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
        if model_info.files.is_empty() {
            return Err(format!(
                "Model '{}' is directory-based but has no file manifest",
                model_name
            ));
        }
        return download_and_verify_files(app_handle, model_info.clone()).await;
    }
    if model_info.url.is_empty() {
        return Err(format!("Model '{}' has no downloadable URL", model_name));
    }

    // Reject a concurrent download of the same model (they'd share the same
    // .bin.tmp path and corrupt each other). Released on drop.
    claim_download_slot(&model_name)?;
    let _slot = DownloadSlotGuard(&model_name);

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

    let client = build_download_client()?;

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
    // Hard cap: never accept more than 1.5x the declared size, mirroring the
    // multi-file/archive paths — guards against a server streaming gigabytes to
    // fill the disk.
    let max_bytes: u64 = (model_info.size_mb as u64)
        .saturating_mul(1024 * 1024)
        .saturating_mul(3)
        / 2;

    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_progress_percent: i32 = -1;

    loop {
        let chunk = match next_chunk_or_stall(&mut stream, &model_name).await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(e);
            }
        };

        downloaded += chunk.len() as u64;
        if downloaded > max_bytes {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(format!(
                "Download exceeded expected size ({} MB cap) — aborting.",
                max_bytes / (1024 * 1024)
            ));
        }

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;

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

/// Download a `.tar.gz` archive and extract it into `get_models_dir()/<name>/`.
/// Used for directory-based models (Parakeet, SenseVoice, Moonshine).
///
/// Security: the tarball is decompressed into a **temp staging dir** first, we
/// verify every entry's path stays inside the staging dir (no `..` escape, no
/// absolute paths), then atomically rename to the final location. This blocks
/// the classic "tar slip" zip-bomb / path-traversal attacks.
/// Download each file of a multi-file model bundle in sequence, verify its
/// SHA-256 against the hardcoded manifest, and atomically move the whole
/// directory into place. Any mismatch aborts the install and rolls back.
async fn download_and_verify_files(
    app_handle: AppHandle,
    model_info: ModelInfo,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};

    let model_name = model_info.name.clone();

    claim_download_slot(&model_name)?;
    let _slot = DownloadSlotGuard(&model_name);

    let models_dir = get_models_dir();
    tokio::fs::create_dir_all(&models_dir)
        .await
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let final_dir = models_dir.join(&model_name);
    let staging_dir = models_dir.join(format!(".{}.staging", model_name));

    if staging_dir.exists() {
        tokio::fs::remove_dir_all(&staging_dir)
            .await
            .map_err(|e| format!("Failed to clean staging dir: {}", e))?;
    }
    tokio::fs::create_dir_all(&staging_dir)
        .await
        .map_err(|e| format!("Failed to create staging dir: {}", e))?;

    let client = build_download_client()?;

    // Pre-compute an over-estimate total byte count for progress reporting.
    // Real sizes come from Content-Length per file.
    let total_expected_bytes: u64 = (model_info.size_mb as u64).saturating_mul(1024 * 1024);
    let max_total_bytes: u64 = total_expected_bytes.saturating_mul(3) / 2;
    let mut total_downloaded: u64 = 0;
    let mut last_pct: i32 = -1;

    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model: model_name.clone(),
            downloaded_mb: 0.0,
            total_mb: total_expected_bytes as f64 / (1024.0 * 1024.0),
            percent: 0.0,
            status: "starting".to_string(),
        },
    );

    for (idx, mf) in model_info.files.iter().enumerate() {
        // Reject any filename that would escape the staging dir — these come
        // from our hardcoded manifest but we defend in depth against future
        // deserialization mistakes.
        if mf.filename.is_empty()
            || mf.filename.contains("..")
            || mf.filename.contains('\0')
            || std::path::Path::new(&mf.filename).is_absolute()
        {
            return Err(format!("Unsafe filename in manifest: {}", mf.filename));
        }
        if mf.sha256.len() != 64 || !mf.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "Invalid SHA-256 in manifest for {}: {}",
                mf.filename, mf.sha256
            ));
        }

        let target = staging_dir.join(&mf.filename);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create dir {}: {}", parent.display(), e))?;
        }

        crate::dev_log!(
            "[MODEL] 📥 ({}/{}) Fetching {} …",
            idx + 1,
            model_info.files.len(),
            mf.filename
        );

        let response = client
            .get(&mf.url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch {}: {}", mf.filename, e))?
            .error_for_status()
            .map_err(|e| format!("HTTP error on {}: {}", mf.filename, e))?;

        let mut file = tokio::fs::File::create(&target)
            .await
            .map_err(|e| format!("Failed to create {}: {}", target.display(), e))?;
        let mut hasher = Sha256::new();
        let mut stream = response.bytes_stream();
        loop {
            let chunk = match next_chunk_or_stall(&mut stream, &mf.filename).await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => {
                    let _ = tokio::fs::remove_dir_all(&staging_dir).await;
                    return Err(e);
                }
            };
            total_downloaded += chunk.len() as u64;
            if total_downloaded > max_total_bytes {
                let _ = tokio::fs::remove_dir_all(&staging_dir).await;
                return Err(format!(
                    "Download exceeded expected size ({} MB cap) — aborting.",
                    max_total_bytes / (1024 * 1024)
                ));
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Write error on {}: {}", mf.filename, e))?;

            let percent =
                (total_downloaded as f64 / total_expected_bytes as f64 * 100.0).min(100.0);
            if percent as i32 > last_pct {
                last_pct = percent as i32;
                let _ = app_handle.emit(
                    "model-download-progress",
                    DownloadProgress {
                        model: model_name.clone(),
                        downloaded_mb: total_downloaded as f64 / (1024.0 * 1024.0),
                        total_mb: total_expected_bytes as f64 / (1024.0 * 1024.0),
                        percent,
                        status: format!("downloading {}", mf.filename),
                    },
                );
            }
        }
        file.flush()
            .await
            .map_err(|e| format!("Flush error on {}: {}", mf.filename, e))?;
        drop(file);

        let actual_sha256 = format!("{:x}", hasher.finalize());
        if !actual_sha256.eq_ignore_ascii_case(&mf.sha256) {
            return Err(format!(
                "Intégrité: SHA-256 de {} ne correspond pas. Attendu {}, obtenu {}. Installation refusée.",
                mf.filename, mf.sha256, actual_sha256
            ));
        }
        crate::dev_log!("[MODEL] ✔ SHA-256 OK for {}", mf.filename);
    }

    // Atomic replace with rollback-safe backup.
    let backup_dir = final_dir.with_extension("old");
    if final_dir.exists() {
        if backup_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&backup_dir).await;
        }
        tokio::fs::rename(&final_dir, &backup_dir)
            .await
            .map_err(|e| format!("Failed to set aside existing model: {}", e))?;
    }
    if let Err(e) = tokio::fs::rename(&staging_dir, &final_dir).await {
        if backup_dir.exists() {
            let _ = tokio::fs::rename(&backup_dir, &final_dir).await;
        }
        return Err(format!("Failed to finalize model dir: {}", e));
    }
    if backup_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&backup_dir).await;
    }

    crate::dev_log!(
        "[MODEL] ✅ {} installed at {} (all files verified)",
        model_name,
        final_dir.display()
    );

    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model: model_name.clone(),
            downloaded_mb: total_expected_bytes as f64 / (1024.0 * 1024.0),
            total_mb: total_expected_bytes as f64 / (1024.0 * 1024.0),
            percent: 100.0,
            status: "complete".to_string(),
        },
    );
    let _ = app_handle.emit(
        "model-download-complete",
        serde_json::json!({
            "model": model_name,
            "path": final_dir.to_string_lossy()
        }),
    );

    Ok(())
}

#[allow(dead_code)]
async fn download_and_extract_archive(
    app_handle: AppHandle,
    model_info: ModelInfo,
) -> Result<(), String> {
    let model_name = model_info.name.clone();

    // Serialize concurrent downloads of the same model. Released on all exit
    // paths via the Drop guard below.
    claim_download_slot(&model_name)?;
    let _slot = DownloadSlotGuard(&model_name);

    let models_dir = get_models_dir();
    tokio::fs::create_dir_all(&models_dir)
        .await
        .map_err(|e| format!("Failed to create models directory: {}", e))?;

    let archive_path = models_dir.join(format!("{}.tar.gz", model_name));
    let final_dir = models_dir.join(&model_name);
    let staging_dir = models_dir.join(format!(".{}.staging", model_name));

    // Clean any leftover staging dir from an interrupted previous run.
    if staging_dir.exists() {
        tokio::fs::remove_dir_all(&staging_dir)
            .await
            .map_err(|e| format!("Failed to clean staging dir: {}", e))?;
    }

    // ── 1. Download ──
    crate::dev_log!(
        "[MODEL] 📥 Starting archive download {} ({} MB)...",
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

    let client = build_download_client()?;

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
    // Hard cap: never accept more than 1.5x the declared size. Guards against
    // a malicious or misconfigured server streaming gigabytes to fill the disk.
    let max_bytes: u64 = (model_info.size_mb as u64)
        .saturating_mul(1024 * 1024)
        .saturating_mul(3)
        / 2;

    let mut file = tokio::fs::File::create(&archive_path)
        .await
        .map_err(|e| format!("Failed to create archive file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let mut last_pct: i32 = -1;
    loop {
        let chunk = match next_chunk_or_stall(&mut stream, &model_name).await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                let _ = tokio::fs::remove_file(&archive_path).await;
                return Err(e);
            }
        };
        downloaded += chunk.len() as u64;
        if downloaded > max_bytes {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return Err(format!(
                "Download exceeded expected size ({} MB > {} MB cap) — aborting.",
                downloaded / (1024 * 1024),
                max_bytes / (1024 * 1024),
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        let percent = (downloaded as f64 / total_size as f64 * 100.0).min(100.0);
        if percent as i32 > last_pct {
            last_pct = percent as i32;
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

    // ── 2. Extract with path-traversal guard ──
    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model: model_name.clone(),
            downloaded_mb: total_size as f64 / (1024.0 * 1024.0),
            total_mb: total_size as f64 / (1024.0 * 1024.0),
            percent: 100.0,
            status: "extracting".to_string(),
        },
    );

    let archive_path_clone = archive_path.clone();
    let staging_dir_clone = staging_dir.clone();
    let extract_result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        use std::fs::File;
        use tar::EntryType;

        let f = File::open(&archive_path_clone)
            .map_err(|e| format!("Failed to open archive: {}", e))?;
        let decoder = flate2::read::GzDecoder::new(f);
        let mut archive = tar::Archive::new(decoder);
        archive.set_preserve_permissions(false);
        archive.set_overwrite(true);

        std::fs::create_dir_all(&staging_dir_clone)
            .map_err(|e| format!("Failed to create staging dir: {}", e))?;
        let staging_canonical = staging_dir_clone
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize staging dir: {}", e))?;

        for entry_result in archive.entries().map_err(|e| format!("tar: {}", e))? {
            let mut entry = entry_result.map_err(|e| format!("tar entry: {}", e))?;

            // Only regular files and directories survive. Symlinks/hardlinks
            // are the canonical tar-slip escape hatch (attacker extracts a
            // symlink `a -> /etc` then a file `a/passwd`) so we ban them
            // outright — transcribe-rs model archives don't need them.
            match entry.header().entry_type() {
                EntryType::Regular | EntryType::Directory => {}
                other => {
                    return Err(format!(
                        "Refusing non-file tar entry ({:?}): {}",
                        other,
                        entry
                            .path()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    ));
                }
            }

            let rel_path = entry
                .path()
                .map_err(|e| format!("tar path: {}", e))?
                .into_owned();
            if rel_path.is_absolute()
                || rel_path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!(
                    "Refusing unsafe tar entry path: {}",
                    rel_path.display()
                ));
            }

            let target = staging_canonical.join(&rel_path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
            }
            entry
                .unpack(&target)
                .map_err(|e| format!("Failed to extract entry: {}", e))?;

            // Post-unpack: canonicalize the real path and make sure we didn't
            // somehow end up outside the staging root (defense in depth).
            if target.exists() {
                let resolved = target
                    .canonicalize()
                    .map_err(|e| format!("Canonicalize post-unpack failed: {}", e))?;
                if !resolved.starts_with(&staging_canonical) {
                    return Err(format!(
                        "Tar entry escaped staging root: {}",
                        resolved.display()
                    ));
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Extraction task failed: {}", e))?;
    extract_result?;

    // Some archives wrap a single top-level folder (`parakeet-tdt-0.6b-v3-int8/…`).
    // Detect that case and move the inner folder to the final location so we
    // don't end up with `models/parakeet-tdt-0.6b-v3-int8/parakeet-tdt-0.6b-v3-int8/`.
    let mut staging_entries: Vec<std::path::PathBuf> = Vec::new();
    let mut rd = tokio::fs::read_dir(&staging_dir)
        .await
        .map_err(|e| format!("Failed to read staging dir: {}", e))?;
    while let Ok(Some(entry)) = rd.next_entry().await {
        staging_entries.push(entry.path());
    }

    let source = if staging_entries.len() == 1 && staging_entries[0].is_dir() {
        staging_entries[0].clone()
    } else {
        staging_dir.clone()
    };

    // Atomic replace: move the existing model (if any) out of the way FIRST,
    // then commit the new one, and only then delete the old. This way a crash
    // between the two renames leaves the user with an intact `.old` they can
    // rename back manually — instead of no model at all.
    let backup_dir = final_dir.with_extension("old");
    if final_dir.exists() {
        if backup_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&backup_dir).await;
        }
        tokio::fs::rename(&final_dir, &backup_dir)
            .await
            .map_err(|e| format!("Failed to set aside existing model: {}", e))?;
    }
    if let Err(e) = tokio::fs::rename(&source, &final_dir).await {
        // Commit failed — try to restore the backup so the user keeps their
        // old model instead of ending up with nothing.
        if backup_dir.exists() {
            let _ = tokio::fs::rename(&backup_dir, &final_dir).await;
        }
        return Err(format!("Failed to finalize model dir: {}", e));
    }
    if backup_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&backup_dir).await;
    }

    if staging_dir.exists() {
        if let Err(e) = tokio::fs::remove_dir_all(&staging_dir).await {
            crate::dev_log!(
                "[MODEL] Warning: failed to clean staging dir {}: {}",
                staging_dir.display(),
                e
            );
        }
    }
    let _ = tokio::fs::remove_file(&archive_path).await;

    crate::dev_log!(
        "[MODEL] ✅ Extracted {} to {}",
        model_name,
        final_dir.display()
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
            "path": final_dir.to_string_lossy()
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

    #[tokio::test]
    async fn next_chunk_or_stall_yields_then_ends() {
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
            vec![Ok(vec![1u8, 2, 3]), Ok(vec![4u8, 5])];
        let mut s = futures::stream::iter(chunks);
        assert_eq!(
            next_chunk_or_stall(&mut s, "t").await.unwrap(),
            Some(vec![1u8, 2, 3])
        );
        assert_eq!(
            next_chunk_or_stall(&mut s, "t").await.unwrap(),
            Some(vec![4u8, 5])
        );
        assert_eq!(next_chunk_or_stall(&mut s, "t").await.unwrap(), None);
    }

    #[tokio::test]
    async fn next_chunk_or_stall_propagates_read_error_with_label() {
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> = vec![Err(std::io::Error::other("boom"))];
        let mut s = futures::stream::iter(chunks);
        let err = next_chunk_or_stall(&mut s, "myfile.onnx")
            .await
            .unwrap_err();
        assert!(err.contains("myfile.onnx"), "label missing: {err}");
        assert!(err.contains("boom"), "cause missing: {err}");
    }

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
    fn test_parakeet_v3_is_available_and_recommended_for_french() {
        let models = get_onnx_models();
        let parakeet = models
            .iter()
            .find(|m| m.engine_type == EngineType::Parakeet)
            .expect("Parakeet V3 should be registered");
        assert!(!parakeet.coming_soon);
        assert!(parakeet.is_directory);
        assert!(parakeet.is_recommended);
        assert!(parakeet.supported_languages.iter().any(|l| l == "fr"));
        // Multi-file bundle: url is empty, files list is populated.
        assert!(parakeet.url.is_empty());
        // 5 files: config.json, vocab.txt, encoder int8, decoder int8, nemo128.
        // We intentionally skip encoder-model.onnx.data (2.3 GB fp32 weights,
        // unused in int8 path).
        assert_eq!(parakeet.files.len(), 5);
        let names: Vec<&str> = parakeet.files.iter().map(|f| f.filename.as_str()).collect();
        assert!(names.contains(&"encoder-model.int8.onnx"));
        assert!(!names.contains(&"encoder-model.onnx.data"));
        for f in &parakeet.files {
            assert!(f.url.starts_with("https://huggingface.co/"));
            assert_eq!(
                f.sha256.len(),
                64,
                "SHA-256 hex must be exactly 64 chars for {}",
                f.filename
            );
            assert!(
                f.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "SHA-256 must be hex for {}",
                f.filename
            );
            // Manifest must not contain path traversal or absolute paths.
            assert!(!f.filename.contains(".."));
            assert!(!std::path::Path::new(&f.filename).is_absolute());
        }
    }

    #[test]
    fn test_still_coming_soon_for_pending_engines() {
        let models = get_onnx_models();
        let sense = models
            .iter()
            .find(|m| m.engine_type == EngineType::SenseVoice)
            .unwrap();
        let moon = models
            .iter()
            .find(|m| m.engine_type == EngineType::Moonshine)
            .unwrap();
        assert!(sense.coming_soon);
        assert!(moon.coming_soon);
    }
}
