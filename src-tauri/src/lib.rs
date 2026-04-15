// Burn + wgpu transitive types are deep enough that rustc's default recursion
// budget trips when evaluating Sync/Send bounds on LoadedEngine::Voxtral.
// Bumping this header-only limit is the standard workaround.
#![recursion_limit = "512"]

#[macro_use]
mod logging;

mod app_data;
mod audio;
mod audio_devices;
mod audio_feedback;
mod autostart;
mod config;
mod gnome_extension;
mod gpu;
mod keyboard_listener;
mod model_downloader;
mod ollama_client;
mod output;
mod stt;
mod text_processing;
mod vad;

// Linux-specific modules
#[cfg(target_os = "linux")]
mod evdev_listener;

// CPAL audio capture (cross-platform: ALSA on Linux, CoreAudio on macOS, WASAPI on Windows)
// Key design: stream opened once on first PTT, kept paused between recordings for instant response

use audio::AudioCapture;
use config::AppConfig;
use output::TextOutput;
use serde::Serialize;
use specta::Type;
use std::sync::{Arc, Mutex};

// ============================================================================
// Types for tauri-specta typed commands
// ============================================================================

/// System information
#[derive(Clone, Serialize, Type)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub display_server: String,
    pub desktop_environment: String,
}

/// GPU acceleration status
#[derive(Clone, Serialize, Type)]
pub struct GpuInfo {
    pub available: bool,
    pub backend: String,
    pub device_name: Option<String>,
    /// Number of consecutive GPU initialization failures
    pub failure_count: u32,
    /// Whether GPU is auto-disabled due to too many failures
    pub disabled_by_failures: bool,
    /// Detected VRAM in MB (None if detection failed)
    pub vram_mb: Option<u32>,
    /// Recommended model name based on VRAM
    pub recommended_model: Option<String>,
}

/// GPU status for real-time monitoring and events
#[derive(Clone, Serialize, Type)]
pub struct GpuStatus {
    /// Current mode: "gpu" or "cpu"
    pub mode: String,
    /// GPU device name if using GPU
    pub device_name: Option<String>,
    /// Reason for CPU fallback if applicable
    pub fallback_reason: Option<String>,
    /// Current failure count
    pub failure_count: u32,
}

use stt::LoadedEngine;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// STT engine wrapper — dispatches over any backend supported by `stt::engines`.
pub struct SttEngine {
    engine: LoadedEngine,
    language: String,
}

impl SttEngine {
    pub fn new(config: &AppConfig) -> Result<Self, String> {
        // Use model_path if provided AND it's a valid local file, otherwise resolve from model name
        let resolved_path = config
            .model_path
            .clone()
            .filter(|p| {
                if p.starts_with("http://") || p.starts_with("https://") {
                    dev_log!("[STT] Ignoring URL in model_path: {}", p);
                    return false;
                }
                let exists = std::path::Path::new(p).exists();
                if !exists {
                    dev_log!(
                        "[STT] model_path doesn't exist, will resolve from model name: {}",
                        p
                    );
                }
                exists
            })
            .or_else(|| model_downloader::get_model_path(&config.model));

        let model_path = resolved_path
            .ok_or("No model downloaded. Download a model from Settings → STT Models.")?;

        let engine = stt::load(config.stt_backend, std::path::Path::new(&model_path))
            .map_err(|e| e.to_string())?;

        Ok(SttEngine {
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
            audio_data.to_vec(),
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

// Worker-thread audio capture: stream opened in background thread, commands via mpsc channel.
// Eliminates PTT race conditions — start/stop are queued and processed after stream is ready.
pub(crate) struct AppState {
    audio_capture: Arc<Mutex<AudioCapture>>,
    stt_engine: Arc<Mutex<Option<SttEngine>>>,
    config: Arc<Mutex<AppConfig>>,
    /// Timestamp of the hotkey press — used for [TIMING] logs
    pub recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
    last_stt_activity: Arc<std::sync::atomic::AtomicU64>,
    idle_watcher_shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_capture: Arc::new(Mutex::new(AudioCapture::new(16000))),
            stt_engine: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(AppConfig::load().unwrap_or_default())),
            recording_start_time: Arc::new(Mutex::new(None)),
            last_stt_activity: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            idle_watcher_shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

fn start_idle_watcher(state: &AppState) {
    use std::sync::atomic::Ordering;

    let engine = state.stt_engine.clone();
    let config = state.config.clone();
    let last_activity = state.last_stt_activity.clone();
    let shutdown = state.idle_watcher_shutdown.clone();

    std::thread::spawn(move || {
        dev_log!("[IDLE_WATCHER] Started");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));

            if shutdown.load(Ordering::Relaxed) {
                dev_log!("[IDLE_WATCHER] Shutdown signal received");
                break;
            }

            let timeout_secs = {
                let cfg = config.lock().unwrap();
                cfg.model_unload_timeout.to_seconds()
            };

            let timeout_secs = match timeout_secs {
                Some(s) => s,
                None => continue, // Never timeout
            };

            let last = last_activity.load(Ordering::Relaxed);
            if last == 0 {
                continue; // No activity yet
            }

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            if now - last >= timeout_secs {
                let mut guard = engine.lock().unwrap();
                if guard.is_some() {
                    *guard = None;
                    dev_log!("[IDLE_WATCHER] Model unloaded after {}s idle", timeout_secs);
                }
            }
        }
    });
}

fn update_stt_activity(state: &AppState) {
    use std::sync::atomic::Ordering;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    state.last_stt_activity.store(now, Ordering::Relaxed);
}

/// Get D-Bus environment variables needed for PulseAudio/PipeWire/GNOME commands
/// when launched from .desktop file (where these might not be inherited)
#[cfg(target_os = "linux")]
pub(crate) fn get_dbus_env() -> (String, String) {
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    let dbus_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={}/bus", xdg_runtime));
    (xdg_runtime, dbus_addr)
}

/// Debug helper to save audio data as WAV file
fn save_debug_wav(audio_data: &[f32], path: &std::path::PathBuf) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer =
        hound::WavWriter::create(path, spec).map_err(|e| format!("Failed to create WAV: {}", e))?;

    for &sample in audio_data {
        let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        writer
            .write_sample(sample_i16)
            .map_err(|e| format!("Failed to write sample: {}", e))?;
    }

    writer
        .finalize()
        .map_err(|e| format!("Failed to finalize WAV: {}", e))?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn init_stt(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Suppress whisper.cpp C library verbose output (beam search, model loading, etc.)
    stt::suppress_backend_logs();

    let config = {
        let config = state.config.lock().unwrap();
        config.clone()
    };

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "initializing".to_string(),
                message: "Loading Whisper model...".to_string(),
            },
        )
        .ok();

    dev_log!(
        "[STT] Loading whisper-rs model: {} (language: {})",
        config.model,
        config.language
    );

    let engine = SttEngine::new(&config).map_err(|e| {
        let error_msg = e.to_string();

        if error_msg.contains("No model downloaded") || error_msg.contains("Model not found") {
            "No model downloaded. Download a model from Settings → STT Models.".to_string()
        } else if error_msg.contains("Failed to load model") {
            format!("Failed to load Whisper model: {}", error_msg)
        } else {
            error_msg
        }
    })?;

    dev_log!("[STT] ✓ {} engine initialized", engine.backend_name());
    *state.stt_engine.lock().unwrap_or_else(|e| e.into_inner()) = Some(engine);

    // Initialize Voice Activity Detection (Silero VAD v4)
    {
        let vad_model_path = app_handle
            .path()
            .resolve(
                "models/silero_vad_v4.onnx",
                tauri::path::BaseDirectory::Resource,
            )
            .map_err(|e| format!("Failed to resolve VAD model path: {}", e))?;

        if vad_model_path.exists() {
            match vad::SileroVad::new(&vad_model_path, 0.3) {
                Ok(silero) => {
                    let smoothed = vad::SmoothedVad::new(Box::new(silero), 15, 15, 2);
                    let capture = state
                        .audio_capture
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    capture.set_vad(smoothed);
                    dev_log!("[VAD] ✓ Silero VAD v4 initialized (threshold: 0.3)");
                }
                Err(e) => {
                    dev_log!("[VAD] ⚠️ Failed to init VAD: {} — recording without VAD", e);
                }
            }
        } else {
            dev_log!(
                "[VAD] ⚠️ VAD model not found at {:?} — recording without VAD",
                vad_model_path
            );
        }
    }

    // Pre-open audio worker now so first PTT press is instant.
    // The worker thread handles ALSA enumeration (~2s) in the background.
    {
        let mut capture = state
            .audio_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let device_name = {
            let config = state.config.lock().unwrap();
            config.audio_device_name.clone()
        };
        dev_log!("[AUDIO] Spawning audio worker (pre-open)...");
        match capture.open_stream(device_name.as_deref()) {
            Ok(()) => dev_log!("[AUDIO] ✅ Audio worker spawned — stream opening in background"),
            Err(e) => dev_log!(
                "[AUDIO] ⚠️ Pre-open failed: {} — will retry on first PTT",
                e
            ),
        }
    }

    // Start idle watcher for model unloading
    update_stt_activity(&state);
    start_idle_watcher(&state);

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "idle".to_string(),
                message: "Ready! Press Ctrl+Space to dictate".to_string(),
            },
        )
        .ok();

    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn check_ollama_status(
    state: State<'_, AppState>,
) -> Result<ollama_client::OllamaStatus, String> {
    let (url, model, enabled) = {
        let cfg = state.config.lock().unwrap();
        (
            cfg.ollama_url
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            cfg.ollama_model
                .clone()
                .unwrap_or_else(|| "mistral".to_string()),
            cfg.ollama_enabled,
        )
    };

    let available = if enabled {
        ollama_client::check_ollama_available(&url).await
    } else {
        false
    };

    Ok(ollama_client::OllamaStatus {
        available,
        model,
        url,
    })
}

#[tauri::command]
#[specta::specta]
async fn start_recording(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    do_start_recording(app_handle, &state).await
}

/// Helper function for start_recording - can be called from other modules.
///
/// Worker-thread architecture: `open_stream()` returns instantly (spawns a thread).
/// `start_recording()` sends a command that is queued and processed only after the
/// stream is ready. No race condition possible.
pub(crate) async fn do_start_recording(
    app_handle: AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), String> {
    let t0 = state
        .recording_start_time
        .lock()
        .unwrap()
        .unwrap_or_else(std::time::Instant::now);
    dev_log!(
        "[TIMING] T+{}ms: do_start_recording() entered",
        t0.elapsed().as_millis()
    );

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "recording".to_string(),
                message: "Recording... Release Ctrl+Space to stop".to_string(),
            },
        )
        .ok();

    // Open worker if not already running, then send Start command
    {
        let mut capture = state
            .audio_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        if !capture.is_stream_open() {
            let device_name = {
                let config = state.config.lock().unwrap();
                config.audio_device_name.clone()
            };
            dev_log!(
                "[TIMING] T+{}ms: Spawning audio worker...",
                t0.elapsed().as_millis()
            );
            capture.open_stream(device_name.as_deref())?;
            dev_log!(
                "[TIMING] T+{}ms: Worker spawned (stream opening in background)",
                t0.elapsed().as_millis()
            );
        } else {
            dev_log!(
                "[TIMING] T+{}ms: Worker already running (instant)",
                t0.elapsed().as_millis()
            );
        }

        // Send Start command — queued in channel, processed after stream is ready
        capture.start_recording()?;
    }

    // Play start sound (non-blocking, in separate thread)
    {
        let cfg = state.config.lock().unwrap();
        if cfg.audio_feedback_enabled {
            let vol = cfg.audio_feedback_volume;
            audio_feedback::play_start_sound(vol);
        }
    }

    dev_log!(
        "[TIMING] T+{}ms: Start command sent",
        t0.elapsed().as_millis()
    );
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn stop_recording(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    do_stop_recording(app_handle, &state).await
}

/// Helper function for stop_recording - can be called from other modules.
///
/// Worker-thread architecture: `stop_recording()` sends a `Cmd::Stop` to the worker,
/// which drains remaining samples and returns the buffer via a reply channel.
/// No race condition — the worker processes Start before Stop (channel ordering).
pub(crate) async fn do_stop_recording(
    app_handle: AppHandle,
    state: &State<'_, AppState>,
) -> Result<String, String> {
    // Get t0 from AppState for timing
    let t0 = state
        .recording_start_time
        .lock()
        .unwrap()
        .unwrap_or_else(std::time::Instant::now);
    dev_log!(
        "[TIMING] T+{}ms: do_stop_recording() entered",
        t0.elapsed().as_millis()
    );

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "processing".to_string(),
                message: "Processing speech...".to_string(),
            },
        )
        .ok();

    // Play stop sound (non-blocking, in separate thread)
    {
        let cfg = state.config.lock().unwrap();
        if cfg.audio_feedback_enabled {
            let vol = cfg.audio_feedback_volume;
            audio_feedback::play_stop_sound(vol);
        }
    }

    dev_log!(
        "[TIMING] T+{}ms: sending Stop to audio worker...",
        t0.elapsed().as_millis()
    );

    // Send Stop command (non-blocking), then drop the Mutex before waiting.
    // This prevents holding the lock during the blocking recv (up to 5s).
    let stop_handle = {
        let capture = state
            .audio_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        capture.stop_recording()?
    };
    // Mutex is released — now block waiting for the worker's reply
    let audio_data = stop_handle.wait()?;

    dev_log!(
        "[TIMING] T+{}ms: audio capture stopped, got {} samples ({:.2}s)",
        t0.elapsed().as_millis(),
        audio_data.len(),
        audio_data.len() as f32 / 16000.0
    );

    // DEBUG: Save audio to file for inspection (dev builds only)
    #[cfg(debug_assertions)]
    {
        let debug_path = std::env::temp_dir().join("karaoketyper_debug.wav");
        if let Err(e) = save_debug_wav(&audio_data, &debug_path) {
            dev_log!("[DEBUG] Failed to save debug WAV: {}", e);
        } else {
            dev_log!(
                "[DEBUG] Audio saved to: {} ({} samples, {:.2}s)",
                debug_path.display(),
                audio_data.len(),
                audio_data.len() as f32 / 16000.0
            );
        }
    }

    if audio_data.is_empty() {
        app_handle
            .emit(
                "status-changed",
                StatusEvent {
                    status: "idle".to_string(),
                    message: "No audio captured".to_string(),
                },
            )
            .ok();
        return Err("No audio data captured".to_string());
    }

    dev_log!(
        "[TIMING] T+{}ms: starting transcription...",
        t0.elapsed().as_millis()
    );

    // Add silence padding at START and END to prevent Whisper from cutting off words
    // START padding: compensates for CPAL stream startup delay
    // END padding: prevents Whisper from truncating the last words
    let start_padding_samples = 3200; // 200ms at 16kHz (covers pipeline startup delay)
    let end_padding_samples = 16000; // 1000ms at 16kHz (increased for better end capture)

    let mut padded_audio =
        Vec::with_capacity(start_padding_samples + audio_data.len() + end_padding_samples);

    // Add silence at START (compensates for pipeline startup delay)
    padded_audio.extend(std::iter::repeat_n(0.0f32, start_padding_samples));
    // Add actual audio
    padded_audio.extend_from_slice(&audio_data);
    // Add silence at END
    padded_audio.extend(std::iter::repeat_n(0.0f32, end_padding_samples));

    dev_log!(
        "[TIMING] T+{}ms: added {}ms start + {}ms end padding ({} -> {} samples)",
        t0.elapsed().as_millis(),
        start_padding_samples * 1000 / 16000,
        end_padding_samples * 1000 / 16000,
        audio_data.len(),
        padded_audio.len()
    );

    // Update STT activity timestamp (for idle watcher)
    update_stt_activity(state);

    // Read text processing config before entering spawn_blocking
    let (custom_words, custom_words_threshold, filler_removal, stutter_collapse) = {
        let cfg = state.config.lock().unwrap();
        (
            cfg.custom_words.clone(),
            cfg.custom_words_threshold,
            cfg.filler_removal_enabled,
            cfg.stutter_collapse_enabled,
        )
    };

    // Build initial_prompt from custom words to bias Whisper recognition
    let initial_prompt = text_processing::build_initial_prompt(&custom_words, {
        &state.config.lock().unwrap().language
    });

    // Clone Arcs to move into spawn_blocking
    let stt_engine_arc = Arc::clone(&state.stt_engine);
    let config_arc = Arc::clone(&state.config);

    // Use spawn_blocking to offload CPU-intensive Whisper transcription
    // Lazy reload is done INSIDE the lock to prevent race with idle watcher
    let prompt_for_whisper = if initial_prompt.is_empty() {
        None
    } else {
        Some(initial_prompt.clone())
    };
    let transcription = tokio::task::spawn_blocking(move || {
        let mut stt_engine = stt_engine_arc.lock().unwrap_or_else(|e| e.into_inner());

        // Lazy reload: if model was unloaded by idle watcher, re-initialize while holding lock
        let needs_reload = stt_engine.is_none();
        if needs_reload {
            dev_log!("[STT] Model was unloaded by idle watcher, reloading...");
            let config = config_arc.lock().unwrap().clone();
            match SttEngine::new(&config) {
                Ok(engine) => {
                    *stt_engine = Some(engine);
                    dev_log!("[STT] ✅ Model reloaded successfully");
                }
                Err(e) => {
                    dev_log!("[STT] ❌ Failed to reload model: {}", e);
                    return Err(format!("Failed to reload model: {}", e));
                }
            }
        }

        let engine = stt_engine.as_mut().ok_or("STT engine not initialized")?;
        engine.transcribe(&padded_audio, prompt_for_whisper.as_deref())
    })
    .await
    .map_err(|e| format!("Transcription task panicked: {}", e))??;

    // Update activity again after transcription
    update_stt_activity(state);

    dev_log!(
        "[TIMING] T+{}ms: transcription complete: '{}'",
        t0.elapsed().as_millis(),
        transcription
    );

    // Apply text processing pipeline: filler removal → stutter collapse → custom words
    let transcription = {
        let processed = text_processing::process_transcription(
            &transcription,
            &custom_words,
            custom_words_threshold,
            filler_removal,
            stutter_collapse,
        );
        if processed != transcription {
            dev_log!(
                "[TEXT] Post-processing: '{}' → '{}'",
                transcription,
                processed
            );
        }
        processed
    };

    // Optional Ollama post-processing for spelling/grammar correction
    let final_text = {
        let (ollama_enabled, ollama_url, ollama_model, language) = {
            let cfg = state.config.lock().unwrap();
            (
                cfg.ollama_enabled,
                cfg.ollama_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434".to_string()),
                cfg.ollama_model
                    .clone()
                    .unwrap_or_else(|| "mistral".to_string()),
                cfg.language.clone(),
            )
        };
        if ollama_enabled && !transcription.is_empty() {
            dev_log!("[OLLAMA] Correcting text with model '{}'...", ollama_model);
            match ollama_client::correct_text(&ollama_url, &ollama_model, &language, &transcription)
                .await
            {
                Ok(corrected) => {
                    if corrected != transcription {
                        dev_log!("[OLLAMA] Corrected: '{}' -> '{}'", transcription, corrected);
                    }
                    corrected
                }
                Err(e) => {
                    dev_log!("[OLLAMA] ⚠️ Correction failed: {} — using original text", e);
                    transcription
                }
            }
        } else {
            transcription
        }
    };

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "idle".to_string(),
                message: "Ready! Press Ctrl+Space to dictate".to_string(),
            },
        )
        .ok();

    Ok(final_text)
}

#[tauri::command]
#[specta::specta]
async fn type_text(text: String, app: AppHandle) -> Result<(), String> {
    let config = AppConfig::load().unwrap_or_default();
    do_type_text(text, &app, config.paste_method).await
}

/// Helper function for type_text - can be called from other modules
pub(crate) async fn do_type_text(
    text: String,
    app_handle: &AppHandle,
    paste_method: crate::config::PasteMethod,
) -> Result<(), String> {
    let mut output = TextOutput::new(60, paste_method, app_handle)?;
    output.type_text(&text)?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn show_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

const OVERLAY_WIDTH: f64 = 300.0;
const OVERLAY_HEIGHT: f64 = 80.0;

/// Create the overlay window programmatically (like Handy).
/// This gives full control over window properties that tauri.conf.json doesn't expose.
fn create_overlay_window(app_handle: &AppHandle) {
    use tauri::WebviewWindowBuilder;

    let url = if cfg!(debug_assertions) {
        // Dev mode: Vite serves overlay.html
        tauri::WebviewUrl::External("http://localhost:1420/overlay.html".parse().unwrap())
    } else {
        // Production: built overlay.html in dist
        tauri::WebviewUrl::App("overlay.html".into())
    };

    let mut builder = WebviewWindowBuilder::new(app_handle, "overlay", url)
        .title("Recording")
        .resizable(false)
        .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
        .shadow(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false);

    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.transparent(true);
    }

    // Position centered on primary monitor
    if let Ok(Some(monitor)) = app_handle.primary_monitor() {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let pos = monitor.position();
        let monitor_w = size.width as f64 / scale;
        let monitor_x = pos.x as f64 / scale;
        let monitor_y = pos.y as f64 / scale;

        let x = monitor_x + (monitor_w - OVERLAY_WIDTH) / 2.0;
        let y = monitor_y + 20.0;
        builder = builder.position(x, y);
    }

    match builder.build() {
        Ok(_window) => {
            dev_log!(
                "[OVERLAY] Window created successfully (hidden, {}x{})",
                OVERLAY_WIDTH,
                OVERLAY_HEIGHT
            );
        }
        Err(e) => {
            dev_log!("[OVERLAY] Failed to create overlay window: {}", e);
        }
    }
}

/// Position the overlay on the monitor where the cursor is, then show it.
/// Called from evdev_listener, keyboard_listener, and show_overlay command.
pub(crate) fn show_overlay_with_state(app: &AppHandle, state: &str) {
    if let Some(window) = app.get_webview_window("overlay") {
        // Reposition overlay centered on the monitor with the cursor
        if let Ok(monitors) = app.available_monitors() {
            // Try to find cursor position via xdotool (X11) or fallback
            let cursor_pos = get_cursor_position();

            if let Some((cx, cy)) = cursor_pos {
                for monitor in &monitors {
                    let pos = monitor.position();
                    let size = monitor.size();
                    let scale = monitor.scale_factor();

                    // Check if cursor is within this monitor
                    if cx >= pos.x
                        && cx < pos.x + size.width as i32
                        && cy >= pos.y
                        && cy < pos.y + size.height as i32
                    {
                        // Center overlay horizontally, place near top
                        let overlay_w = OVERLAY_WIDTH;
                        let monitor_w = size.width as f64 / scale;
                        let monitor_x = pos.x as f64 / scale;
                        let monitor_y = pos.y as f64 / scale;

                        let x = monitor_x + (monitor_w - overlay_w) / 2.0;
                        let y = monitor_y + 20.0; // 20px from top of monitor

                        let _ =
                            window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
                                x,
                                y,
                            }));
                        dev_log!(
                            "[OVERLAY] Positioned at ({}, {}) on monitor at ({}, {})",
                            x,
                            y,
                            pos.x,
                            pos.y
                        );
                        break;
                    }
                }
            } else if let Some(primary) = monitors.first() {
                // Fallback: center on primary monitor
                let size = primary.size();
                let scale = primary.scale_factor();
                let pos = primary.position();
                let overlay_w = OVERLAY_WIDTH;
                let monitor_w = size.width as f64 / scale;
                let monitor_x = pos.x as f64 / scale;
                let monitor_y = pos.y as f64 / scale;

                let x = monitor_x + (monitor_w - overlay_w) / 2.0;
                let y = monitor_y + 20.0;

                let _ =
                    window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
                dev_log!("[OVERLAY] Positioned on primary monitor at ({}, {})", x, y);
            }
        }

        // Show window first (content is opacity:0 by default), then emit state
        // React receives event → adds fade-in class → opacity transitions to 1
        let _ = window.show();
        let _ = window.emit("show-overlay", state);
    }
}

/// Hide overlay: emit hide-overlay (triggers CSS fade-out), then hide window after 300ms
pub(crate) fn hide_overlay_animated(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        // Emit event → React removes fade-in class → opacity transitions to 0
        let _ = window.emit("hide-overlay", ());
        // Hide window after fade-out animation completes (300ms)
        let window_clone = window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = window_clone.hide();
        });
    }
}

/// Get cursor position (cross-platform)
fn get_cursor_position() -> Option<(i32, i32)> {
    #[cfg(target_os = "linux")]
    {
        // Try xdotool first
        if let Ok(output) = std::process::Command::new("xdotool")
            .args(["getmouselocation", "--shell"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut x = None;
                let mut y = None;
                for line in stdout.lines() {
                    if let Some(val) = line.strip_prefix("X=") {
                        x = val.parse().ok();
                    } else if let Some(val) = line.strip_prefix("Y=") {
                        y = val.parse().ok();
                    }
                }
                if let (Some(x), Some(y)) = (x, y) {
                    return Some((x, y));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // CoreGraphics cursor position
        if let Ok(output) = std::process::Command::new("sh")
            .args(["-c", "python3 -c \"from Quartz import CGEventGetLocation, CGEventCreate; e=CGEventCreate(None); p=CGEventGetLocation(e); print(int(p.x), int(p.y))\""])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
                if parts.len() == 2 {
                    if let (Ok(x), Ok(y)) = (parts[0].parse(), parts[1].parse()) {
                        return Some((x, y));
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Win32 GetCursorPos
        if let Ok(output) = std::process::Command::new("powershell")
            .args([
                "-Command",
                "[System.Windows.Forms.Cursor]::Position | ForEach-Object { \"$($_.X) $($_.Y)\" }",
            ])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
                if parts.len() == 2 {
                    if let (Ok(x), Ok(y)) = (parts[0].parse(), parts[1].parse()) {
                        return Some((x, y));
                    }
                }
            }
        }
    }

    None
}

#[tauri::command]
#[specta::specta]
async fn show_overlay(app: AppHandle) -> Result<(), String> {
    show_overlay_with_state(&app, "recording");
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn hide_overlay(app: AppHandle) -> Result<(), String> {
    hide_overlay_animated(&app);
    Ok(())
}

#[derive(Clone, Serialize, Type)]
struct StatusEvent {
    status: String,
    message: String,
}

#[tauri::command]
#[specta::specta]
async fn load_settings(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
#[specta::specta]
async fn save_settings(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    new_config: AppConfig,
) -> Result<(), String> {
    let (old_shortcut, old_device, old_model, old_language, old_model_path, old_use_gpu) = {
        let config = state.config.lock().unwrap();
        (
            config.shortcut.clone(),
            config.audio_device_name.clone(),
            config.model.clone(),
            config.language.clone(),
            config.model_path.clone(),
            config.use_gpu,
        )
    };

    new_config.save()?;

    {
        let mut config = state.config.lock().unwrap();
        *config = new_config.clone();
    }

    if old_shortcut != new_config.shortcut {
        update_global_shortcut(&app_handle, &old_shortcut, &new_config.shortcut)?;
    }

    // Re-open audio worker on new device if device changed
    if old_device != new_config.audio_device_name {
        let device_name = new_config.audio_device_name.clone();
        let mut capture = state
            .audio_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // close_stream() shuts down the old worker thread
        capture.close_stream();
        match capture.open_stream(device_name.as_deref()) {
            Ok(()) => dev_log!(
                "[AUDIO] ✅ Audio worker re-spawned on new device: {:?}",
                device_name
            ),
            Err(e) => dev_log!(
                "[AUDIO] ⚠️ Worker re-spawn failed: {} — will retry on next PTT",
                e
            ),
        }
    }

    // Re-init STT engine if model, language, or GPU setting changed
    let stt_needs_reload = old_model != new_config.model
        || old_language != new_config.language
        || old_model_path != new_config.model_path
        || old_use_gpu != new_config.use_gpu;
    if stt_needs_reload {
        dev_log!(
            "[STT] Settings changed — reloading engine (model: {}, lang: {})",
            new_config.model,
            new_config.language
        );
        stt::suppress_backend_logs();
        match SttEngine::new(&new_config) {
            Ok(engine) => {
                *state.stt_engine.lock().unwrap_or_else(|e| e.into_inner()) = Some(engine);
                dev_log!("[STT] ✅ Engine reloaded successfully");
            }
            Err(e) => {
                dev_log!(
                    "[STT] ⚠️ Engine reload failed: {} — keeping current engine",
                    e
                );
            }
        }
    }

    Ok(())
}

/// Check if a shortcut string contains a mouse button (which Tauri's global shortcut doesn't support)
fn is_mouse_shortcut(shortcut: &str) -> bool {
    let lower = shortcut.to_lowercase();
    lower.contains("mouse") || lower.contains("btn_")
}

fn update_global_shortcut(
    app_handle: &AppHandle,
    old_shortcut: &str,
    new_shortcut: &str,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // Try to unregister the old shortcut (ignore errors if it was a mouse shortcut)
    if !is_mouse_shortcut(old_shortcut) {
        let _ = app_handle.global_shortcut().unregister(old_shortcut);
    }

    // If the new shortcut is a mouse button, don't register with Tauri's global shortcut
    // On Linux, evdev handles mouse button detection directly
    if is_mouse_shortcut(new_shortcut) {
        dev_log!(
            "[SHORTCUT] Mouse button shortcut '{}' - handled by evdev, skipping Tauri registration",
            new_shortcut
        );
        return Ok(());
    }

    // For keyboard shortcuts, register with Tauri's global shortcut system
    // Note: This is mainly used for the overlay toggle, not for push-to-talk
    // Push-to-talk is handled by evdev on Linux for better Wayland compatibility
    dev_log!(
        "[SHORTCUT] Registering keyboard shortcut '{}' with Tauri",
        new_shortcut
    );

    // We don't actually need to register push-to-talk shortcuts with Tauri
    // because evdev handles them. This function is kept for compatibility
    // but we skip the actual registration to avoid errors.
    // The overlay toggle uses F10 which is registered in setup().

    Ok(())
}

/// Transcribe audio data received from the frontend (Web Audio API)
/// This is the new approach that works reliably with PipeWire
#[tauri::command]
#[specta::specta]
async fn transcribe_audio(
    state: State<'_, AppState>,
    audio_data: Vec<f32>,
) -> Result<String, String> {
    dev_log!(
        "[TRANSCRIBE] Received {} samples from frontend",
        audio_data.len()
    );

    if audio_data.is_empty() {
        return Err("No audio data received".to_string());
    }

    let duration = audio_data.len() as f32 / 16000.0;
    dev_log!("[TRANSCRIBE] Audio duration: {:.2}s", duration);

    // Add 1 second of silence at the END to prevent Whisper from cutting off last words
    let padding_samples = 16000; // 1s at 16kHz (increased for better end capture)
    let mut padded_audio = audio_data.clone();
    padded_audio.extend(std::iter::repeat_n(0.0f32, padding_samples));
    dev_log!(
        "[TRANSCRIBE] Added {}ms padding at end ({} -> {} samples)",
        padding_samples * 1000 / 16000,
        audio_data.len(),
        padded_audio.len()
    );

    // Build initial_prompt from custom words
    let initial_prompt = {
        let cfg = state.config.lock().unwrap();
        text_processing::build_initial_prompt(&cfg.custom_words, &cfg.language)
    };
    let prompt_for_whisper = if initial_prompt.is_empty() {
        None
    } else {
        Some(initial_prompt)
    };

    // Clone Arc to move into spawn_blocking
    let stt_engine_arc = Arc::clone(&state.stt_engine);

    // Use spawn_blocking to offload CPU-intensive Whisper transcription
    // This prevents blocking the async executor
    let transcription = tokio::task::spawn_blocking(move || {
        let mut stt_engine = stt_engine_arc.lock().unwrap_or_else(|e| e.into_inner());
        let engine = stt_engine.as_mut().ok_or("STT engine not initialized")?;
        engine.transcribe(&padded_audio, prompt_for_whisper.as_deref())
    })
    .await
    .map_err(|e| format!("Transcription task panicked: {}", e))??;

    dev_log!("[TRANSCRIBE] Result: '{}'", transcription);
    Ok(transcription)
}

#[tauri::command]
#[specta::specta]
fn check_evdev_permissions() -> bool {
    use std::path::Path;
    let rules_file = Path::new("/etc/udev/rules.d/70-karaoketyper.rules");
    rules_file.exists()
}

#[tauri::command]
#[specta::specta]
async fn install_evdev_permissions(app_handle: AppHandle) -> Result<String, String> {
    use tauri_plugin_shell::ShellExt;

    dev_log!("[EVDEV INSTALL] Starting installation...");

    let script_path = std::env::current_dir()
        .map_err(|e| format!("Failed to get current directory: {}", e))?
        .parent()
        .ok_or("Failed to get parent directory")?
        .join("install-permissions.sh");

    if !script_path.exists() {
        return Err(format!(
            "Installation script not found at: {}",
            script_path.display()
        ));
    }

    dev_log!("[EVDEV INSTALL] Running script: {}", script_path.display());

    let shell = app_handle.shell();

    match shell
        .command("pkexec")
        .args(["sh", script_path.to_str().unwrap()])
        .output()
        .await
    {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                dev_log!("[EVDEV INSTALL] Success: {}", stdout);
                Ok(
                    "evdev permissions installed successfully! Please restart the application."
                        .to_string(),
                )
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                dev_log!("[EVDEV INSTALL] Failed: {}", stderr);
                Err(format!("Installation failed: {}", stderr))
            }
        }
        Err(e) => {
            dev_log!("[EVDEV INSTALL] Command error: {}", e);
            Err(format!("Failed to execute installation: {}", e))
        }
    }
}

/// Check if keyboard input permissions are available (cross-platform)
#[tauri::command]
#[specta::specta]
fn check_keyboard_permissions() -> bool {
    // On Windows/macOS: always true (no special permissions needed)
    // On Linux: check if user has input group membership
    #[cfg(target_os = "linux")]
    {
        keyboard_listener::check_linux_permissions().is_ok()
    }

    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Check if uinput is available for text output (Linux only)
#[tauri::command]
#[specta::specta]
fn check_uinput_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::fs::OpenOptions;
        // Try to open /dev/uinput for writing
        OpenOptions::new().write(true).open("/dev/uinput").is_ok()
    }

    #[cfg(not(target_os = "linux"))]
    {
        // On other platforms, we use enigo which doesn't need special permissions
        true
    }
}

/// Log message from frontend to Rust console
#[tauri::command]
#[specta::specta]
fn log_from_frontend(message: String) {
    dev_log!("[FRONTEND] {}", message);
}

/// Get system information for Settings UI
#[tauri::command]
#[specta::specta]
async fn get_system_info() -> Result<SystemInfo, String> {
    let os_name = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    #[cfg(target_os = "linux")]
    let display_server =
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string());
    #[cfg(not(target_os = "linux"))]
    let display_server = "native".to_string();

    #[cfg(target_os = "linux")]
    let desktop_env =
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".to_string());
    #[cfg(not(target_os = "linux"))]
    let desktop_env = "native".to_string();

    Ok(SystemInfo {
        os: os_name.to_string(),
        arch: arch.to_string(),
        display_server,
        desktop_environment: desktop_env,
    })
}

/// Get GPU acceleration status for Settings UI
#[tauri::command]
#[specta::specta]
fn get_gpu_info() -> Result<GpuInfo, String> {
    let result = gpu::detect_gpu();
    let failure_count = stt::gpu_health::get_gpu_failure_count();
    let disabled_by_failures = stt::gpu_health::is_gpu_disabled_by_failures();

    let recommended_model = result
        .vram_mb
        .map(|vram| gpu::recommended_model_for_vram(vram).to_string());

    Ok(GpuInfo {
        available: result.available,
        backend: result.backend,
        device_name: result.device_name,
        failure_count,
        disabled_by_failures,
        vram_mb: result.vram_mb,
        recommended_model,
    })
}

/// Get current GPU status (mode, failures, etc.)
#[tauri::command]
#[specta::specta]
fn get_gpu_status() -> Result<GpuStatus, String> {
    let failure_count = stt::gpu_health::get_gpu_failure_count();
    let disabled = stt::gpu_health::is_gpu_disabled_by_failures();

    // Determine current mode based on failure state
    let (mode, fallback_reason) = if disabled {
        (
            "cpu".to_string(),
            Some(format!(
                "GPU auto-disabled after {} consecutive failures",
                failure_count
            )),
        )
    } else {
        // Without active engine context, we report based on compile-time features
        let gpu_compiled = cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal");
        if gpu_compiled {
            ("gpu".to_string(), None)
        } else {
            (
                "cpu".to_string(),
                Some("GPU not compiled in this build".to_string()),
            )
        }
    };

    Ok(GpuStatus {
        mode,
        device_name: None,
        fallback_reason,
        failure_count,
    })
}

/// Reset GPU failure counter (allows retrying GPU after fixes)
#[tauri::command]
#[specta::specta]
fn reset_gpu_failure_count() -> Result<String, String> {
    stt::gpu_health::reset_gpu_failures();
    Ok("GPU failure counter reset. GPU will be tried again on next initialization.".to_string())
}

/// Get list of installed models with their metadata
#[tauri::command]
#[specta::specta]
async fn get_installed_models() -> Result<Vec<model_downloader::ModelInfo>, String> {
    model_downloader::check_installed_models()
}

/// Download a model with progress events
#[tauri::command]
#[specta::specta]
async fn download_model(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    model_name: String,
) -> Result<(), String> {
    // Download the model
    model_downloader::download_model(app_handle.clone(), model_name.clone()).await?;

    // Auto-reinitialize STT engine with the new model
    dev_log!(
        "[STT] Model download complete, reinitializing STT engine with model: {}",
        model_name
    );

    // Update config to use the new model and reinit
    {
        let mut config = state.config.lock().unwrap();
        config.model = model_name.clone();
        config.model_path = None; // Clear any stale model_path
    }

    // Reinitialize STT engine
    let config = state.config.lock().unwrap().clone();
    match SttEngine::new(&config) {
        Ok(engine) => {
            let mut stt = state.stt_engine.lock().unwrap_or_else(|e| e.into_inner());
            *stt = Some(engine);
            dev_log!(
                "[STT] ✅ STT engine reinitialized with model: {}",
                model_name
            );

            app_handle
                .emit(
                    "status-changed",
                    StatusEvent {
                        status: "idle".to_string(),
                        message: "Model ready! Press shortcut to dictate".to_string(),
                    },
                )
                .ok();
        }
        Err(e) => {
            dev_log!("[STT] ⚠️ Failed to reinitialize STT engine: {}", e);
            // Don't return error - model download succeeded, just init failed
        }
    }

    Ok(())
}

/// Install system dependencies (Linux only - handles evdev permissions)
#[tauri::command]
#[specta::specta]
async fn install_system_dependencies(app_handle: AppHandle) -> Result<String, String> {
    dev_log!("[SETUP] Checking system dependencies...");

    #[cfg(target_os = "linux")]
    {
        use tauri_plugin_shell::ShellExt;

        let mut actions: Vec<String> = Vec::new();
        let mut needs_restart = false;

        // 1. Check evdev permissions
        let evdev_ok = check_evdev_permissions();
        dev_log!(
            "[SETUP] evdev permissions: {}",
            if evdev_ok { "OK" } else { "needed" }
        );
        if !evdev_ok {
            actions.push("evdev permissions".to_string());
            needs_restart = true;
        }

        // 2. Check xdotool (X11 direct typing + paste shortcut, handles AZERTY)
        let xdotool_ok = std::process::Command::new("which")
            .arg("xdotool")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        dev_log!(
            "[SETUP] xdotool: {}",
            if xdotool_ok { "OK" } else { "needed" }
        );
        if !xdotool_ok {
            actions.push("xdotool".to_string());
        }

        // 3. Check xclip (X11 clipboard with proper text/plain MIME for Remmina)
        let xclip_ok = std::process::Command::new("which")
            .arg("xclip")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        dev_log!("[SETUP] xclip: {}", if xclip_ok { "OK" } else { "needed" });
        if !xclip_ok {
            actions.push("xclip".to_string());
        }

        if actions.is_empty() {
            dev_log!("[SETUP] ✅ All dependencies ready");
            return Ok("All dependencies ready ✅".to_string());
        }

        dev_log!("[SETUP] Installing: {:?}", actions);

        // Build install script
        let mut script = String::from("#!/bin/sh\nset -e\n");

        // Install packages via apt (Debian/Ubuntu) or dnf (Fedora) or pacman (Arch)
        let packages: Vec<&str> = actions
            .iter()
            .filter(|a| *a != "evdev permissions")
            .map(|a| a.as_str())
            .collect();

        if !packages.is_empty() {
            let pkg_list = packages.join(" ");
            script.push_str(&format!(
                "if command -v apt-get >/dev/null 2>&1; then\n  apt-get install -y {pkg}\nelif command -v dnf >/dev/null 2>&1; then\n  dnf install -y {pkg}\nelif command -v pacman >/dev/null 2>&1; then\n  pacman -S --noconfirm {pkg}\nfi\n",
                pkg = pkg_list
            ));
        }

        // evdev permissions (udev rule + input group)
        if !evdev_ok {
            script.push_str("usermod -aG input \"$SUDO_USER\" 2>/dev/null || usermod -aG input \"$(logname)\" 2>/dev/null || true\n");
            script.push_str("echo 'KERNEL==\"uinput\", MODE=\"0660\", GROUP=\"input\"' > /etc/udev/rules.d/99-karaoketyper-uinput.rules\n");
            script.push_str("udevadm control --reload-rules && udevadm trigger\n");
        }

        // Write temp script and run with pkexec
        let script_path = std::env::temp_dir().join("karaoketyper-install.sh");
        std::fs::write(&script_path, &script)
            .map_err(|e| format!("Failed to write install script: {}", e))?;

        dev_log!("[SETUP] Running install script via pkexec...");

        let shell = app_handle.shell();
        match shell
            .command("pkexec")
            .args([
                "sh",
                script_path
                    .to_str()
                    .unwrap_or("/tmp/karaoketyper-install.sh"),
            ])
            .output()
            .await
        {
            Ok(output) => {
                let _ = std::fs::remove_file(&script_path);
                if output.status.success() {
                    let msg = if needs_restart {
                        format!(
                            "Installed: {}. Please restart the application.",
                            actions.join(", ")
                        )
                    } else {
                        format!("Installed: {} ✅", actions.join(", "))
                    };
                    dev_log!("[SETUP] ✅ {}", msg);
                    Ok(msg)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    dev_log!("[SETUP] ❌ Install failed: {}", stderr);
                    Err(format!("Installation failed: {}", stderr))
                }
            }
            Err(e) => {
                let _ = std::fs::remove_file(&script_path);
                Err(format!("Failed to run installer: {}", e))
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = app_handle;
        Ok("All dependencies ready ✅".to_string())
    }
}

#[tauri::command]
#[specta::specta]
fn get_autostart_enabled() -> bool {
    autostart::is_autostart_enabled()
}

#[tauri::command]
#[specta::specta]
async fn set_autostart_enabled(enabled: bool) -> Result<String, String> {
    if enabled {
        autostart::enable_autostart()?;
        Ok("Autostart enabled".to_string())
    } else {
        autostart::disable_autostart()?;
        Ok("Autostart disabled".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Build the invoke handler with tauri-specta for type-safe TypeScript bindings
    let builder =
        tauri_specta::Builder::<tauri::Wry>::new().commands(tauri_specta::collect_commands![
            init_stt,
            start_recording,
            stop_recording,
            type_text,
            show_settings,
            show_overlay,
            hide_overlay,
            audio_devices::get_audio_devices,
            load_settings,
            save_settings,
            check_evdev_permissions,
            install_evdev_permissions,
            check_keyboard_permissions,
            check_uinput_available,
            install_system_dependencies,
            transcribe_audio,
            log_from_frontend,
            get_installed_models,
            download_model,
            get_system_info,
            gnome_extension::check_focused_window_extension,
            gnome_extension::install_focused_window_extension,
            gnome_extension::remove_focused_window_extension,
            get_autostart_enabled,
            set_autostart_enabled,
            app_data::get_app_data_info,
            app_data::cleanup_app_data,
            get_gpu_info,
            get_gpu_status,
            reset_gpu_failure_count,
            check_ollama_status
        ]);

    // Generate TypeScript bindings in development mode
    #[cfg(debug_assertions)]
    builder
        .export(
            specta_typescript::Typescript::default(),
            "../src/bindings.ts",
        )
        .expect("Failed to export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // When another instance tries to start, show the existing settings window
            dev_log!("[SINGLE-INSTANCE] Another instance tried to start - focusing existing window");
            if let Some(window) = app.get_webview_window("settings") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::default())
        .setup(|app| {
            // Create overlay window programmatically (like Handy)
            // Not in tauri.conf.json — gives full control over window properties
            create_overlay_window(app.handle());

            // Initialize STT engine at startup
            dev_log!("[STARTUP] Initializing STT engine...");
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();
                match init_stt(app_handle.clone(), state).await {
                    Ok(_) => dev_log!("[STARTUP] ✅ STT engine initialized successfully"),
                    Err(e) => {
                        dev_log!("[STARTUP] ⚠️  STT initialization skipped: {}", e);
                        dev_log!("[STARTUP] Open Settings to configure STT backend");
                    }
                }
            });

            let open_settings = MenuItem::with_id(app, "open_settings", "Settings", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&open_settings, &quit_item])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(Image::from_bytes(include_bytes!("../icons/icon.png"))?)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .tooltip("KaraokeTyper")
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "open_settings" => {
                            if let Some(settings_win) = app.get_webview_window("settings") {
                                let _ = settings_win.show();
                                let _ = settings_win.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

            // Detect display server
            let display_server = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string());
            dev_log!("[SHORTCUT DEBUG] Display server: {}", display_server);

            if display_server == "wayland" {
                dev_log!("[SHORTCUT WARNING] ⚠️  Running on Wayland - global shortcuts may not work");
                dev_log!("[SHORTCUT WARNING] Modifiers (Ctrl, Alt) are often blocked by Wayland compositor");
                dev_log!("[SHORTCUT WARNING] Consider using X11 session or simple F-key shortcuts");
            }

            let shortcut = {
                let state = app.state::<AppState>();
                let config = state.config.lock().unwrap();
                config.shortcut.clone()
            };

            dev_log!("[SHORTCUT DEBUG] Loading primary shortcut from config: '{}'", shortcut);
            dev_log!("[SHORTCUT DEBUG] ⚠️  Ctrl+Space handled by evdev only - Global Shortcuts disabled");

            // Only register F10 for Settings window
            let shortcuts_to_try = vec![
                "F10".to_string(), // Settings window
            ];

            let mut registered_shortcuts = Vec::new();

            for test_shortcut in shortcuts_to_try {
                let app_handle = app.handle().clone();
                let shortcut_name = test_shortcut.clone();

                match app.global_shortcut().on_shortcut(test_shortcut.as_str(), move |_app, _shortcut_event, event| {
                    if event.state == ShortcutState::Pressed {
                        dev_log!("[SHORTCUT DEBUG] F10 pressed - showing settings");
                        if let Some(settings_win) = app_handle.get_webview_window("settings") {
                            let _ = settings_win.show();
                            let _ = settings_win.set_focus();
                        }
                    }
                }) {
                    Ok(_) => {
                        dev_log!("[SHORTCUT DEBUG] ✅ Successfully registered shortcut: '{}'", shortcut_name);
                        registered_shortcuts.push(shortcut_name);
                    }
                    Err(e) => {
                        dev_log!("[SHORTCUT WARNING] ⚠️  Failed to register shortcut '{}': {}", shortcut_name, e);
                    }
                }
            }

            if registered_shortcuts.is_empty() {
                dev_log!("[SHORTCUT WARNING] ⚠️  Failed to register ANY global shortcuts!");
                if display_server == "wayland" {
                    dev_log!("[SHORTCUT INFO] On Wayland, global F-key shortcuts are often blocked by other apps.");
                    dev_log!("[SHORTCUT INFO] Don't worry - evdev handles Ctrl+Space for push-to-talk! 👍");
                } else {
                    dev_log!("[SHORTCUT INFO] No global shortcuts available, but evdev handles Ctrl+Space! 👍");
                }
            } else {
                dev_log!("[SHORTCUT DEBUG] 📋 Active shortcuts: {:?}", registered_shortcuts);
                if display_server == "wayland" && !registered_shortcuts.contains(&shortcut) {
                    dev_log!("[SHORTCUT WARNING] ⚠️  Your configured shortcut '{}' doesn't work on Wayland", shortcut);
                    dev_log!("[SHORTCUT WARNING] Using fallback: {:?}", registered_shortcuts);
                }
            }

            // Start keyboard listener for push-to-talk (cross-platform)
            // - Linux: uses evdev for best Wayland support
            // - Windows/macOS: uses rdev
            // NOTE: Delayed start to avoid Bluetooth audio interference during device enumeration
            dev_log!("[KEYBOARD] Starting push-to-talk listener (delayed)...");
            let app_handle_keyboard = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                // Delay startup to let Bluetooth audio devices stabilize
                // This prevents crackling/noise when the app launches with Bluetooth headsets
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                match keyboard_listener::start_keyboard_listener(app_handle_keyboard).await {
                    Ok(_) => {
                        dev_log!("[KEYBOARD] ✅ Keyboard listener active");
                    }
                    Err(e) => {
                        dev_log!("[KEYBOARD] ❌ Keyboard listener failed: {}", e);
                        dev_log!("[KEYBOARD] Push-to-talk will not work.");

                        #[cfg(target_os = "linux")]
                        {
                            dev_log!("[KEYBOARD] Add user to 'input' group:");
                            dev_log!("[KEYBOARD]   sudo usermod -aG input $USER");
                            dev_log!("[KEYBOARD]   Then log out and back in.");
                        }
                    }
                }
            });

            // NOTE: STT engine is initialized in the spawn block at lines 1887-1900
            // (single initialization point using init_stt function)

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                dev_log!("[WINDOW] Close requested for window: {}", label);

                // Hide window instead of closing (tray app behavior)
                if let Err(e) = window.hide() {
                    dev_log!("[WINDOW] Failed to hide window {}: {}", label, e);
                } else {
                    dev_log!("[WINDOW] Window {} hidden successfully", label);
                }
                api.prevent_close();
            }
        })
        .invoke_handler(builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
