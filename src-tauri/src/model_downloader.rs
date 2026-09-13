//! Multi-backend model registry + downloader.
//!
//! Models are served by transcribe-rs (Whisper GGML + optional ONNX engines).
//! The registry is engine-aware; already-downloaded files in
//! `~/.local/share/karaoketyper/models/` are preserved across upgrades via
//! `is_model_installed()` which only checks on-disk size.

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

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

// ── Resumable, verified download ─────────────────────────────────────────────

/// How a [`download_resumable`] attempt ended.
#[derive(Debug, PartialEq)]
enum DownloadOutcome {
    Completed,
    Cancelled,
}

/// Start offset of a `Content-Range: bytes <start>-<end>/<total>` header.
fn content_range_start(value: &str) -> Option<u64> {
    value
        .trim()
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Stream a file from disk through SHA-256.
///
/// Reads rather than hashing during download because a resumed transfer only ever sees
/// the bytes it fetched itself — the ones already on disk were never hashed.
async fn file_sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("Failed to open {} for verification: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_READ_BUFFER];

    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Read error while verifying {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify `path` against `expected_sha256`, deleting it on mismatch.
async fn verify_or_discard(path: &Path, expected_sha256: &str) -> Result<(), String> {
    let actual = file_sha256(path).await?;
    if actual.eq_ignore_ascii_case(expected_sha256) {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(path).await;
    Err(format!(
        "Checksum mismatch: expected {}, got {}. The file was discarded.",
        expected_sha256, actual
    ))
}

/// Fetch `url` into `partial_path`, resuming whatever is already there.
///
/// Leaves the fetched bytes in `partial_path` on success; finalizing is the caller's
/// job. The interesting part is everything that can go wrong with a `Range` request,
/// each handled explicitly rather than assumed away:
///
/// - a partial **already at the expected size** (a crash between the last byte and the
///   rename) is verified and accepted, instead of asking for `Range: bytes=<EOF>-` and
///   looping on 416 forever; an oversized one is deleted
/// - a **416** means the server's object ends before our offset, so the partial is
///   wrong: discard it rather than trying to salvage bytes we cannot prove
/// - a **200 to a Range request** means the server ignored the Range and is sending the
///   whole file; appending that to the partial would corrupt the model, so restart
/// - a **206** must start at exactly our offset, or the partial is discarded — anything
///   else corrupts silently on append
/// - a server advertising a total that disagrees with the expected size is rejected
///   *before* anything is written, and one that sends more than expected is cut off at
///   the first excess byte rather than trusted to close the stream
///
/// Cancellation races both the request and every chunk, and keeps the partial so a
/// later attempt resumes.
#[allow(clippy::too_many_arguments)]
async fn download_resumable(
    client: &reqwest::Client,
    label: &str,
    url: &str,
    partial_path: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<DownloadOutcome, String> {
    let mut resume_from = tokio::fs::metadata(partial_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    if let Some(expected) = expected_size {
        if resume_from > expected {
            crate::dev_log!("[MODEL] Partial for {} is oversized; discarding", label);
            let _ = tokio::fs::remove_file(partial_path).await;
            resume_from = 0;
        } else if resume_from == expected && expected > 0 {
            crate::dev_log!(
                "[MODEL] Partial for {} is already full-size; verifying",
                label
            );
            if let Some(sha) = expected_sha256 {
                verify_or_discard(partial_path, sha).await?;
            }
            return Ok(DownloadOutcome::Completed);
        }
    }

    if resume_from > 0 {
        crate::dev_log!("[MODEL] Resuming {} from byte {}", label, resume_from);
    }

    let mut request = client.get(url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
    }

    let response = tokio::select! {
        r = tokio::time::timeout(DOWNLOAD_STALL_TIMEOUT, request.send()) => r
            .map_err(|_| format!(
                "No response within {}s from {}",
                DOWNLOAD_STALL_TIMEOUT.as_secs(), url
            ))?
            .map_err(|e| format!("Failed to start download: {}", e))?,
        _ = cancel.cancelled() => return Ok(DownloadOutcome::Cancelled),
    };

    if resume_from > 0 && response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let _ = tokio::fs::remove_file(partial_path).await;
        return Err("Server object ends before the expected size (HTTP 416)".to_string());
    }

    if !response.status().is_success() {
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    // Any success that is not a 206 means the server did not honour the Range and is
    // sending the whole object. Appending that to the partial would corrupt it, so the
    // partial goes and we start over. Checking `!= 206` rather than `== 200` matters:
    // a 202 or 203 would otherwise slip straight into the append path.
    if resume_from > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        crate::dev_log!("[MODEL] Server ignored Range for {}; restarting", label);
        let _ = tokio::fs::remove_file(partial_path).await;
        resume_from = 0;
    }

    if resume_from > 0 {
        let starts_at = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(content_range_start);
        if starts_at != Some(resume_from) {
            let _ = tokio::fs::remove_file(partial_path).await;
            return Err(format!(
                "Server returned Content-Range starting at {:?}, expected {}",
                starts_at, resume_from
            ));
        }
    }

    if let (Some(expected), Some(len)) = (expected_size, response.content_length()) {
        if resume_from + len != expected {
            // Drop the partial before giving up. Keeping it would make every future
            // attempt reproduce this same mismatch from the same offset, leaving the
            // model permanently undownloadable with no way for the user to clear it.
            if resume_from > 0 {
                let _ = tokio::fs::remove_file(partial_path).await;
            }
            return Err(format!(
                "Server advertises {} bytes, expected {}",
                resume_from + len,
                expected
            ));
        }
    }

    // Never leave the write path uncapped: without a pinned size and without a
    // Content-Length (a chunked response), an untrusted server could otherwise write
    // until the disk is full. The old code derived its cap from the declared size,
    // which was always present; keep an absolute backstop for the same guarantee.
    let known_total = expected_size
        .or_else(|| response.content_length().map(|l| resume_from + l))
        .or(Some(MAX_UNVERIFIED_DOWNLOAD_BYTES));

    let mut file = if resume_from > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(partial_path)
            .await
    } else {
        tokio::fs::File::create(partial_path).await
    }
    .map_err(|e| format!("Failed to open {}: {}", partial_path.display(), e))?;

    let mut downloaded = resume_from;
    let mut stream = response.bytes_stream();
    on_progress(downloaded, known_total.unwrap_or(0));

    loop {
        let chunk = tokio::select! {
            c = next_chunk_or_stall(&mut stream, label) => match c? {
                Some(chunk) => chunk,
                None => break,
            },
            _ = cancel.cancelled() => {
                // Keep the partial: a cancel is usually "not now", not "never".
                let _ = file.flush().await;
                return Ok(DownloadOutcome::Cancelled);
            }
        };

        // Cut a server that oversends at the first excess byte rather than trusting it
        // to close the stream. What it already wrote is tainted, so drop it entirely.
        if let Some(cap) = known_total {
            if downloaded + chunk.len() as u64 > cap {
                drop(file);
                let _ = tokio::fs::remove_file(partial_path).await;
                return Err(format!("Server sent more than the expected {} bytes", cap));
            }
        }

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {}", e))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, known_total.unwrap_or(downloaded));
    }

    // sync_all, not just flush: flush only pushes our buffers into the kernel. The hash
    // below reads the file back, so without this it would be verifying the page cache
    // rather than what actually survives a power cut.
    file.sync_all()
        .await
        .map_err(|e| format!("Sync error: {}", e))?;
    drop(file);

    if let Some(expected) = expected_size {
        if downloaded != expected {
            return Err(format!(
                "Download ended at {} bytes, expected {}",
                downloaded, expected
            ));
        }
    }

    if let Some(sha) = expected_sha256 {
        verify_or_discard(partial_path, sha).await?;
    }

    Ok(DownloadOutcome::Completed)
}

/// Absolute ceiling when neither a pinned size nor a Content-Length is available.
/// Larger than any model we ship, small enough that a hostile server cannot fill a disk.
const MAX_UNVERIFIED_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Read size when streaming a file through SHA-256. Large enough that the syscall cost
/// is noise on a multi-GB file, small enough to yield to the runtime regularly.
const HASH_READ_BUFFER: usize = 1024 * 1024;

/// Attempts per URL before moving on to the next source.
const DOWNLOAD_ATTEMPTS: u32 = 3;

/// Base delay for the backoff between attempts; doubles each time.
const DOWNLOAD_RETRY_BACKOFF: Duration = Duration::from_secs(2);

// ── Cancellation ─────────────────────────────────────────────────────────────

/// Cancel tokens for downloads currently running, keyed by model name.
///
/// A model download is minutes of work over gigabytes; without this the only way to
/// stop one is to quit the app, and the partial file is then orphaned rather than
/// resumed.
static CANCEL_TOKENS: Mutex<Option<HashMap<String, CancellationToken>>> = Mutex::new(None);

fn register_cancel_token(name: &str) -> CancellationToken {
    let token = CancellationToken::new();
    if let Ok(mut guard) = CANCEL_TOKENS.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), token.clone());
    }
    token
}

fn unregister_cancel_token(name: &str) {
    if let Ok(mut guard) = CANCEL_TOKENS.lock() {
        if let Some(map) = guard.as_mut() {
            map.remove(name);
        }
    }
}

/// RAII: drop the token registration however the download ends.
struct CancelTokenGuard<'a>(&'a str);
impl Drop for CancelTokenGuard<'_> {
    fn drop(&mut self) {
        unregister_cancel_token(self.0);
    }
}

/// Ask a running download to stop. Returns false if nothing was running.
///
/// The partial file is deliberately left on disk: a cancelled download is usually a
/// "not now", and keeping the bytes means the next attempt resumes instead of starting
/// the gigabytes over.
pub fn cancel_download(model_name: &str) -> bool {
    let Ok(guard) = CANCEL_TOKENS.lock() else {
        return false;
    };
    match guard.as_ref().and_then(|m| m.get(model_name)) {
        Some(token) => {
            token.cancel();
            crate::dev_log!("[MODEL] Cancellation requested for {}", model_name);
            true
        }
        None => false,
    }
}

/// One file that ships as part of a multi-file model bundle (e.g. Parakeet V3).
/// Hash is mandatory — the download is rejected if the content doesn't match.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModelFile {
    /// Filename inside the model directory (e.g. `encoder-model.int8.onnx`).
    pub filename: String,
    /// Absolute URL to fetch.
    pub url: String,
    /// Expected SHA-256 of the file contents, lowercase hex.
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
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
    /// Expected SHA-256 of a single-file model, lowercase hex.
    ///
    /// The trust anchor for the `url` download: without it a truncated, corrupted or
    /// substituted file is installed and only discovered when transcription misbehaves.
    /// Empty for multi-file bundles, which carry a hash per `ModelFile` instead.
    #[serde(default)]
    pub sha256: String,
    /// Exact size of a single-file model. Lets the downloader recognise a complete
    /// partial, reject a server advertising the wrong total, and cut off one that
    /// oversends — all before any bytes are trusted. 0 when unknown.
    ///
    /// Kept off the wire: it is only ever read inside the downloader, and specta
    /// rejects `u64` outright (`BigIntForbidden`) because JavaScript numbers cannot
    /// represent it — exporting it would break the TypeScript bindings entirely.
    #[serde(skip)]
    #[specta(skip)]
    pub size_bytes: u64,
}

#[derive(Clone, Serialize, Type)]
pub struct DownloadProgress {
    pub model: String,
    pub downloaded_mb: f64,
    pub total_mb: f64,
    pub percent: f64,
    pub status: String,
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

/// Every installable model, from the versioned catalogue.
///
/// Was ~230 lines of `ModelInfo` literals with hand-typed SHA-256 values; it is now
/// `src/catalog/catalog.json`, validated at parse time and checkable against upstream
/// by `scripts/catalog.py`. `installed` is filled in by `check_installed_models`.
pub fn get_available_models() -> Vec<ModelInfo> {
    crate::catalog::models().to_vec()
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

/// Where a catalogue entry lives once installed: a directory for ONNX bundles, a
/// single `.bin` for Whisper.
fn model_path_of(model: &ModelInfo) -> PathBuf {
    if model.is_directory {
        get_models_dir().join(&model.name)
    } else {
        get_model_file_path(&model.name)
    }
}

pub fn get_model_path(model_name: &str) -> Option<String> {
    let models = get_available_models();
    let model = models.iter().find(|m| m.name == model_name)?;
    if !is_model_installed(model) {
        return None;
    }
    Some(model_path_of(model).to_string_lossy().to_string())
}

/// Path of a model `engine` can load, if one is installed — how the hybrid fast lane
/// finds Parakeet without hard-coding a catalogue name. Walks the catalogue and stats
/// each candidate until the first installed one; nothing is cloned.
#[must_use]
pub fn installed_model_path_for(engine: EngineType) -> Option<PathBuf> {
    crate::catalog::models()
        .iter()
        .find(|m| m.engine_type == engine && !m.coming_soon && is_model_installed(m))
        .map(model_path_of)
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

    let cancel = register_cancel_token(&model_name);
    let _cancel_guard = CancelTokenGuard(&model_name);

    // Trust anchors. Both are pinned in the registry for every Whisper model; a model
    // that somehow lacks them still downloads, just without the guarantees.
    let expected_sha256 = (!model_info.sha256.is_empty()).then_some(model_info.sha256.as_str());
    let expected_size = (model_info.size_bytes > 0).then_some(model_info.size_bytes);
    if expected_sha256.is_none() {
        crate::dev_log!(
            "[MODEL] ⚠️ {} has no pinned checksum — cannot verify what is downloaded",
            model_name
        );
    }

    // One entry today. The shape is what matters: with a pinned SHA-256 as the trust
    // anchor an extra source is safe to add, because a mirror serving the wrong bytes
    // fails verification exactly like a corrupted transfer does.
    let sources = [model_info.url.as_str()];

    let mut last_progress_percent: i32 = -1;
    let mut emit_progress = |downloaded: u64, total: u64| {
        let total = if total == 0 { 1 } else { total };
        let percent = (downloaded as f64 / total as f64 * 100.0).min(100.0);
        // A retry that restarts from zero goes backwards. Without this the bar stays
        // frozen at the previous high-water mark for the whole re-download — minutes
        // of apparent hang on a 3 GB model.
        if (percent as i32) < last_progress_percent {
            last_progress_percent = -1;
        }
        if percent as i32 > last_progress_percent {
            last_progress_percent = percent as i32;
            let _ = app_handle.emit(
                "model-download-progress",
                DownloadProgress {
                    model: model_name.clone(),
                    downloaded_mb: downloaded as f64 / (1024.0 * 1024.0),
                    total_mb: total as f64 / (1024.0 * 1024.0),
                    percent,
                    status: "downloading".to_string(),
                },
            );
        }
    };

    let mut last_error = String::from("no download source available");
    let mut completed = false;

    'sources: for url in sources {
        for attempt in 1..=DOWNLOAD_ATTEMPTS {
            if cancel.is_cancelled() {
                break 'sources;
            }

            match download_resumable(
                &client,
                &model_name,
                url,
                &temp_path,
                expected_size,
                expected_sha256,
                &cancel,
                &mut emit_progress,
            )
            .await
            {
                Ok(DownloadOutcome::Completed) => {
                    completed = true;
                    break 'sources;
                }
                Ok(DownloadOutcome::Cancelled) => break 'sources,
                Err(e) => {
                    last_error = e;
                    crate::dev_log!(
                        "[MODEL] {} attempt {}/{} failed: {}",
                        model_name,
                        attempt,
                        DOWNLOAD_ATTEMPTS,
                        last_error
                    );
                    if attempt < DOWNLOAD_ATTEMPTS {
                        // Backoff, but stay cancellable while waiting — a user who gave
                        // up should not have to sit through the retry schedule.
                        let backoff = DOWNLOAD_RETRY_BACKOFF * 2u32.saturating_pow(attempt - 1);
                        tokio::select! {
                            _ = tokio::time::sleep(backoff) => {}
                            _ = cancel.cancelled() => break 'sources,
                        }
                    }
                }
            }
        }
    }

    if cancel.is_cancelled() {
        // The partial stays on disk on purpose: the next attempt resumes from it.
        crate::dev_log!("[MODEL] Download of {} cancelled by user", model_name);
        let _ = app_handle.emit(
            "model-download-progress",
            DownloadProgress {
                model: model_name.clone(),
                downloaded_mb: 0.0,
                total_mb: model_info.size_mb as f64,
                percent: 0.0,
                status: "cancelled".to_string(),
            },
        );
        return Err(format!("Téléchargement de '{}' annulé.", model_name));
    }

    if !completed {
        return Err(format!(
            "Download of '{}' failed after {} attempts: {}",
            model_name, DOWNLOAD_ATTEMPTS, last_error
        ));
    }

    let total_size = tokio::fs::metadata(&temp_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

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

    // Without this the Cancel button is a silent no-op for every directory-based model
    // — including Parakeet V3, which is both the largest bundle and the one recommended
    // to French users, i.e. exactly the download someone would want to stop.
    let cancel = register_cancel_token(&model_name);
    let _cancel_guard = CancelTokenGuard(&model_name);

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
        if mf.sha256.len() != 64 || !mf.sha256.chars().all(|c: char| c.is_ascii_hexdigit()) {
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
            if cancel.is_cancelled() {
                let _ = tokio::fs::remove_dir_all(&staging_dir).await;
                return Err(format!("Téléchargement de '{}' annulé.", model_name));
            }
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
    fn model_path_of_is_a_directory_for_bundles_and_a_bin_for_whisper() {
        let models = get_available_models();
        let parakeet = models
            .iter()
            .find(|m| m.engine_type == EngineType::Parakeet)
            .expect("Parakeet V3 should be registered");
        assert!(model_path_of(parakeet).ends_with(&parakeet.name));
        let small = models
            .iter()
            .find(|m| m.name == "ggml-small")
            .expect("ggml-small should be registered");
        assert!(model_path_of(small).ends_with("ggml-small.bin"));
    }

    #[test]
    fn test_parakeet_v3_is_available_and_recommended_for_french() {
        let models = get_available_models();
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
                f.sha256.chars().all(|c: char| c.is_ascii_hexdigit()),
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
        let models = get_available_models();
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

    // ── Resumable download ───────────────────────────────────────────────────

    #[test]
    fn content_range_start_parses_a_real_header() {
        assert_eq!(content_range_start("bytes 1024-2047/2048"), Some(1024));
        assert_eq!(content_range_start("  bytes 0-9/10  "), Some(0));
    }

    #[test]
    fn content_range_start_rejects_anything_else() {
        // A header we cannot parse must read as "unknown", never as offset 0 — the
        // caller compares it against the resume offset to decide whether appending
        // is safe, and a wrong 0 there would corrupt the file.
        for header in [
            "items 0-9/10",
            "bytes */2048",
            "",
            "bytes abc-9/10",
            "0-9/10",
        ] {
            assert_eq!(content_range_start(header), None, "accepted {:?}", header);
        }
    }

    /// Every single-file model must be verifiable. A model shipped without a hash
    /// installs whatever the network returned.
    #[test]
    fn every_single_file_model_pins_a_checksum_and_size() {
        for m in get_available_models() {
            if m.is_directory || m.url.is_empty() {
                continue;
            }
            assert_eq!(
                m.sha256.len(),
                64,
                "{} has no usable sha256 ({:?})",
                m.name,
                m.sha256
            );
            assert!(
                m.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} sha256 is not hex",
                m.name
            );
            assert!(m.size_bytes > 0, "{} has no pinned size", m.name);
        }
    }

    /// The pinned size and the advertised MB must agree, or the progress bar lies and
    /// the oversend cut-off fires at the wrong place.
    #[test]
    fn pinned_sizes_agree_with_the_advertised_megabytes() {
        for m in get_available_models() {
            if m.is_directory || m.size_bytes == 0 {
                continue;
            }
            let mb = m.size_bytes as f64 / (1024.0 * 1024.0);
            let declared = m.size_mb as f64;
            assert!(
                (mb - declared).abs() < declared * 0.05 + 1.0,
                "{}: pinned {} bytes ({:.0} MB) vs declared {} MB",
                m.name,
                m.size_bytes,
                mb,
                declared
            );
        }
    }

    #[tokio::test]
    async fn file_sha256_matches_a_known_digest() {
        let dir = std::env::temp_dir().join("kt-sha-test");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join("abc.bin");
        tokio::fs::write(&path, b"abc").await.unwrap();

        // Well-known SHA-256 of "abc".
        assert_eq!(
            file_sha256(&path).await.unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = tokio::fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn verify_or_discard_deletes_a_file_that_does_not_match() {
        let dir = std::env::temp_dir().join("kt-sha-test");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join("bad.bin");
        tokio::fs::write(&path, b"not what was expected")
            .await
            .unwrap();

        let err = verify_or_discard(&path, &"0".repeat(64)).await.unwrap_err();
        assert!(err.contains("Checksum mismatch"), "{}", err);
        assert!(
            !path.exists(),
            "a file that failed verification must not be left on disk"
        );
    }

    #[tokio::test]
    async fn verify_or_discard_keeps_a_file_that_matches() {
        let dir = std::env::temp_dir().join("kt-sha-test");
        let _ = tokio::fs::create_dir_all(&dir).await;
        let path = dir.join("good.bin");
        tokio::fs::write(&path, b"abc").await.unwrap();

        verify_or_discard(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .await
        .unwrap();
        assert!(path.exists());
        let _ = tokio::fs::remove_file(&path).await;
    }

    /// Cancelling something that is not running must not pretend it did anything.
    #[test]
    fn cancelling_an_idle_model_reports_false() {
        assert!(!cancel_download("no-such-model-is-downloading"));
    }

    // ── Resume against a real socket ─────────────────────────────────────────
    //
    // The Range traps only exist in the interaction with a server, so they are tested
    // against one rather than against a mock of our own assumptions. Minimal HTTP is
    // enough: these are the exact responses the traps describe.

    /// What the fake server should do with the request it receives.
    #[derive(Clone, Copy)]
    enum ServerBehaviour {
        /// Honour Range, but close the connection after `cut` bytes of body.
        TruncateAfter(usize),
        /// Honour Range and serve to the end.
        Complete,
        /// Answer a Range request with 200 and the whole body (server ignored Range).
        IgnoreRange,
        /// Answer a Range request with 206 but the wrong Content-Range start.
        WrongOffset,
        /// Answer a Range request with 416.
        RangeNotSatisfiable,
    }

    /// Serve exactly one request on an ephemeral port; returns the bound address.
    async fn spawn_server(body: Vec<u8>, behaviour: ServerBehaviour) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();

            let mut req = vec![0u8; 2048];
            let n = sock.read(&mut req).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&req[..n]).to_string();

            let range_start: Option<usize> = head
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("range:"))
                .and_then(|l| l.split('=').nth(1))
                .and_then(|v| v.trim().trim_end_matches('-').parse().ok());

            let total = body.len();
            let (status, extra, slice): (&str, String, &[u8]) = match (behaviour, range_start) {
                (ServerBehaviour::RangeNotSatisfiable, Some(_)) => {
                    ("416 Range Not Satisfiable", String::new(), &[][..])
                }
                (ServerBehaviour::IgnoreRange, Some(_)) => ("200 OK", String::new(), &body[..]),
                (ServerBehaviour::WrongOffset, Some(start)) => (
                    "206 Partial Content",
                    format!(
                        "Content-Range: bytes {}-{}/{}\r\n",
                        start + 7,
                        total - 1,
                        total
                    ),
                    &body[start..],
                ),
                (_, Some(start)) => (
                    "206 Partial Content",
                    format!("Content-Range: bytes {}-{}/{}\r\n", start, total - 1, total),
                    &body[start..],
                ),
                (_, None) => ("200 OK", String::new(), &body[..]),
            };

            let header = format!(
                "HTTP/1.1 {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                status,
                slice.len(),
                extra
            );
            let _ = sock.write_all(header.as_bytes()).await;

            // TruncateAfter simulates the network dying mid-transfer: fewer body bytes
            // than the Content-Length promised, then the socket closes.
            let to_send = match behaviour {
                ServerBehaviour::TruncateAfter(cut) => &slice[..cut.min(slice.len())],
                _ => slice,
            };
            let _ = sock.write_all(to_send).await;
            let _ = sock.shutdown().await;
        });

        format!("http://{}/model.bin", addr)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("kt-download-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    fn sha_of(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::new().chain_update(bytes).finalize())
    }

    /// The headline property: a transfer cut in half resumes and ends up correct.
    #[tokio::test]
    async fn an_interrupted_download_resumes_and_verifies() {
        let body: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
        let sha = sha_of(&body);
        let path = scratch("resume.bin");
        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();

        // First attempt: the server dies after 12 000 bytes.
        let url = spawn_server(body.clone(), ServerBehaviour::TruncateAfter(12_000)).await;
        let first = download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&sha),
            &cancel,
            |_, _| {},
        )
        .await;
        assert!(
            first.is_err(),
            "a truncated transfer must not report success"
        );
        // How many of the server's 12 000 bytes actually landed before the socket closed
        // is a TCP timing detail — all of them on loopback, fewer on a loaded CI runner.
        // The property under test is that *something* was kept and that it falls short
        // of the whole file, not an exact count.
        let partial = std::fs::metadata(&path).unwrap().len();
        assert!(
            partial > 0 && partial < body.len() as u64,
            "expected a partial to resume from, got {partial} of {} bytes",
            body.len()
        );

        // Second attempt: resumes from byte 12 000 and finishes.
        let url = spawn_server(body.clone(), ServerBehaviour::Complete).await;
        download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&sha),
            &cancel,
            |_, _| {},
        )
        .await
        .expect("resume should complete");

        assert_eq!(std::fs::read(&path).unwrap(), body, "resumed file differs");
        let _ = std::fs::remove_file(&path);
    }

    /// The whole point of pinning a hash: wrong bytes must not be installed.
    #[tokio::test]
    async fn a_corrupted_body_is_rejected_and_discarded() {
        let body: Vec<u8> = vec![0xAB; 5_000];
        let path = scratch("corrupt.bin");
        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();

        let url = spawn_server(body.clone(), ServerBehaviour::Complete).await;
        let err = download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&"0".repeat(64)), // not the body's hash
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(err.contains("Checksum mismatch"), "{}", err);
        assert!(
            !path.exists(),
            "a file that failed verification must be gone"
        );
    }

    /// Trap: the server ignores Range and replies 200 with the whole file. Appending
    /// that to the partial would corrupt it, so the partial must be dropped first.
    #[tokio::test]
    async fn a_200_to_a_range_request_restarts_from_zero() {
        let body: Vec<u8> = (0..9_000u32).map(|i| (i % 97) as u8).collect();
        let sha = sha_of(&body);
        let path = scratch("ignored-range.bin");
        std::fs::write(&path, &body[..3_000]).unwrap();

        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();
        let url = spawn_server(body.clone(), ServerBehaviour::IgnoreRange).await;

        download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&sha),
            &cancel,
            |_, _| {},
        )
        .await
        .expect("should restart cleanly");

        assert_eq!(std::fs::read(&path).unwrap(), body);
        let _ = std::fs::remove_file(&path);
    }

    /// Trap: a 206 starting anywhere but our offset would corrupt the file on append.
    #[tokio::test]
    async fn a_206_at_the_wrong_offset_is_refused() {
        let body: Vec<u8> = (0..9_000u32).map(|i| (i % 97) as u8).collect();
        let path = scratch("wrong-offset.bin");
        std::fs::write(&path, &body[..3_000]).unwrap();

        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();
        let url = spawn_server(body.clone(), ServerBehaviour::WrongOffset).await;

        let err = download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&sha_of(&body)),
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(err.contains("Content-Range"), "{}", err);
        assert!(
            !path.exists(),
            "a partial we cannot trust must be discarded"
        );
    }

    /// Trap: 416 means the server object ends before our offset — the partial is wrong.
    #[tokio::test]
    async fn a_416_discards_the_partial() {
        let body: Vec<u8> = vec![7u8; 9_000];
        let path = scratch("range-416.bin");
        std::fs::write(&path, &body[..3_000]).unwrap();

        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();
        let url = spawn_server(body.clone(), ServerBehaviour::RangeNotSatisfiable).await;

        let err = download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&sha_of(&body)),
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(err.contains("416"), "{}", err);
        assert!(!path.exists());
    }

    /// Trap: a partial already at full size (crash between last byte and rename) is
    /// verified and accepted, never turned into a `Range: bytes=<EOF>-` request.
    #[tokio::test]
    async fn a_complete_partial_is_verified_without_touching_the_network() {
        let body: Vec<u8> = vec![3u8; 6_000];
        let path = scratch("already-complete.bin");
        std::fs::write(&path, &body).unwrap();

        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();
        // Deliberately unroutable: reaching the network at all is the failure.
        let url = "http://127.0.0.1:1/model.bin";

        download_resumable(
            &client,
            "test",
            url,
            &path,
            Some(body.len() as u64),
            Some(&sha_of(&body)),
            &cancel,
            |_, _| {},
        )
        .await
        .expect("a full-size partial must be accepted offline");

        assert_eq!(std::fs::read(&path).unwrap(), body);
        let _ = std::fs::remove_file(&path);
    }

    /// Cancelling keeps the bytes, so the next attempt resumes instead of restarting.
    ///
    /// The cancel fires *during* the transfer, not before it: cancelling before the
    /// request goes out exercises only the first arm of the select and proves nothing
    /// about the partial, which is the property that actually matters.
    #[tokio::test]
    async fn cancelling_mid_transfer_keeps_the_partial() {
        let body: Vec<u8> = vec![9u8; 400_000];
        let path = scratch("cancelled.bin");
        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();

        let url = spawn_server(body.clone(), ServerBehaviour::Complete).await;

        let trigger = cancel.clone();
        let outcome = download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(body.len() as u64),
            Some(&sha_of(&body)),
            &cancel,
            move |downloaded, _| {
                // Cancel once real bytes have landed, so there is a partial to keep.
                if downloaded > 0 {
                    trigger.cancel();
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(outcome, DownloadOutcome::Cancelled);
        let kept = std::fs::metadata(&path)
            .expect("the partial must survive a cancel so the next attempt resumes")
            .len();
        assert!(
            kept > 0,
            "cancel left an empty file, nothing to resume from"
        );
        assert!(
            kept < body.len() as u64,
            "the transfer was not actually cut short"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A partial at full size but with the wrong content must be discarded, not blessed.
    #[tokio::test]
    async fn a_full_size_partial_with_a_bad_hash_is_discarded() {
        let body: Vec<u8> = vec![1u8; 6_000];
        let path = scratch("full-but-wrong.bin");
        std::fs::write(&path, vec![2u8; 6_000]).unwrap(); // right size, wrong bytes

        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();
        let err = download_resumable(
            &client,
            "test",
            "http://127.0.0.1:1/model.bin",
            &path,
            Some(body.len() as u64),
            Some(&sha_of(&body)),
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(err.contains("Checksum mismatch"), "{}", err);
        assert!(
            !path.exists(),
            "a full-size partial that fails its hash must go"
        );
    }

    /// A server that oversends is cut off, and what it wrote is not kept.
    #[tokio::test]
    async fn a_server_that_advertises_the_wrong_size_is_refused() {
        let body: Vec<u8> = vec![4u8; 10_000];
        let path = scratch("wrong-total.bin");
        let client = build_download_client().unwrap();
        let cancel = CancellationToken::new();

        let url = spawn_server(body.clone(), ServerBehaviour::Complete).await;
        let err = download_resumable(
            &client,
            "test",
            &url,
            &path,
            Some(99_999), // we expect a different size than the server will serve
            None,
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert!(err.contains("advertises"), "{}", err);
    }
}
