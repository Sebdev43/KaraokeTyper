//! Native Rust STT engine using whisper-rs (whisper.cpp bindings)
//!
//! This replaces the Python-based sherpa-onnx implementation with a pure Rust solution.
//! Uses GGML models from ggerganov/whisper.cpp releases.

use std::fmt;
use std::path::PathBuf;
use std::sync::Mutex;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
}

impl WhisperSttEngine {
    /// Create a new WhisperSttEngine
    ///
    /// # Arguments
    /// * `model_path` - Path to the GGML model file (.bin)
    /// * `language` - Language code (e.g., "fr", "en", "auto")
    pub fn new(model_path: &str, language: &str) -> Self {
        Self {
            context: Mutex::new(None),
            model_path: PathBuf::from(model_path),
            language: language.to_string(),
        }
    }

    /// Initialize the Whisper context by loading the model
    pub fn initialize(&self) -> Result<(), WhisperError> {
        if !self.model_path.exists() {
            return Err(WhisperError::ModelNotFound(
                self.model_path.display().to_string(),
            ));
        }

        #[cfg(not(debug_assertions))]
        let params = WhisperContextParameters::default();

        #[cfg(debug_assertions)]
        let params = {
            let p = WhisperContextParameters::default();
            // In debug mode, we might want to enable GPU if available
            p
        };

        let ctx = WhisperContext::new_with_params(
            self.model_path.to_str().unwrap_or_default(),
            params,
        )
        .map_err(|e| WhisperError::ModelLoadError(format!("{:?}", e)))?;

        let mut guard = self.context.lock().unwrap();
        *guard = Some(ctx);

        #[cfg(debug_assertions)]
        println!("[Whisper] Model loaded: {}", self.model_path.display());

        Ok(())
    }

    /// Check if the engine is initialized
    pub fn is_initialized(&self) -> bool {
        self.context.lock().unwrap().is_some()
    }

    /// Transcribe audio data
    ///
    /// # Arguments
    /// * `audio_data` - PCM audio samples at 16kHz, mono, f32 format
    ///
    /// # Returns
    /// Transcribed text or error
    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, WhisperError> {
        let guard = self.context.lock().unwrap();
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

        // Create inference parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

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
        params.set_single_segment(true);

        // Suppress non-speech tokens for cleaner output
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);

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

    /// Get the model path
    pub fn model_path(&self) -> &PathBuf {
        &self.model_path
    }

    /// Get the configured language
    pub fn language(&self) -> &str {
        &self.language
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
        let engine = WhisperSttEngine::new("/path/to/model.bin", "fr");
        assert_eq!(engine.language(), "fr");
        assert!(!engine.is_initialized());
    }

    #[test]
    fn test_model_not_found() {
        let engine = WhisperSttEngine::new("/nonexistent/model.bin", "fr");
        let result = engine.initialize();
        assert!(matches!(result, Err(WhisperError::ModelNotFound(_))));
    }
}
