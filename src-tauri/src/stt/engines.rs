use std::path::Path;

use transcribe_cpp::{
    Backend, Model, ModelOptions, RunExtension, RunOptions, Session, WhisperRunOptions,
};
use transcribe_rs::onnx::{
    parakeet::{ParakeetModel, ParakeetParams},
    Quantization,
};

use super::error::SttError;
use super::types::EngineType;

/// Unified handle for any loaded speech-to-text engine.
///
/// Currently shipped: Whisper + Parakeet V3. SenseVoice / Moonshine variants
/// are kept as placeholders — they need their own Cargo features and have no
/// French support so they're de-prioritized.
pub enum LoadedEngine {
    Whisper(Session),
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
            Self::Whisper(_) => "transcribe-cpp/whisper",
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
            if model_path
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("gguf"))
            {
                return Err(SttError::UnsupportedFormat(format!(
                    "{} is not a GGUF model. The Whisper engine now reads GGUF; \
                     download the model again from Settings → Models (the new file \
                     is smaller) — the old one is left on disk for you to remove.",
                    model_path.display()
                )));
            }
            // Named backend rather than `Auto`: `Auto` falls back to the CPU
            // without telling anyone, which makes every load look like a success,
            // leaves the GPU auto-disable in `gpu_health` dead, and lets Settings
            // keep claiming the GPU is in use. `CpuAccel`, not `Cpu`: the user
            // turned the GPU off, not BLAS/AMX.
            let backend = match (gpu.use_gpu, cfg!(feature = "gpu-metal")) {
                (false, _) => Backend::CpuAccel,
                (true, true) => Backend::Metal,
                (true, false) => Backend::Vulkan,
            };
            let options = ModelOptions {
                backend,
                device: None,
            };
            if gpu.use_gpu && gpu.device_index.is_some() {
                crate::dev_log!(
                    "[STT/Whisper] GPU device index ignored: the backend picks the device"
                );
            }
            let session = Model::load_with(model_path, &options)
                .map_err(|e| SttError::LoadError(format!("whisper: {e}")))?
                .session()
                .map_err(|e| SttError::LoadError(format!("whisper session: {e}")))?;
            Ok(LoadedEngine::Whisper(session))
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

/// Decode settings for one Whisper run.
///
/// Spelled out rather than left to the defaults, because two of them are the
/// difference between a transcript and an invented one:
///
/// - `temperature_inc` arms whisper.cpp's fallback. At temperature 0 a window the
///   model is unsure about is kept as-is; with the fallback it is decoded again,
///   hotter, until it passes the thresholds. Without it the uncertain words come
///   back confidently wrong — which is what "it replaces my words" looks like.
/// - `no_speech_thold` stays at whisper.cpp's own 0.6. Below it, a window whose
///   no-speech probability creeps up is dropped whole, taking real speech with it.
///
/// `condition_on_prev_tokens` is off: each window decodes on its own, so one bad
/// window cannot drag the rest of the dictation with it.
fn whisper_run_options(
    language: &str,
    translate: bool,
    initial_prompt: Option<&str>,
) -> RunOptions {
    RunOptions {
        task: if translate {
            transcribe_cpp::Task::Translate
        } else {
            transcribe_cpp::Task::Transcribe
        },
        language: (!language.eq_ignore_ascii_case("auto")).then(|| language.to_string()),
        family: Some(RunExtension::Whisper(WhisperRunOptions {
            initial_prompt: initial_prompt.map(str::to_string),
            temperature: Some(0.0),
            temperature_inc: Some(0.2),
            no_speech_thold: Some(0.6),
            condition_on_prev_tokens: Some(false),
            ..Default::default()
        })),
        ..Default::default()
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
        LoadedEngine::Whisper(session) => {
            let options = whisper_run_options(language, translate, initial_prompt);
            match session.run(audio, &options) {
                Ok(result) => Ok(result.text),
                // A truncated, repeating or aborted run still carries everything
                // decoded before it stopped. Dropping it throws away a minute of
                // speech because one window went wrong near the end.
                Err(e) => match e.partial() {
                    Some(partial) if !partial.text.trim().is_empty() => {
                        crate::dev_log!(
                            "[STT/Whisper] incomplete run ({}) — keeping the {} characters \
                             decoded before it stopped",
                            e,
                            partial.text.len()
                        );
                        Ok(partial.text.clone())
                    }
                    _ => Err(SttError::TranscribeError(format!("whisper: {e}"))),
                },
            }
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
    fn whisper_runs_with_the_temperature_fallback_armed() {
        let opts = whisper_run_options("fr", false, Some("Vocabulaire: Tauri"));
        let Some(RunExtension::Whisper(w)) = opts.family else {
            panic!("the Whisper run extension must be set, or every knob below is a default");
        };
        assert_eq!(
            w.temperature_inc,
            Some(0.2),
            "without the fallback an uncertain window is kept as decoded — the words \
             come back confidently wrong"
        );
        assert_eq!(
            w.temperature,
            Some(0.0),
            "the fallback must start from greedy"
        );
        assert_eq!(
            w.no_speech_thold,
            Some(0.6),
            "below whisper.cpp's own threshold a window of real speech is dropped whole"
        );
        assert_eq!(w.condition_on_prev_tokens, Some(false));
        assert_eq!(w.initial_prompt.as_deref(), Some("Vocabulaire: Tauri"));
        assert_eq!(opts.language.as_deref(), Some("fr"));
        assert_eq!(opts.task, transcribe_cpp::Task::Transcribe);
    }

    #[test]
    fn auto_language_is_sent_as_no_hint_and_translate_switches_the_task() {
        assert_eq!(whisper_run_options("auto", false, None).language, None);
        assert_eq!(whisper_run_options("AUTO", false, None).language, None);
        assert_eq!(
            whisper_run_options("fr", true, None).task,
            transcribe_cpp::Task::Translate
        );
    }

    #[test]
    fn a_ggml_model_is_refused_with_an_actionable_message() {
        let dir = std::env::temp_dir().join("kt-gguf-check");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let legacy = dir.join("ggml-large-v3.bin");
        std::fs::write(&legacy, b"not a gguf").expect("write");

        let msg = match load(EngineType::Whisper, &legacy, CPU) {
            // The variant matters as much as the text: `LoadError` would be counted
            // as a backend failure and auto-disable the GPU over a file format.
            Err(e @ SttError::UnsupportedFormat(_)) => e.to_string(),
            Err(other) => panic!("wrong variant for a format mismatch: {other}"),
            Ok(_) => panic!("a .bin must be refused by the GGUF engine"),
        };
        assert!(msg.contains("GGUF"), "{msg}");
        assert!(
            msg.contains("download the model again"),
            "the message must say what to do: {msg}"
        );

        // Same refusal whatever the case of the extension.
        let shouty = dir.join("MODEL.BIN");
        std::fs::write(&shouty, b"not a gguf").expect("write");
        assert!(
            matches!(
                load(EngineType::Whisper, &shouty, CPU),
                Err(SttError::UnsupportedFormat(_))
            ),
            "the extension check must not depend on case"
        );

        std::fs::remove_file(&legacy).ok();
        std::fs::remove_file(&shouty).ok();
    }

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
