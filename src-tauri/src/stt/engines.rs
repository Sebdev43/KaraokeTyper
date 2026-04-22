use std::path::Path;

use transcribe_rs::{
    engines::{
        parakeet::{ParakeetEngine, ParakeetInferenceParams, ParakeetModelParams},
        whisper::{WhisperEngine, WhisperInferenceParams},
    },
    TranscriptionEngine,
};

use super::error::SttError;
use super::types::EngineType;

/// Unified handle for any loaded speech-to-text engine.
///
/// Currently shipped: Whisper + Parakeet V3. SenseVoice / Moonshine variants
/// are kept as placeholders — they need their own Cargo features and have no
/// French support so they're de-prioritized.
pub enum LoadedEngine {
    Whisper(WhisperEngine),
    Parakeet(ParakeetEngine),
}

impl LoadedEngine {
    pub fn engine_type(&self) -> EngineType {
        match self {
            Self::Whisper(_) => EngineType::Whisper,
            Self::Parakeet(_) => EngineType::Parakeet,
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self {
            Self::Whisper(_) => "transcribe-rs/whisper",
            Self::Parakeet(_) => "transcribe-rs/parakeet",
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
        EngineType::Parakeet => {
            // Parakeet expects the **directory** extracted from the .tar.gz
            // (e.g. `.../models/parakeet-tdt-0.6b-v3-int8/`), not a single
            // file. `model_downloader` extracts automatically before calling us.
            let mut engine = ParakeetEngine::new();
            engine
                .load_model_with_params(model_path, ParakeetModelParams::int8())
                .map_err(|e| SttError::LoadError(format!("parakeet: {e}")))?;
            Ok(LoadedEngine::Parakeet(engine))
        }
        EngineType::SenseVoice | EngineType::Moonshine => {
            Err(SttError::BackendUnavailable(format!(
                "backend {} not enabled yet (ONNX feature pending)",
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
        LoadedEngine::Parakeet(parakeet) => {
            // Parakeet V3 auto-detects language from its 27-language training
            // set — no language/translate/initial_prompt hooks. We explicitly
            // ignore those parameters to keep the engines interchangeable,
            // but warn once per call when the caller asked for something we
            // can't honour so the UI decision stays auditable.
            if translate {
                crate::dev_log!(
                    "[STT/Parakeet] 'translate' ignored — Parakeet V3 doesn't support translation."
                );
            }
            if let Some(prompt) = initial_prompt {
                if !prompt.is_empty() {
                    crate::dev_log!(
                        "[STT/Parakeet] initial_prompt ignored — Parakeet has no prompting."
                    );
                }
            }
            if !language.is_empty() && !language.eq_ignore_ascii_case("auto") {
                crate::dev_log!(
                    "[STT/Parakeet] language hint '{}' ignored — model auto-detects.",
                    language
                );
            }
            let params = ParakeetInferenceParams::default();
            let result = parakeet
                .transcribe_samples(audio, Some(params))
                .map_err(|e| SttError::TranscribeError(format!("parakeet: {e}")))?;
            Ok(result.text)
        }
    }
}

/// Release the underlying model from memory. Idempotent.
pub fn unload(engine: &mut LoadedEngine) {
    match engine {
        LoadedEngine::Whisper(e) => e.unload_model(),
        LoadedEngine::Parakeet(e) => e.unload_model(),
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
        for engine in [EngineType::SenseVoice, EngineType::Moonshine] {
            match load(engine, &existing) {
                Err(SttError::BackendUnavailable(_)) => {}
                Err(other) => panic!("expected BackendUnavailable, got {other:?}"),
                Ok(_) => panic!("expected error, got ok"),
            }
        }
    }
}
