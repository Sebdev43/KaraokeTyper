//! Engine lifecycle: the loaded-engine wrapper, the two engine slots (the user's
//! primary engine and the Parakeet fast lane) and the blocking transcription that
//! runs on them. Everything here is Tauri-agnostic; `lib.rs` only wires it to
//! commands and events.
#![warn(dead_code, unused_imports)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::config::AppConfig;
use crate::model_downloader;
use crate::stt::{self, EngineType, LoadedEngine, SttLane};

/// Samples per second of every capture buffer that reaches the engines.
pub const CAPTURE_SAMPLE_RATE_HZ: usize = 16_000;

/// Seconds of audio in a capture buffer.
#[must_use]
pub fn capture_secs(samples: usize) -> f32 {
    samples as f32 / CAPTURE_SAMPLE_RATE_HZ as f32
}

/// STT engine wrapper — dispatches over any backend supported by `stt::engines`.
pub struct SttEngine {
    engine: LoadedEngine,
    language: String,
}

impl SttEngine {
    /// The engine described by the config (`stt_backend` + `model` / `model_path`).
    pub fn new(config: &AppConfig) -> Result<Self, String> {
        Self::for_lane(SttLane::Primary, config)
    }

    /// The engine a lane runs on: the configured one, or Parakeet for the fast lane,
    /// resolved through the catalogue so it stays independent of `config.model`
    /// (which names the primary Whisper model).
    pub fn for_lane(lane: SttLane, config: &AppConfig) -> Result<Self, String> {
        let (engine_type, model_path) = match lane {
            SttLane::Primary => (config.stt_backend, primary_model_path(config)?),
            SttLane::FastParakeet => (
                EngineType::Parakeet,
                model_downloader::installed_model_path_for(EngineType::Parakeet).ok_or_else(
                    || {
                        format!(
                            "{} is not installed. Download it from Settings → STT Models.",
                            EngineType::Parakeet.display_name()
                        )
                    },
                )?,
            ),
        };
        // The GPU toggle and the auto-disable after repeated load failures both
        // apply here, so what Settings reports ("GPU auto-disabled") is what runs.
        let gpu = stt::GpuChoice {
            use_gpu: config.use_gpu && !stt::gpu_health::is_gpu_disabled_by_failures(),
            device_index: config.gpu_device_index,
        };
        let engine = load_engine(engine_type, &model_path, gpu)?;
        Ok(Self {
            engine,
            language: config.language.clone(),
        })
    }

    pub fn transcribe(
        &mut self,
        audio_data: &[f32],
        initial_prompt: Option<&str>,
    ) -> Result<String, String> {
        stt::transcribe(
            &mut self.engine,
            audio_data,
            &self.language,
            false,
            initial_prompt,
        )
        .map_err(|e| e.to_string())
    }

    pub fn backend_name(&self) -> &'static str {
        self.engine.backend_name()
    }
}

/// `config.model_path` when it names an existing local file, else the catalogue
/// path of `config.model`.
fn primary_model_path(config: &AppConfig) -> Result<PathBuf, String> {
    config
        .model_path
        .as_deref()
        .filter(|p| {
            if p.starts_with("http://") || p.starts_with("https://") {
                crate::dev_log!("[STT] Ignoring URL in model_path: {}", p);
                return false;
            }
            let exists = Path::new(p).exists();
            if !exists {
                crate::dev_log!(
                    "[STT] model_path doesn't exist, will resolve from model name: {}",
                    p
                );
            }
            exists
        })
        .map(PathBuf::from)
        .or_else(|| model_downloader::get_model_path(&config.model).map(PathBuf::from))
        .ok_or_else(|| {
            "No model downloaded. Download a model from Settings → STT Models.".to_string()
        })
}

fn load_engine(
    engine_type: EngineType,
    model_path: &Path,
    gpu: stt::GpuChoice,
) -> Result<LoadedEngine, String> {
    let result = stt::load(engine_type, model_path, gpu);
    // GPU health only tracks Whisper: it is the backend the GPU feature accelerates,
    // and a Parakeet (ONNX, CPU) failure must not trip the GPU auto-disable. Nor
    // must a missing model file: only the backend refusing to load counts.
    if engine_type == EngineType::Whisper {
        let counts_for_gpu_health = !matches!(
            result,
            Err(stt::SttError::ModelNotFound(_) | stt::SttError::BackendUnavailable(_))
        );
        if counts_for_gpu_health {
            stt::gpu_health::note_engine_load(gpu.use_gpu, result.is_ok());
        }
    }
    result.map_err(|e| e.to_string())
}

pub type EngineSlot = Arc<Mutex<Option<SttEngine>>>;

/// Serialises inference across both slots: overlapping transcriptions would each
/// see its slot empty (the running one took the engine out) and load a second
/// model. The fast-lane warm-up takes it too, so it never loads next to an
/// inference that is about to put its engine back.
static INFERENCE_GUARD: Mutex<()> = Mutex::new(());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The two engine slots: the user's primary engine and the Parakeet fast lane.
///
/// Both are kept resident while hybrid mode is active so a short dictation never
/// waits for a model swap — reloading a multi-GB Whisper on every alternation is
/// exactly the latency the fast lane exists to remove.
#[derive(Clone, Default)]
pub struct EngineSlots {
    primary: EngineSlot,
    fast: EngineSlot,
}

impl EngineSlots {
    fn slot(&self, lane: SttLane) -> &EngineSlot {
        match lane {
            SttLane::Primary => &self.primary,
            SttLane::FastParakeet => &self.fast,
        }
    }

    pub fn install_primary(&self, engine: SttEngine) {
        *lock(&self.primary) = Some(engine);
    }

    /// Swap the primary engine for the one `config` describes, **releasing the old
    /// one first**: two resident Whisper large models do not fit an 8 GB GPU, and
    /// the Vulkan allocation failure is a C++ exception that aborts the process
    /// rather than an error we could recover from. Waits for an in-flight
    /// inference to hand its engine back so that one is released too. On failure
    /// the slot stays empty and the next dictation retries the load lazily.
    ///
    /// Blocking (holds the slot for the whole load): call from `spawn_blocking`.
    pub fn replace_primary(&self, config: &AppConfig) -> Result<(), String> {
        let _inference = lock(&INFERENCE_GUARD);
        let mut guard = lock(&self.primary);
        if guard.take().is_some() {
            crate::dev_log!("[STT] Previous engine released before reload");
        }
        *guard = Some(SttEngine::new(config)?);
        Ok(())
    }

    /// Drop every loaded engine. True when something was actually unloaded.
    pub fn unload_all(&self) -> bool {
        let primary = lock(&self.primary).take().is_some();
        let fast = lock(&self.fast).take().is_some();
        primary || fast
    }

    /// Route a recording of `duration_secs` (raw capture, before padding) between
    /// the primary engine and the Parakeet fast lane.
    ///
    /// Only the 12-byte policy is read under the config lock; the install check
    /// (a catalogue walk with one stat per entry until Parakeet is found) runs
    /// outside it, and not at all while Parakeet is already resident.
    pub fn lane_for_recording(&self, config: &Mutex<AppConfig>, duration_secs: f32) -> SttLane {
        let policy = lock(config).hybrid_policy();
        if !policy.is_active() {
            return SttLane::Primary;
        }
        let parakeet_installed = lock(&self.fast).is_some()
            || model_downloader::installed_model_path_for(EngineType::Parakeet).is_some();
        let lane = policy.lane_for(duration_secs, parakeet_installed);
        crate::dev_log!(
            "[STT] Hybrid: {:.1}s vs {}s threshold, parakeet_installed={} → {:?}",
            duration_secs,
            policy.threshold_secs,
            parakeet_installed,
            lane
        );
        lane
    }

    /// Bring the fast lane in line with the config: release Parakeet when hybrid
    /// mode is inactive, warm it up off-thread when it is active and the slot is
    /// empty. Best effort — a Parakeet that cannot load is logged, never surfaced;
    /// the lane falls back to the primary engine at dictation time anyway.
    pub fn sync_fast_lane(&self, config: &Arc<Mutex<AppConfig>>) {
        let snapshot = lock(config).clone();
        if !snapshot.hybrid_policy().is_active() {
            if lock(&self.fast).take().is_some() {
                crate::dev_log!("[STT] Hybrid fast lane released (mode inactive)");
            }
            return;
        }
        let slots = self.clone();
        let config = Arc::clone(config);
        tokio::task::spawn_blocking(move || slots.warm_up_fast_lane(&config, &snapshot));
    }

    fn warm_up_fast_lane(&self, config: &Mutex<AppConfig>, snapshot: &AppConfig) {
        // An inference in flight has taken its engine out of the slot and will put
        // it back when it finishes; loading now would only duplicate it.
        let Ok(_inference) = INFERENCE_GUARD.try_lock() else {
            crate::dev_log!("[STT] Hybrid warm-up skipped: inference in flight");
            return;
        };
        let mut guard = lock(&self.fast);
        if guard.is_some() {
            return;
        }
        match SttEngine::for_lane(SttLane::FastParakeet, snapshot) {
            // The toggle may have flipped during the load: keep the engine only if
            // the mode is still active, or the memory would never be given back.
            Ok(engine) if lock(config).hybrid_policy().is_active() => {
                *guard = Some(engine);
                crate::dev_log!(
                    "[STT] ✅ Hybrid fast lane ready ({})",
                    EngineType::Parakeet.as_str()
                );
            }
            Ok(_) => {
                crate::dev_log!("[STT] Hybrid fast lane dropped: mode deactivated during warm-up")
            }
            Err(e) => crate::dev_log!("[STT] Hybrid fast lane not warmed up: {}", e),
        }
    }

    /// Run a blocking transcription with panic isolation and without holding the
    /// engine lock for the (multi-second) inference.
    ///
    /// 1. Under a short lock, lazily reload the lane's engine if the idle watcher
    ///    unloaded it, then *take* it out of the shared `Option` so the inference
    ///    below runs lock-free (the idle watcher sees `None` and skips unloading; a
    ///    concurrent settings reload can install a fresh engine). A fast lane that
    ///    cannot load falls back to the primary engine rather than losing the dictation.
    /// 2. Transcribe inside `catch_unwind` so a panic in the C/ONNX backend cannot
    ///    poison the mutex or leave a corrupt engine in place.
    /// 3. On success, return the engine to its slot (see `put_back`). On panic, drop
    ///    it so the next call reloads.
    ///
    /// Must be called from a blocking context (e.g. `spawn_blocking`).
    pub fn transcribe_blocking(
        &self,
        config: &Mutex<AppConfig>,
        lane: SttLane,
        padded_audio: Vec<f32>,
        prompt_for_whisper: Option<String>,
    ) -> Result<String, String> {
        let _inference = lock(&INFERENCE_GUARD);

        let (lane, mut engine) = match self.take_or_load(lane, config) {
            Ok(engine) => (lane, engine),
            Err(e) if lane == SttLane::FastParakeet => {
                crate::dev_log!(
                    "[STT] Fast lane unavailable ({}) — falling back to the primary engine",
                    e
                );
                (
                    SttLane::Primary,
                    self.take_or_load(SttLane::Primary, config)?,
                )
            }
            Err(e) => return Err(e),
        };

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine.transcribe(&padded_audio, prompt_for_whisper.as_deref())
        }));

        match outcome {
            Ok(result) => {
                self.put_back(lane, engine, config);
                result
            }
            Err(_) => {
                crate::dev_log!(
                    "[STT] ❌ Transcription panicked — engine reset, will reload next time"
                );
                Err("Transcription failed (engine crashed). It has been reset and will reload on the next attempt.".to_string())
            }
        }
    }

    /// Take the lane's engine out of its slot, loading it first if the slot is empty.
    fn take_or_load(&self, lane: SttLane, config: &Mutex<AppConfig>) -> Result<SttEngine, String> {
        let mut guard = lock(self.slot(lane));
        if guard.is_none() {
            crate::dev_log!("[STT] {:?} slot empty, loading model...", lane);
            let snapshot = lock(config).clone();
            *guard = Some(
                SttEngine::for_lane(lane, &snapshot)
                    .map_err(|e| format!("Failed to reload model: {}", e))?,
            );
            crate::dev_log!("[STT] ✅ Model reloaded successfully");
        }
        guard
            .take()
            .ok_or_else(|| "STT engine not initialized".to_string())
    }

    /// Return an engine to its slot **only if it is still empty** — a newer engine
    /// installed meanwhile (model change) wins — and, for the fast lane, only if
    /// hybrid mode is still active: "empty" can also mean "released by a settings
    /// save while we were transcribing", and putting Parakeet back would leak it.
    fn put_back(&self, lane: SttLane, engine: SttEngine, config: &Mutex<AppConfig>) {
        if lane == SttLane::FastParakeet && !lock(config).hybrid_policy().is_active() {
            crate::dev_log!("[STT] Hybrid fast lane dropped after inference: mode deactivated");
            return;
        }
        let mut guard = lock(self.slot(lane));
        if guard.is_none() {
            *guard = Some(engine);
        }
    }
}
