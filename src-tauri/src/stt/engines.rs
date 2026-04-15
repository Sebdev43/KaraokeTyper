use std::path::Path;

use transcribe_rs::{
    engines::whisper::{WhisperEngine, WhisperInferenceParams},
    TranscriptionEngine,
};

use super::error::SttError;
use super::types::EngineType;

/// Unified handle for any loaded speech-to-text engine.
///
/// Lot 2: Whisper only. SenseVoice / Moonshine / MistralApi variants are
/// added in lots 4 and 5 once their features land in `Cargo.toml`.
pub enum LoadedEngine {
    Whisper(WhisperEngine),
}

impl LoadedEngine {
    pub fn engine_type(&self) -> EngineType {
        match self {
            Self::Whisper(_) => EngineType::Whisper,
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Whisper(_) => "transcribe-rs/whisper",
        }
    }
}

/// Suppress whisper.cpp C library verbose output (beam search, model loading, etc.).
///
/// Upstream transcribe-rs 0.2 does not re-export the whisper-rs log hook, so this
/// is currently a no-op. Left as a named entry point so callers don't need to
/// care, and so we can restore behavior in a later lot if needed.
pub fn suppress_backend_logs() {
    // no-op for now; transcribe-rs swallows the whisper.cpp stderr in release
}

/// Load an engine from disk.
pub fn load(engine_type: EngineType, model_path: &Path) -> Result<LoadedEngine, SttError> {
    if !model_path.exists() {
        return Err(SttError::ModelNotFound(model_path.display().to_string()));
    }

    match engine_type {
        EngineType::Whisper => {
            let mut engine = WhisperEngine::new();
            engine
                .load_model(model_path)
                .map_err(|e| SttError::LoadError(format!("whisper: {e}")))?;
            Ok(LoadedEngine::Whisper(engine))
        }
        EngineType::SenseVoice | EngineType::Moonshine | EngineType::MistralApi => {
            Err(SttError::BackendUnavailable(format!(
                "backend {} not enabled yet (added in a later lot)",
                engine_type.as_str()
            )))
        }
    }
}

/// Run transcription on 16 kHz mono f32 PCM samples.
pub fn transcribe(
    engine: &mut LoadedEngine,
    audio: Vec<f32>,
    language: &str,
    translate: bool,
    initial_prompt: Option<&str>,
) -> Result<String, SttError> {
    if audio.is_empty() {
        return Err(SttError::InvalidAudio("empty audio buffer".to_string()));
    }

    match engine {
        LoadedEngine::Whisper(whisper) => {
            let lang = if language.eq_ignore_ascii_case("auto") {
                None
            } else {
                Some(language.to_string())
            };
            let params = WhisperInferenceParams {
                language: lang,
                translate,
                initial_prompt: initial_prompt.map(str::to_string),
                ..Default::default()
            };
            let result = whisper
                .transcribe_samples(audio, Some(params))
                .map_err(|e| SttError::TranscribeError(format!("whisper: {e}")))?;
            Ok(result.text)
        }
    }
}

/// Release the underlying model from memory. Idempotent.
pub fn unload(engine: &mut LoadedEngine) {
    match engine {
        LoadedEngine::Whisper(e) => e.unload_model(),
    }
}
