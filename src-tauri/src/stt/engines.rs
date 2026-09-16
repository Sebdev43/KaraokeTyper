use std::path::Path;

use transcribe_rs::{
    onnx::{
        parakeet::{ParakeetModel, ParakeetParams},
        Quantization,
    },
    whisper_cpp::{WhisperEngine, WhisperInferenceParams, WhisperLoadParams},
    GPU_DEVICE_AUTO,
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
    Parakeet(ParakeetModel),
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

/// How Whisper should use the GPU; the ONNX engines run on the CPU provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuChoice {
    pub use_gpu: bool,
    /// Backend device index from the config; `None` lets whisper.cpp pick the
    /// best one (dedicated over integrated, then most VRAM).
    pub device_index: Option<u32>,
}

/// Load an engine from disk.
pub fn load(
    engine_type: EngineType,
    model_path: &Path,
    gpu: GpuChoice,
) -> Result<LoadedEngine, SttError> {
    if !model_path.exists() {
        return Err(SttError::ModelNotFound(model_path.display().to_string()));
    }

    match engine_type {
        EngineType::Whisper => {
            // Every field spelled out: the crate's `Default` turns flash attention
            // on, which 1.7.0 never did, and a silent numeric change is not what a
            // runtime upgrade should bring. Revisit as its own change.
            let params = WhisperLoadParams {
                use_gpu: gpu.use_gpu,
                flash_attn: false,
                gpu_device: gpu
                    .device_index
                    .and_then(|i| i32::try_from(i).ok())
                    .unwrap_or(GPU_DEVICE_AUTO),
            };
            let engine = WhisperEngine::load_with_params(model_path, params)
                .map_err(|e| SttError::LoadError(format!("whisper: {e}")))?;
            Ok(LoadedEngine::Whisper(engine))
        }
        EngineType::Parakeet => {
            // Parakeet expects the **directory** of ONNX files (e.g.
            // `.../models/parakeet-tdt-0.6b-v3-int8/`), not a single file; the
            // int8 variant is what the catalogue ships. `model_downloader`
            // extracts it before calling us.
            let engine = ParakeetModel::load(model_path, &Quantization::Int8)
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
    audio: &[f32],
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
                .transcribe_with(audio, &params)
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
            // `transcribe_with` prepends the 250 ms of silence the NeMo model expects.
            let result = parakeet
                .transcribe_with(audio, &ParakeetParams::default())
                .map_err(|e| SttError::TranscribeError(format!("parakeet: {e}")))?;
            Ok(result.text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CPU: GpuChoice = GpuChoice {
        use_gpu: false,
        device_index: None,
    };

    #[test]
    fn load_missing_file_returns_model_not_found() {
        let bogus = std::path::PathBuf::from("/tmp/this-does-not-exist-__stt__");
        match load(EngineType::Whisper, &bogus, CPU) {
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
            match load(engine, &existing, CPU) {
                Err(SttError::BackendUnavailable(_)) => {}
                Err(other) => panic!("expected BackendUnavailable, got {other:?}"),
                Ok(_) => panic!("expected error, got ok"),
            }
        }
    }
}
