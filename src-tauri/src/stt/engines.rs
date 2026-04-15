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
        EngineType::SenseVoice | EngineType::Moonshine => {
            Err(SttError::BackendUnavailable(format!(
                "backend {} not enabled yet (ONNX feature pending)",
                engine_type.as_str()
            )))
        }
        EngineType::Voxtral => Err(SttError::BackendUnavailable(
            "backend voxtral not compiled in — rebuild with `--features voxtral`".to_string(),
        )),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_model_not_found() {
        let bogus = std::path::PathBuf::from("/tmp/this-does-not-exist-__stt__");
        match load(EngineType::Whisper, &bogus) {
            Err(SttError::ModelNotFound(msg)) => {
                assert!(msg.contains("this-does-not-exist"))
            }
            Err(other) => panic!("expected ModelNotFound, got {other:?}"),
            Ok(_) => panic!("expected error, got ok"),
        }
    }

    #[test]
    fn unavailable_backends_report_clearly() {
        // Use a path that exists so the ModelNotFound short-circuit doesn't fire.
        let existing = std::env::temp_dir();
        for engine in [
            EngineType::SenseVoice,
            EngineType::Moonshine,
            EngineType::Voxtral,
        ] {
            match load(engine, &existing) {
                Err(SttError::BackendUnavailable(_)) => {}
                Err(other) => panic!("expected BackendUnavailable, got {other:?}"),
                Ok(_) => panic!("expected error, got ok"),
            }
        }
    }
}
