#[macro_use]
mod logging;

mod audio;
mod autostart;
mod config;
mod gpu;
mod keyboard_listener;
mod model_downloader;
mod output;
mod stt_whisper;

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

/// Single app data item info
#[derive(Clone, Serialize, Type)]
pub struct AppDataItem {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub size_mb: f64,
}

/// App data info for cleanup UI
#[derive(Clone, Serialize, Type)]
pub struct AppDataInfo {
    pub items: Vec<AppDataItem>,
    pub total_size_mb: f64,
}

/// GNOME extension status
#[derive(Clone, Serialize, Type)]
pub struct GnomeExtensionStatus {
    pub installed: bool,
    pub enabled: bool,
    pub is_gnome: bool,
    pub is_wayland: bool,
    pub path: String,
    pub needs_restart: bool,
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

use stt_whisper::WhisperSttEngine;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// STT engine wrapper (native Rust whisper-rs)
pub struct SttEngine {
    engine: WhisperSttEngine,
}

impl SttEngine {
    pub fn new(config: &AppConfig) -> Result<Self, String> {
        // Use model_path if provided AND it's a valid local file, otherwise resolve from model name
        // This prevents using URLs or invalid paths that were incorrectly saved in config
        let resolved_path = config.model_path.clone()
            .filter(|p| {
                // Reject URLs
                if p.starts_with("http://") || p.starts_with("https://") {
                    dev_log!("[STT] Ignoring URL in model_path: {}", p);
                    return false;
                }
                // Check if file exists
                let exists = std::path::Path::new(p).exists();
                if !exists {
                    dev_log!("[STT] model_path doesn't exist, will resolve from model name: {}", p);
                }
                exists
            })
            .or_else(|| {
                model_downloader::get_model_path(&config.model)
            });

        let model_path = resolved_path.ok_or("No model downloaded. Download a model from Settings → STT Models.")?;
        let engine = WhisperSttEngine::new(&model_path, &config.language, config.use_gpu, config.gpu_device_index);

        // Initialize the Whisper context by loading the model
        // Returns GpuInitResult with info about which mode was actually used
        let _init_result = engine.initialize().map_err(|e| e.to_string())?;

        Ok(SttEngine { engine })
    }

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        self.engine
            .transcribe(audio_data)
            .map_err(|e| e.to_string())
    }

    pub fn backend_name(&self) -> &'static str {
        "whisper-rs"
    }
}

// CPAL: stream opened on first PTT (slow ~1-2s), kept paused between recordings (instant PTT)
pub(crate) struct AppState {
    audio_capture: Arc<Mutex<AudioCapture>>,
    stt_engine: Arc<Mutex<Option<SttEngine>>>,
    config: Arc<Mutex<AppConfig>>,
    recording_ready: Arc<std::sync::atomic::AtomicBool>,
    pub recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_capture: Arc::new(Mutex::new(AudioCapture::new(16000))),
            stt_engine: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(AppConfig::load().unwrap_or_default())),
            recording_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            recording_start_time: Arc::new(Mutex::new(None)),
        }
    }
}

/// Get D-Bus environment variables needed for PulseAudio/PipeWire/GNOME commands
/// when launched from .desktop file (where these might not be inherited)
#[cfg(target_os = "linux")]
fn get_dbus_env() -> (String, String) {
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
    stt_whisper::suppress_whisper_cpp_logs();

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
        config.model, config.language
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
    *state.stt_engine.lock().unwrap() = Some(engine);

    // Pre-open audio stream now (paused) so first PTT press is instant
    // Without this, first open_stream() takes ~2s (ALSA enumeration) and loses speech
    {
        let capture = state.audio_capture.lock().unwrap();
        let device_name = {
            let config = state.config.lock().unwrap();
            config.audio_device_name.clone()
        };
        dev_log!("[AUDIO] Pre-opening audio stream...");
        match capture.open_stream(device_name.as_deref()) {
            Ok(()) => dev_log!("[AUDIO] ✅ Stream pre-opened (paused) — first PTT will be instant"),
            Err(e) => dev_log!("[AUDIO] ⚠️ Pre-open failed: {} — will retry on first PTT", e),
        }
    }

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
async fn start_recording(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    do_start_recording(app_handle, &state).await
}

/// Helper function for start_recording - can be called from other modules
/// Stream is already open — this just toggles the recording flag (instant)
pub(crate) async fn do_start_recording(
    app_handle: AppHandle,
    state: &State<'_, AppState>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;

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

    // Open stream if not already open, then start recording
    {
        let capture = state.audio_capture.lock().unwrap();

        if !capture.is_stream_open() {
            let device_name = {
                let config = state.config.lock().unwrap();
                config.audio_device_name.clone()
            };
            dev_log!("[TIMING] T+{}ms: Opening audio stream...", t0.elapsed().as_millis());
            capture.open_stream(device_name.as_deref())?;
            dev_log!("[TIMING] T+{}ms: Stream opened", t0.elapsed().as_millis());
        } else {
            dev_log!("[TIMING] T+{}ms: Stream already open (instant)", t0.elapsed().as_millis());
        }

        capture.start_recording()?;
    }

    state.recording_ready.store(true, Ordering::SeqCst);
    dev_log!(
        "[TIMING] T+{}ms: Recording started",
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

/// Helper function for stop_recording - can be called from other modules
pub(crate) async fn do_stop_recording(
    app_handle: AppHandle,
    state: &State<'_, AppState>,
) -> Result<String, String> {
    use std::sync::atomic::Ordering;

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

    // Wait for recording to be ready (max 3000ms — open_stream can take ~2s on first PTT)
    let mut waited = 0;
    while !state.recording_ready.load(Ordering::SeqCst) && waited < 3000 {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        waited += 10;
    }
    dev_log!(
        "[TIMING] T+{}ms: waited {}ms for recording_ready",
        t0.elapsed().as_millis(),
        waited
    );

    if !state.recording_ready.load(Ordering::SeqCst) {
        dev_log!("[RECORDING] ⚠️ Recording never became ready, proceeding anyway");
    }

    // Reset the ready flag
    state.recording_ready.store(false, Ordering::SeqCst);

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "processing".to_string(),
                message: "Processing speech...".to_string(),
            },
        )
        .ok();

    dev_log!(
        "[TIMING] T+{}ms: stopping audio capture...",
        t0.elapsed().as_millis()
    );

    // Stop recording (stream stays open/paused for instant next PTT)
    let audio_data = {
        let capture = state.audio_capture.lock().unwrap();

        // Pause stream — mic stops capturing but stays ready
        capture.stop_recording();

        // Get the captured audio buffer
        capture.get_buffer()
    };

    dev_log!(
        "[TIMING] T+{}ms: audio capture stopped, got {} samples ({:.2}s)",
        t0.elapsed().as_millis(),
        audio_data.len(),
        audio_data.len() as f32 / 16000.0
    );

    // DEBUG: Save audio to file for inspection (cross-platform temp dir)
    let debug_path = std::env::temp_dir().join("karaoketyper_debug.wav");
    if let Err(e) = save_debug_wav(&audio_data, &debug_path) {
        dev_log!("[DEBUG] Failed to save debug WAV: {}", e);
    } else {
        dev_log!(
            "[DEBUG] 🎵 Audio saved to: {} ({} samples, {:.2}s)",
            debug_path.display(),
            audio_data.len(),
            audio_data.len() as f32 / 16000.0
        );
    }
    dev_log!("[TIMING] T+{}ms: debug WAV saved", t0.elapsed().as_millis());

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
    let start_padding_samples = 3200;  // 200ms at 16kHz (covers pipeline startup delay)
    let end_padding_samples = 16000;   // 1000ms at 16kHz (increased for better end capture)

    let mut padded_audio = Vec::with_capacity(
        start_padding_samples + audio_data.len() + end_padding_samples
    );

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

    // Clone Arc to move into spawn_blocking
    let stt_engine_arc = Arc::clone(&state.stt_engine);

    // Use spawn_blocking to offload CPU-intensive Whisper transcription
    // This prevents blocking the async executor (like Handy app does)
    let transcription = tokio::task::spawn_blocking(move || {
        let stt_engine = stt_engine_arc.lock().unwrap();
        let engine = stt_engine.as_ref().ok_or("STT engine not initialized")?;
        engine.transcribe(&padded_audio)
    })
    .await
    .map_err(|e| format!("Transcription task panicked: {}", e))??;

    dev_log!(
        "[TIMING] T+{}ms: transcription complete: '{}'",
        t0.elapsed().as_millis(),
        transcription
    );

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "idle".to_string(),
                message: "Ready! Press Ctrl+Space to dictate".to_string(),
            },
        )
        .ok();

    Ok(transcription)
}

#[tauri::command]
#[specta::specta]
async fn type_text(text: String) -> Result<(), String> {
    do_type_text(text).await
}

/// Helper function for type_text - can be called from other modules
pub(crate) async fn do_type_text(text: String) -> Result<(), String> {
    let mut output = TextOutput::new(100)?; // 100ms delay for key release
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

#[tauri::command]
#[specta::specta]
async fn show_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn hide_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    #[cfg(target_os = "linux")]
    {
        get_audio_devices_linux().await
    }

    #[cfg(not(target_os = "linux"))]
    {
        get_audio_devices_generic()
    }
}

#[cfg(target_os = "linux")]
async fn get_audio_devices_linux() -> Result<Vec<AudioDevice>, String> {
    // Hybrid approach:
    // 1. /proc/asound/cards + lsusb → hardware devices with nice USB names
    // 2. pactl list sources → Bluetooth and other PipeWire-only devices
    // This ensures ALL input sources are visible (USB, PCI, Bluetooth, etc.)

    let alsa_cards = read_alsa_card_names();
    let usb_names = parse_lsusb();

    let mut devices = Vec::new();
    let mut seen_raw_names = std::collections::HashSet::new();

    // Always add system default first (index 0)
    devices.push(AudioDevice {
        name: "🎵 System Default".to_string(),
        raw_name: "default".to_string(),
        index: 0,
    });
    seen_raw_names.insert("default".to_string());

    // 1. Add hardware devices from /proc/asound/cards (reliable, always available)
    let mut index: u32 = 1;
    for (card_id, description) in &alsa_cards {
        if description.to_lowercase().contains("hdmi") {
            continue;
        }

        let raw_name = format!("plughw:CARD={},DEV=0", card_id);
        let emoji = format_device_emoji(description);

        devices.push(AudioDevice {
            name: format!("{} {}", emoji, description),
            raw_name: raw_name.clone(),
            index,
        });
        seen_raw_names.insert(card_id.to_lowercase());
        index += 1;
    }

    // 2. Add PipeWire-only devices from pactl (Bluetooth, virtual sources, etc.)
    let (xdg_runtime, dbus_addr) = get_dbus_env();
    if let Ok(output) = tokio::process::Command::new("pactl")
        .args(["list", "sources"])
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
        .env("LC_ALL", "C")
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut current_name: Option<String> = None;
            let mut current_description: Option<String> = None;

            for line in stdout.lines() {
                let line = line.trim();
                if line.starts_with("Name:") {
                    if let (Some(name), Some(desc)) = (current_name.take(), current_description.take()) {
                        add_pactl_device(&name, &desc, &seen_raw_names, &usb_names, &mut devices, &mut index);
                    }
                    current_name = Some(line.trim_start_matches("Name:").trim().to_string());
                } else if line.starts_with("Description:") {
                    current_description = Some(line.trim_start_matches("Description:").trim().to_string());
                }
            }
            // Don't forget the last device
            if let (Some(name), Some(desc)) = (current_name, current_description) {
                add_pactl_device(&name, &desc, &seen_raw_names, &usb_names, &mut devices, &mut index);
            }
        }
    }

    // 3. Add Bluetooth devices from pactl list cards (visible even in A2DP mode)
    // In A2DP mode, BT devices have no input source, so step 2 misses them.
    // We detect bluez_card.* with headset profiles and construct the source name.
    if let Ok(output) = tokio::process::Command::new("pactl")
        .args(["list", "cards"])
        .env("XDG_RUNTIME_DIR", &xdg_runtime)
        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
        .env("LC_ALL", "C")
        .output()
        .await
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for bt_dev in parse_bt_cards_from_pactl(&stdout) {
                // Skip if already listed (e.g. was in HFP mode and found via sources)
                let already_listed = devices.iter().any(|d| d.raw_name.starts_with(&format!("bluez_input.{}", bt_dev.mac)));
                if !already_listed {
                    let raw_name = format!("bluez_input.{}.0", bt_dev.mac);
                    devices.push(AudioDevice {
                        name: format!("🎧 {}", bt_dev.description),
                        raw_name,
                        index,
                    });
                    index += 1;
                    dev_log!("[DEVICES] Added BT device from card: {} ({})", bt_dev.description, bt_dev.mac);
                }
            }
        }
    }

    dev_log!("[DEVICES] Listed {} audio devices:", devices.len());
    for dev in &devices {
        dev_log!(
            "[DEVICES]   [{}] {} -> {}",
            dev.index, dev.name, dev.raw_name
        );
    }

    Ok(devices)
}

/// Bluetooth card info parsed from pactl list cards
#[cfg(target_os = "linux")]
struct BtCardInfo {
    mac: String,         // e.g. "40_58_99_49_68_8F"
    description: String, // e.g. "G435 Bluetooth Gaming Headset"
}

/// Parse pactl list cards output to find Bluetooth cards with headset (mic) profiles
#[cfg(target_os = "linux")]
fn parse_bt_cards_from_pactl(output: &str) -> Vec<BtCardInfo> {
    let mut results = Vec::new();
    // Split into card blocks
    let cards: Vec<&str> = output.split("\nCard #").collect();

    for card in cards {
        // Look for bluez card name
        let card_name = card.lines()
            .find(|l| l.contains("device.name") && l.contains("bluez_card."))
            .and_then(|l| l.split('"').nth(1));

        let card_name = match card_name {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Check if headset-head-unit profile is available
        let has_headset = card.lines().any(|l| {
            let trimmed = l.trim();
            trimmed.starts_with("headset-head-unit:")
                || trimmed.starts_with("headset-head-unit-cvsd:")
                || trimmed.starts_with("headset-head-unit-msbc:")
        });

        if !has_headset {
            continue;
        }

        // Extract MAC from card name: "bluez_card.40_58_99_49_68_8F" → "40_58_99_49_68_8F"
        let mac = card_name.strip_prefix("bluez_card.").unwrap_or("").to_string();
        if mac.is_empty() {
            continue;
        }

        // Get device description
        let description = card.lines()
            .find(|l| l.contains("device.description"))
            .and_then(|l| l.split('"').nth(1))
            .unwrap_or("Bluetooth Headset")
            .to_string();

        results.push(BtCardInfo { mac, description });
    }
    results
}

/// Add a pactl source to the device list if it's not a duplicate or monitor
#[cfg(target_os = "linux")]
fn add_pactl_device(
    raw_name: &str,
    description: &str,
    _seen: &std::collections::HashSet<String>,
    _usb_names: &std::collections::HashMap<String, String>,
    devices: &mut Vec<AudioDevice>,
    index: &mut u32,
) {
    // Skip monitor sources (they capture output audio, not mic input)
    if raw_name.contains(".monitor") {
        return;
    }

    // Skip ALSA-backed sources — these are PipeWire wrappers around hardware devices
    // already listed from /proc/asound/cards (USB, PCI, etc.)
    // Only keep truly PipeWire-native sources: Bluetooth (bluez_*), virtual, etc.
    if raw_name.starts_with("alsa_input.") || raw_name.starts_with("alsa_output.") {
        return;
    }

    // This is a PipeWire-only source (Bluetooth, virtual, etc.) — add it
    let emoji = format_device_emoji(description);
    devices.push(AudioDevice {
        name: format!("{} {}", emoji, description),
        raw_name: raw_name.to_string(),
        index: *index,
    });
    *index += 1;
}

/// Choose an emoji based on device description
#[cfg(target_os = "linux")]
fn format_device_emoji(description: &str) -> &'static str {
    let lower = description.to_lowercase();
    if lower.contains("bluetooth") || lower.contains("bluez") {
        "🎧"
    } else if lower.contains("046d") || lower.contains("0x46d") || lower.contains("webcam") || lower.contains("c270") {
        "📷"
    } else if lower.contains("usb") {
        "🎤"
    } else if lower.contains("hd-audio") || lower.contains("generic") || lower.contains("hda") {
        "🔊"
    } else {
        "🎵"
    }
}

/// Read ALSA card names from /proc/asound/cards, enriched with USB product names from lsusb
/// Returns ordered list of (card_id, description)
/// Example: ("U0x46d0x825", "Webcam C270 (Logitech)")
#[cfg(target_os = "linux")]
fn read_alsa_card_names() -> Vec<(String, String)> {
    let mut cards = Vec::new();
    let content = match std::fs::read_to_string("/proc/asound/cards") {
        Ok(c) => c,
        Err(_) => return cards,
    };

    // Parse lsusb to get real USB product names
    let usb_names = parse_lsusb();

    let lines: Vec<&str> = content.lines().collect();
    for chunk in lines.chunks(2) {
        if let Some(first_line) = chunk.first() {
            if let (Some(bracket_start), Some(bracket_end)) = (first_line.find('['), first_line.find(']')) {
                let card_id = first_line[bracket_start + 1..bracket_end].trim().to_string();

                // For USB cards, try to get a better name from lsusb
                let description = if let Some(usb_name) = resolve_usb_card_name(&card_id, &usb_names) {
                    usb_name
                } else if let Some(dash_pos) = first_line.find(" - ") {
                    first_line[dash_pos + 3..].trim().to_string()
                } else {
                    card_id.clone()
                };

                cards.push((card_id, description));
            }
        }
    }

    cards
}

/// Parse lsusb output into a map of "vendor_id:product_id" → "Product Name"
/// Example: "046d:0825" → "Webcam C270"
#[cfg(target_os = "linux")]
fn parse_lsusb() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let output = match std::process::Command::new("lsusb").output() {
        Ok(o) => o,
        Err(_) => return map,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Format: "Bus 001 Device 002: ID 046d:0825 Logitech, Inc. Webcam C270"
    for line in stdout.lines() {
        if let Some(id_pos) = line.find("ID ") {
            let after_id = &line[id_pos + 3..];
            if after_id.len() > 9 {
                let usb_id = &after_id[..9]; // "046d:0825"
                let product = after_id[10..].trim();
                // Clean up vendor prefix: "Logitech, Inc. Webcam C270" → "Webcam C270 (Logitech)"
                let friendly = if let Some(dot_pos) = product.find(". ") {
                    let vendor = product[..dot_pos].trim_end_matches(',');
                    let name = &product[dot_pos + 2..];
                    format!("{} ({})", name, vendor)
                } else {
                    product.to_string()
                };
                map.insert(usb_id.to_lowercase(), friendly);
            }
        }
    }
    map
}

/// Try to resolve an ALSA USB card name (like "U0x46d0x825") to a lsusb product name
#[cfg(target_os = "linux")]
fn resolve_usb_card_name(card_id: &str, usb_names: &std::collections::HashMap<String, String>) -> Option<String> {
    // ALSA USB card IDs look like "U0x46d0x825" → USB vendor 046d, product 0825
    if !card_id.starts_with('U') {
        return None;
    }

    let hex_part = &card_id[1..]; // "0x46d0x825"
    // Split on "0x" boundaries: ["", "46d", "825"]
    let parts: Vec<&str> = hex_part.split("0x").filter(|s| !s.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }

    let vendor = format!("{:0>4}", parts[0]); // "046d"
    let product = format!("{:0>4}", parts[1]); // "0825"
    let usb_id = format!("{}:{}", vendor, product);

    usb_names.get(&usb_id).cloned()
}

#[cfg(not(target_os = "linux"))]
fn get_audio_devices_generic() -> Result<Vec<AudioDevice>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let input_devices = host.input_devices().map_err(|e| e.to_string())?;

    let devices: Vec<AudioDevice> = input_devices
        .enumerate()
        .filter_map(|(index, device)| {
            device.name().ok().map(|raw_name| AudioDevice {
                name: raw_name.clone(), // On non-Linux, just use raw name as display
                raw_name,
                index: index as u32,
            })
        })
        .collect();

    if devices.is_empty() {
        return Err("No audio input devices found".to_string());
    }

    Ok(devices)
}

#[derive(Clone, Serialize, Type)]
struct AudioDevice {
    name: String,     // Display name for UI
    raw_name: String, // Actual cpal device name for audio.rs
    index: u32,       // Index in cpal device list
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
    let (old_shortcut, old_device) = {
        let config = state.config.lock().unwrap();
        (config.shortcut.clone(), config.audio_device_name.clone())
    };

    new_config.save()?;

    {
        let mut config = state.config.lock().unwrap();
        *config = new_config.clone();
    }

    if old_shortcut != new_config.shortcut {
        update_global_shortcut(&app_handle, &old_shortcut, &new_config.shortcut)?;
    }

    // Close any existing stream if device changed (will re-open on next PTT)
    if old_device != new_config.audio_device_name {
        let capture = state.audio_capture.lock().unwrap();
        capture.close_stream();
        dev_log!("[AUDIO] Stream closed — new device will be used on next PTT");
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

    // Clone Arc to move into spawn_blocking
    let stt_engine_arc = Arc::clone(&state.stt_engine);

    // Use spawn_blocking to offload CPU-intensive Whisper transcription
    // This prevents blocking the async executor
    let transcription = tokio::task::spawn_blocking(move || {
        let stt_engine = stt_engine_arc.lock().unwrap();
        let engine = stt_engine.as_ref().ok_or("STT engine not initialized")?;
        engine.transcribe(&padded_audio)
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
    let failure_count = stt_whisper::get_gpu_failure_count();
    let disabled_by_failures = stt_whisper::is_gpu_disabled_by_failures();

    Ok(GpuInfo {
        available: result.available,
        backend: result.backend,
        device_name: result.device_name,
        failure_count,
        disabled_by_failures,
    })
}

/// Get current GPU status (mode, failures, etc.)
#[tauri::command]
#[specta::specta]
fn get_gpu_status() -> Result<GpuStatus, String> {
    let failure_count = stt_whisper::get_gpu_failure_count();
    let disabled = stt_whisper::is_gpu_disabled_by_failures();

    // Determine current mode based on failure state
    let (mode, fallback_reason) = if disabled {
        ("cpu".to_string(), Some(format!("GPU auto-disabled after {} consecutive failures", failure_count)))
    } else {
        // Without active engine context, we report based on compile-time features
        let gpu_compiled = cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal");
        if gpu_compiled {
            ("gpu".to_string(), None)
        } else {
            ("cpu".to_string(), Some("GPU not compiled in this build".to_string()))
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
    stt_whisper::reset_gpu_failures();
    Ok("GPU failure counter reset. GPU will be tried again on next initialization.".to_string())
}

/// Get all app data paths for cleanup
fn get_app_data_paths() -> Vec<(String, std::path::PathBuf)> {
    let mut paths = Vec::new();

    // Config directory: ~/.config/karaoketyper/
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(("Config".to_string(), config_dir.join("karaoketyper")));
    }

    // Data directory: ~/.local/share/karaoketyper/ (contains venv)
    if let Some(data_dir) = dirs::data_dir() {
        paths.push(("Data (venv)".to_string(), data_dir.join("karaoketyper")));
    }

    // Autostart file: ~/.config/autostart/karaoketyper.desktop
    if let Some(config_dir) = dirs::config_dir() {
        paths.push((
            "Autostart".to_string(),
            config_dir.join("autostart").join("karaoketyper.desktop"),
        ));
    }

    // Hugging Face cache for our models: ~/.cache/huggingface/hub/models--csukuangfj--*
    if let Some(cache_dir) = dirs::cache_dir() {
        let hf_hub = cache_dir.join("huggingface").join("hub");
        if hf_hub.exists() {
            if let Ok(entries) = std::fs::read_dir(&hf_hub) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("models--csukuangfj--sherpa-onnx") {
                        paths.push((format!("Model: {}", name), entry.path()));
                    }
                }
            }
        }
    }

    paths
}

/// Get app data info for display in UI
#[tauri::command]
#[specta::specta]
fn get_app_data_info() -> Result<AppDataInfo, String> {
    let paths = get_app_data_paths();
    let mut total_size: u64 = 0;
    let mut items = Vec::new();

    for (name, path) in &paths {
        let exists = path.exists();
        let size = if exists {
            calculate_dir_size(path).unwrap_or(0)
        } else {
            0
        };
        total_size += size;

        items.push(AppDataItem {
            name: name.clone(),
            path: path.to_string_lossy().to_string(),
            exists,
            size_mb: size as f64 / 1024.0 / 1024.0,
        });
    }

    Ok(AppDataInfo {
        items,
        total_size_mb: total_size as f64 / 1024.0 / 1024.0,
    })
}

/// Calculate directory size recursively
fn calculate_dir_size(path: &std::path::Path) -> std::io::Result<u64> {
    let mut size = 0;
    if path.is_file() {
        return Ok(std::fs::metadata(path)?.len());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                size += std::fs::metadata(&path)?.len();
            } else if path.is_dir() {
                size += calculate_dir_size(&path)?;
            }
        }
    }
    Ok(size)
}

/// Clean all app data (for uninstallation)
#[tauri::command]
#[specta::specta]
async fn cleanup_app_data(app_handle: AppHandle) -> Result<String, String> {
    use std::fs;

    let paths = get_app_data_paths();
    let mut cleaned = Vec::new();
    let mut errors = Vec::new();

    // Emit progress event
    let _ = app_handle.emit("cleanup-progress", serde_json::json!({
        "status": "starting",
        "message": "Starting cleanup..."
    }));

    for (name, path) in paths {
        if !path.exists() {
            continue;
        }

        let _ = app_handle.emit("cleanup-progress", serde_json::json!({
            "status": "cleaning",
            "message": format!("Removing {}...", name)
        }));

        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };

        match result {
            Ok(_) => {
                dev_log!("[CLEANUP] Removed: {}", path.display());
                cleaned.push(name);
            }
            Err(e) => {
                dev_log!("[CLEANUP] Failed to remove {}: {}", path.display(), e);
                errors.push(format!("{}: {}", name, e));
            }
        }
    }

    let _ = app_handle.emit("cleanup-progress", serde_json::json!({
        "status": "complete",
        "message": "Cleanup complete"
    }));

    if errors.is_empty() {
        Ok(format!(
            "Successfully cleaned {} items:\n• {}",
            cleaned.len(),
            cleaned.join("\n• ")
        ))
    } else {
        Err(format!(
            "Cleaned {} items but {} errors:\n{}",
            cleaned.len(),
            errors.len(),
            errors.join("\n")
        ))
    }
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
            let mut stt = state.stt_engine.lock().unwrap();
            *stt = Some(engine);
            dev_log!("[STT] ✅ STT engine reinitialized with model: {}", model_name);

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
    dev_log!("[SETUP] 🚀 Checking system dependencies...");

    // Cross-platform keyboard simulation via enigo - no install needed
    dev_log!("[SETUP] ✅ Text output: using enigo (cross-platform, no install needed)");

    // Linux-specific: evdev permissions for keyboard listening
    #[cfg(target_os = "linux")]
    {
        let evdev_ok = check_evdev_permissions();
        dev_log!(
            "[SETUP] evdev permissions: {}",
            if evdev_ok { "OK" } else { "needed" }
        );

        if !evdev_ok {
            dev_log!("[SETUP] Installing evdev permissions...");
            match install_evdev_permissions(app_handle.clone()).await {
                Ok(_) => {
                    dev_log!("[SETUP] ✅ evdev permissions installed");
                    return Ok(
                        "evdev permissions installed! Please restart the application.".to_string(),
                    );
                }
                Err(e) => {
                    return Err(format!("evdev installation failed: {}", e));
                }
            }
        }
    }

    // Windows/macOS: no special permissions needed
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app_handle; // Suppress unused warning
        dev_log!("[SETUP] ✅ No special permissions needed on this platform");
    }

    Ok("All dependencies ready ✅".to_string())
}

// ========== GNOME Focused Window Extension Commands (Linux only) ==========

/// Get the extension installation path
#[cfg(target_os = "linux")]
#[allow(dead_code)]
fn get_gnome_extension_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gnome-shell")
        .join("extensions")
        .join("focused-window-dbus@karaoketyper")
}

/// Check if the GNOME focused window extension is installed
#[tauri::command]
#[specta::specta]
async fn check_focused_window_extension() -> Result<GnomeExtensionStatus, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            use std::process::Command;

            let extension_path = get_gnome_extension_path();
            let is_installed = extension_path.join("extension.js").exists();

            let is_enabled = if is_installed {
                let (xdg_runtime, dbus_addr) = get_dbus_env();
                Command::new("gnome-extensions")
                    .args(["info", "focused-window-dbus@karaoketyper"])
                    .env("XDG_RUNTIME_DIR", &xdg_runtime)
                    .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                    .output()
                    .map(|o| {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        stdout.contains("State: ACTIVE") || stdout.contains("State: ENABLED")
                    })
                    .unwrap_or(false)
            } else {
                false
            };

            let is_gnome = std::env::var("XDG_CURRENT_DESKTOP")
                .map(|d| d.to_lowercase().contains("gnome"))
                .unwrap_or(false);

            let is_wayland = std::env::var("XDG_SESSION_TYPE")
                .map(|s| s == "wayland")
                .unwrap_or(false);

            GnomeExtensionStatus {
                installed: is_installed,
                enabled: is_enabled,
                is_gnome,
                is_wayland,
                path: extension_path.to_string_lossy().to_string(),
                needs_restart: is_installed && !is_enabled,
            }
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(GnomeExtensionStatus {
            installed: false,
            enabled: false,
            is_gnome: false,
            is_wayland: false,
            path: String::new(),
            needs_restart: false,
        })
    }
}

/// Install the GNOME focused window extension
#[tauri::command]
#[specta::specta]
async fn install_focused_window_extension(app_handle: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        use std::fs;
        use std::io::Write;

        // Get resource paths first (requires app_handle, but is fast)
        let resource_path = app_handle
            .path()
            .resource_dir()
            .map_err(|e| format!("Failed to get resource dir: {}", e))?
            .join("focused-window-dbus");

        let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("focused-window-dbus");

        let source_path = if resource_path.exists() {
            resource_path.clone()
        } else if dev_path.exists() {
            dev_path.clone()
        } else {
            return Err(format!(
                "Extension files not found. Checked:\n  - {}\n  - {}",
                resource_path.display(),
                dev_path.display()
            ));
        };

        // Now do blocking work in spawn_blocking
        tokio::task::spawn_blocking(move || {
            let extension_js = fs::read_to_string(source_path.join("extension.js"))
                .map_err(|e| format!("Failed to read extension.js: {}", e))?;
            let metadata_json = fs::read_to_string(source_path.join("metadata.json"))
                .map_err(|e| format!("Failed to read metadata.json: {}", e))?;

            let temp_dir = std::env::temp_dir();
            let zip_path = temp_dir.join("focused-window-dbus@karaoketyper.zip");

            let zip_file = fs::File::create(&zip_path)
                .map_err(|e| format!("Failed to create zip: {}", e))?;
            let mut zip = zip::ZipWriter::new(zip_file);

            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            zip.start_file("extension.js", options)
                .map_err(|e| format!("Failed to add extension.js to zip: {}", e))?;
            zip.write_all(extension_js.as_bytes())
                .map_err(|e| format!("Failed to write extension.js: {}", e))?;

            zip.start_file("metadata.json", options)
                .map_err(|e| format!("Failed to add metadata.json to zip: {}", e))?;
            zip.write_all(metadata_json.as_bytes())
                .map_err(|e| format!("Failed to write metadata.json: {}", e))?;

            zip.finish().map_err(|e| format!("Failed to finish zip: {}", e))?;

            dev_log!("[GNOME EXT] Created zip at: {}", zip_path.display());

            // Get D-Bus env for all GNOME commands in this block
            let (xdg_runtime, dbus_addr) = get_dbus_env();

            let install_result = std::process::Command::new("gnome-extensions")
                .args(["install", "--force", &zip_path.to_string_lossy()])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output()
                .map_err(|e| format!("Failed to run gnome-extensions install: {}", e))?;

            let _ = fs::remove_file(&zip_path);

            if !install_result.status.success() {
                let stderr = String::from_utf8_lossy(&install_result.stderr);
                return Err(format!("gnome-extensions install failed: {}", stderr));
            }

            dev_log!("[GNOME EXT] ✅ Extension installed via gnome-extensions");

            std::thread::sleep(std::time::Duration::from_millis(500));

            let enable_result = std::process::Command::new("gnome-extensions")
                .args(["enable", "focused-window-dbus@karaoketyper"])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output();

            let cli_enabled = matches!(&enable_result, Ok(output) if output.status.success());

            let ext_uuid = "focused-window-dbus@karaoketyper";
            let current = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.shell", "enabled-extensions"])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_else(|| "@as []".to_string());

            if !current.contains(ext_uuid) {
                let trimmed = current.trim();
                let new_list = if trimmed == "@as []" || trimmed == "[]" {
                    format!("['{}']", ext_uuid)
                } else {
                    let without_bracket = trimmed.trim_end_matches(']');
                    format!("{}, '{}']", without_bracket, ext_uuid)
                };

                dev_log!("[GNOME EXT] Setting enabled-extensions to: {}", new_list);
                let _ = std::process::Command::new("gsettings")
                    .args(["set", "org.gnome.shell", "enabled-extensions", &new_list])
                    .env("XDG_RUNTIME_DIR", &xdg_runtime)
                    .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                    .output();
            }

            if cli_enabled {
                dev_log!("[GNOME EXT] ✅ Extension enabled");

                dev_log!("[GNOME EXT] Initiating automatic logout...");
                let logout_result = std::process::Command::new("gnome-session-quit")
                    .args(["--logout", "--no-prompt"])
                    .env("XDG_RUNTIME_DIR", &xdg_runtime)
                    .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                    .spawn();

                match logout_result {
                    Ok(_) => {
                        dev_log!("[GNOME EXT] ✓ Logout command sent");
                        Ok("Extension installée et activée !\n\nDéconnexion en cours...".to_string())
                    },
                    Err(e) => {
                        dev_log!("[GNOME EXT] ⚠️ Logout failed: {}", e);
                        Ok("Extension installée et activée !\n\nVeuillez vous déconnecter manuellement pour activer l'extension.".to_string())
                    }
                }
            } else {
                Ok(
                    "Extension installée !\n\n\
                     Déconnectez-vous et reconnectez-vous pour l'activer."
                        .to_string(),
                )
            }
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))?
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = app_handle;
        Err("GNOME extensions are only available on Linux".to_string())
    }
}

/// Remove the GNOME focused window extension
#[tauri::command]
#[specta::specta]
async fn remove_focused_window_extension() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            use std::fs;

            let extension_path = get_gnome_extension_path();

            if !extension_path.exists() {
                return Ok("Extension was not installed".to_string());
            }

            let (xdg_runtime, dbus_addr) = get_dbus_env();
            let _ = std::process::Command::new("gnome-extensions")
                .args(["disable", "focused-window-dbus@karaoketyper"])
                .env("XDG_RUNTIME_DIR", &xdg_runtime)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .output();

            fs::remove_dir_all(&extension_path)
                .map_err(|e| format!("Failed to remove extension: {}", e))?;

            dev_log!(
                "[GNOME EXT] ✅ Extension removed: {}",
                extension_path.display()
            );

            Ok("Extension removed!\n\nLog out and back in to complete the removal.".to_string())
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))?
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("GNOME extensions are only available on Linux".to_string())
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
    let builder = tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            init_stt,
            start_recording,
            stop_recording,
            type_text,
            show_settings,
            show_overlay,
            hide_overlay,
            get_audio_devices,
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
            check_focused_window_extension,
            install_focused_window_extension,
            remove_focused_window_extension,
            get_autostart_enabled,
            set_autostart_enabled,
            get_app_data_info,
            cleanup_app_data,
            get_gpu_info,
            get_gpu_status,
            reset_gpu_failure_count
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
        .manage(AppState::default())
        .setup(|app| {
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
