//! Native Rust STT engine using whisper-rs (whisper.cpp bindings)
//!
//! This replaces the Python-based sherpa-onnx implementation with a pure Rust solution.
//! Uses GGML models from ggerganov/whisper.cpp releases.

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Suppress all whisper.cpp C library verbose output (beam search, model loading, etc.)
/// Only called once at startup. Our own [Whisper] logs via println! are unaffected.
pub fn suppress_whisper_cpp_logs() {
    unsafe {
        whisper_rs_sys::whisper_log_set(None, std::ptr::null_mut());
    }
}

/// Maximum consecutive GPU failures before auto-disabling GPU
const MAX_GPU_FAILURES: u32 = 3;

/// Global counter for consecutive GPU initialization failures
static GPU_FAILURE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Record a GPU failure and return the new failure count
pub fn record_gpu_failure() -> u32 {
    let count = GPU_FAILURE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("[GPU] Failure recorded. Total consecutive failures: {}", count);
    if count >= MAX_GPU_FAILURES {
        eprintln!("[GPU] GPU auto-disabled after {} consecutive failures", count);
    }
    count
}

/// Reset GPU failure counter (called after successful GPU init)
pub fn reset_gpu_failures() {
    let previous = GPU_FAILURE_COUNT.swap(0, Ordering::Relaxed);
    if previous > 0 {
        println!("[GPU] Failure counter reset (was: {})", previous);
    }
}

/// Get current GPU failure count
pub fn get_gpu_failure_count() -> u32 {
    GPU_FAILURE_COUNT.load(Ordering::Relaxed)
}

/// Check if GPU is disabled due to too many failures
pub fn is_gpu_disabled_by_failures() -> bool {
    GPU_FAILURE_COUNT.load(Ordering::Relaxed) >= MAX_GPU_FAILURES
}

/// Whisper STT error types
#[derive(Debug)]
pub enum WhisperError {
    ModelNotFound(String),
    ModelLoadError(String),
    TranscriptionError(String),
    InvalidAudio(String),
}

impl fmt::Display for WhisperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WhisperError::ModelNotFound(path) => write!(f, "Model not found: {}", path),
            WhisperError::ModelLoadError(msg) => write!(f, "Failed to load model: {}", msg),
            WhisperError::TranscriptionError(msg) => write!(f, "Transcription failed: {}", msg),
            WhisperError::InvalidAudio(msg) => write!(f, "Invalid audio data: {}", msg),
        }
    }
}

impl std::error::Error for WhisperError {}

/// Result of GPU initialization attempt
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GpuInitResult {
    /// Mode used: "gpu" or "cpu"
    pub mode: String,
    /// Device name if GPU was used
    pub device_name: Option<String>,
    /// Reason for fallback if GPU failed
    pub fallback_reason: Option<String>,
    /// Current GPU failure count
    pub failure_count: u32,
}

impl GpuInitResult {
    pub fn gpu_success(device_name: Option<String>) -> Self {
        Self {
            mode: "gpu".to_string(),
            device_name,
            fallback_reason: None,
            failure_count: get_gpu_failure_count(),
        }
    }

    pub fn cpu_by_choice() -> Self {
        Self {
            mode: "cpu".to_string(),
            device_name: None,
            fallback_reason: None,
            failure_count: get_gpu_failure_count(),
        }
    }

    pub fn cpu_fallback(reason: String) -> Self {
        Self {
            mode: "cpu".to_string(),
            device_name: None,
            fallback_reason: Some(reason),
            failure_count: get_gpu_failure_count(),
        }
    }

    pub fn cpu_disabled_by_failures() -> Self {
        Self {
            mode: "cpu".to_string(),
            device_name: None,
            fallback_reason: Some(format!("GPU auto-disabled after {} consecutive failures", MAX_GPU_FAILURES)),
            failure_count: get_gpu_failure_count(),
        }
    }
}

/// Clean up Whisper transcription by removing artifacts and hallucinations
fn clean_transcription(text: &str) -> String {
    let mut result = text.trim().to_string();

    // Remove common Whisper hallucination patterns at start/end
    let artifacts = [
        "???", "?", "...", "…",
        "[Music]", "[Musique]", "[Applause]", "[Applaudissements]",
        "[Silence]", "[Rires]", "[Laughter]", "[BLANK_AUDIO]",
        "(musique)", "(music)", "(silence)",
        "♪", "♫", "🎵",
        "Sous-titres", "Subtitles", "sous-titres réalisés",
        "Merci d'avoir regardé", "Thanks for watching",
        "À bientôt", "à la prochaine",
    ];

    // Remove artifacts from start
    loop {
        let original = result.clone();
        result = result.trim().to_string();

        for artifact in &artifacts {
            if result.to_lowercase().starts_with(&artifact.to_lowercase()) {
                result = result[artifact.len()..].trim_start().to_string();
            }
        }

        // Remove leading punctuation that doesn't make sense
        while result.starts_with('?') || result.starts_with('.') || result.starts_with('!') || result.starts_with(',') {
            result = result[1..].trim_start().to_string();
        }

        if result == original {
            break;
        }
    }

    // Remove artifacts from end
    loop {
        let original = result.clone();
        result = result.trim().to_string();

        for artifact in &artifacts {
            if result.to_lowercase().ends_with(&artifact.to_lowercase()) {
                result = result[..result.len() - artifact.len()].trim_end().to_string();
            }
        }

        if result == original {
            break;
        }
    }

    result.trim().to_string()
}

/// Native Rust STT engine using whisper-rs
pub struct WhisperSttEngine {
    context: Mutex<Option<WhisperContext>>,
    model_path: PathBuf,
    language: String,
    use_gpu: bool,
    gpu_device_index: Option<u32>,
}

impl WhisperSttEngine {
    /// Create a new WhisperSttEngine
    ///
    /// # Arguments
    /// * `model_path` - Path to the GGML model file (.bin)
    /// * `language` - Language code (e.g., "fr", "en", "auto")
    /// * `use_gpu` - Enable GPU acceleration (requires compile-time GPU features)
    /// * `gpu_device_index` - Optional GPU device index for multi-GPU systems
    pub fn new(model_path: &str, language: &str, use_gpu: bool, gpu_device_index: Option<u32>) -> Self {
        Self {
            context: Mutex::new(None),
            model_path: PathBuf::from(model_path),
            language: language.to_string(),
            use_gpu,
            gpu_device_index,
        }
    }

    /// Initialize the Whisper context by loading the model
    ///
    /// If GPU is enabled but fails to initialize, automatically falls back to CPU mode.
    /// Returns information about which mode was actually used.
    pub fn initialize(&self) -> Result<GpuInitResult, WhisperError> {
        if !self.model_path.exists() {
            return Err(WhisperError::ModelNotFound(
                self.model_path.display().to_string(),
            ));
        }

        // Configure GPU acceleration based on user preference
        // GPU is only used if:
        // 1. User enabled it in settings (use_gpu = true)
        // 2. App was compiled with GPU features (gpu-vulkan or gpu-metal)
        // 3. GPU hasn't been auto-disabled due to failures
        let gpu_available = cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal");
        let gpu_disabled_by_failures = is_gpu_disabled_by_failures();
        let should_use_gpu = self.use_gpu && gpu_available && !gpu_disabled_by_failures;

        println!(
            "[Whisper] GPU: user_pref={}, compiled={}, disabled_by_failures={}, active={}",
            self.use_gpu, gpu_available, gpu_disabled_by_failures, should_use_gpu
        );

        let model_path_str = self.model_path.to_str().unwrap_or_default();

        // If GPU disabled by failures, use CPU directly
        if self.use_gpu && gpu_available && gpu_disabled_by_failures {
            println!("[Whisper] GPU requested but disabled due to {} consecutive failures", get_gpu_failure_count());

            let mut cpu_params = WhisperContextParameters::default();
            cpu_params.use_gpu(false);

            let ctx = WhisperContext::new_with_params(model_path_str, cpu_params)
                .map_err(|e| WhisperError::ModelLoadError(format!("{:?}", e)))?;

            let mut guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(ctx);

            println!("[Whisper] Model loaded (CPU - GPU disabled): {}", self.model_path.display());
            return Ok(GpuInitResult::cpu_disabled_by_failures());
        }

        // Try to load with GPU if requested
        let (ctx, init_result) = if should_use_gpu {
            let mut gpu_params = WhisperContextParameters::default();
            gpu_params.use_gpu(true);

            // Set GPU device index if specified
            if let Some(device_idx) = self.gpu_device_index {
                gpu_params.gpu_device(device_idx as i32);
                println!("[Whisper] Using GPU device index: {}", device_idx);
            }

            match WhisperContext::new_with_params(model_path_str, gpu_params) {
                Ok(ctx) => {
                    // GPU success - reset failure counter
                    reset_gpu_failures();
                    println!("[Whisper] ✓ GPU acceleration enabled successfully");
                    (ctx, GpuInitResult::gpu_success(None))
                }
                Err(gpu_error) => {
                    // GPU failed - record failure and fallback to CPU
                    let failure_count = record_gpu_failure();
                    let error_msg = format!("{:?}", gpu_error);

                    eprintln!(
                        "[Whisper] ⚠ GPU initialization failed (attempt {}): {}",
                        failure_count, error_msg
                    );
                    eprintln!("[Whisper] → Falling back to CPU mode...");

                    let mut cpu_params = WhisperContextParameters::default();
                    cpu_params.use_gpu(false);

                    let ctx = WhisperContext::new_with_params(model_path_str, cpu_params)
                        .map_err(|e| {
                            WhisperError::ModelLoadError(format!(
                                "GPU failed ({}), CPU fallback also failed: {:?}",
                                error_msg, e
                            ))
                        })?;

                    (ctx, GpuInitResult::cpu_fallback(error_msg))
                }
            }
        } else {
            // CPU mode requested by user
            let mut cpu_params = WhisperContextParameters::default();
            cpu_params.use_gpu(false);

            let ctx = WhisperContext::new_with_params(model_path_str, cpu_params)
                .map_err(|e| WhisperError::ModelLoadError(format!("{:?}", e)))?;

            (ctx, GpuInitResult::cpu_by_choice())
        };

        let mut guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(ctx);

        println!("[Whisper] Model loaded ({}): {}", init_result.mode, self.model_path.display());

        Ok(init_result)
    }

    pub fn unload(&self) {
        let mut guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            *guard = None;
            crate::dev_log!("[STT] Model unloaded from memory");
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.context.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// Transcribe audio data
    ///
    /// # Arguments
    /// * `audio_data` - PCM audio samples at 16kHz, mono, f32 format
    ///
    /// # Returns
    /// Transcribed text or error
    pub fn transcribe(&self, audio_data: &[f32], initial_prompt: Option<&str>) -> Result<String, WhisperError> {
        let guard = self.context.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = guard
            .as_ref()
            .ok_or_else(|| WhisperError::TranscriptionError("Engine not initialized".to_string()))?;

        if audio_data.is_empty() {
            return Err(WhisperError::InvalidAudio("Empty audio data".to_string()));
        }

        // Minimum audio length check (whisper needs at least some audio)
        // At 16kHz, 0.1 seconds = 1600 samples
        if audio_data.len() < 1600 {
            return Err(WhisperError::InvalidAudio(
                "Audio too short (minimum 0.1 seconds)".to_string(),
            ));
        }

        #[cfg(debug_assertions)]
        {
            let duration = audio_data.len() as f32 / 16000.0;
            println!("[Whisper] Transcribing {:.2}s of audio ({} samples)", duration, audio_data.len());
        }

        // GPU: use Greedy to minimize VRAM usage and speed up inference
        // CPU: use BeamSearch for better accuracy (VRAM not a concern)
        let strategy = if self.use_gpu {
            #[cfg(debug_assertions)]
            println!("[Whisper] Using Greedy decoding (GPU mode - lower VRAM usage)");
            SamplingStrategy::Greedy { best_of: 1 }
        } else {
            SamplingStrategy::BeamSearch {
                beam_size: 3,
                patience: -1.0,
            }
        };
        let mut params = FullParams::new(strategy);

        // Scale whisper.cpp threads to CPU: use half the cores (min 1, max 8)
        // Prevents virtual memory explosion from too many malloc arenas
        // while still scaling with powerful CPUs for faster transcription
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get() as i32)
            .unwrap_or(2);
        let n_threads = (cpu_count / 2).max(1).min(8);
        params.set_n_threads(n_threads);

        // Set language (empty string means auto-detect)
        if self.language != "auto" && !self.language.is_empty() {
            params.set_language(Some(&self.language));
        }

        // Optimize for real-time dictation
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_translate(false);
        params.set_no_context(true);
        // Allow multiple segments for long audio (>30s)
        // single_segment=true would truncate long dictations
        params.set_single_segment(false);

        // Suppress non-speech tokens for cleaner output
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);

        // Apply initial_prompt to bias Whisper toward custom vocabulary
        if let Some(prompt) = initial_prompt {
            if !prompt.is_empty() {
                params.set_initial_prompt(prompt);
                #[cfg(debug_assertions)]
                println!("[Whisper] Using initial_prompt: '{}'", prompt);
            }
        }

        // Create state for this transcription
        let mut state = ctx
            .create_state()
            .map_err(|e| WhisperError::TranscriptionError(format!("{:?}", e)))?;

        // Run inference
        state
            .full(params, audio_data)
            .map_err(|e| WhisperError::TranscriptionError(format!("{:?}", e)))?;

        // Collect all segments
        let num_segments = state.full_n_segments()
            .map_err(|e| WhisperError::TranscriptionError(format!("{:?}", e)))?;

        let mut result = String::new();
        for i in 0..num_segments {
            if let Ok(segment) = state.full_get_segment_text(i) {
                result.push_str(&segment);
            }
        }

        // Clean up the result - remove Whisper artifacts and hallucinations
        let result = clean_transcription(&result);

        #[cfg(debug_assertions)]
        println!("[Whisper] Transcription: \"{}\"", result);

        Ok(result)
    }

}

impl Drop for WhisperSttEngine {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        println!("[Whisper] Engine dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine = WhisperSttEngine::new("/path/to/model.bin", "fr", false, None);
        assert_eq!(engine.language, "fr");
        assert!(engine.context.lock().unwrap().is_none());
    }

    #[test]
    fn test_model_not_found() {
        let engine = WhisperSttEngine::new("/nonexistent/model.bin", "fr", false, None);
        let result = engine.initialize();
        assert!(matches!(result, Err(WhisperError::ModelNotFound(_))));
    }

    #[test]
    fn test_clean_transcription_normal_text() {
        assert_eq!(clean_transcription("Bonjour le monde"), "Bonjour le monde");
        assert_eq!(clean_transcription("  Hello world  "), "Hello world");
    }

    #[test]
    fn test_clean_transcription_removes_artifacts() {
        assert_eq!(clean_transcription("[Music] Hello"), "Hello");
        assert_eq!(clean_transcription("[BLANK_AUDIO]"), "");
        assert_eq!(clean_transcription("♪ ♫ some text ♪"), "some text");
        assert_eq!(clean_transcription("[Silence] Bonjour [Musique]"), "Bonjour");
    }

    #[test]
    fn test_clean_transcription_removes_leading_punctuation() {
        assert_eq!(clean_transcription("??? Hello"), "Hello");
        assert_eq!(clean_transcription("... world"), "world");
        assert_eq!(clean_transcription("!,. test"), "test");
    }

    #[test]
    fn test_clean_transcription_removes_hallucinations() {
        assert_eq!(clean_transcription("Merci d'avoir regardé"), "");
        assert_eq!(clean_transcription("Thanks for watching"), "");
        assert_eq!(clean_transcription("À bientôt"), "");
        // "Sous-titres" artifact matches prefix, leaving "réalisés"
        assert_eq!(clean_transcription("Sous-titres"), "");
    }

    #[test]
    fn test_gpu_failure_counter() {
        reset_gpu_failures();
        assert_eq!(get_gpu_failure_count(), 0);
        assert!(!is_gpu_disabled_by_failures());

        record_gpu_failure();
        assert_eq!(get_gpu_failure_count(), 1);
        assert!(!is_gpu_disabled_by_failures());

        record_gpu_failure();
        record_gpu_failure();
        assert_eq!(get_gpu_failure_count(), 3);
        assert!(is_gpu_disabled_by_failures());

        reset_gpu_failures();
        assert_eq!(get_gpu_failure_count(), 0);
        assert!(!is_gpu_disabled_by_failures());
    }
}
