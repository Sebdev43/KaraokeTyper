mod audio;
mod autostart;
mod config;
mod keyboard_listener;
mod model_downloader;
mod output;
mod stt_parakeet;

// Linux-specific modules
#[cfg(target_os = "linux")]
mod evdev_listener;

#[cfg(target_os = "linux")]
mod audio_cli;

use audio::AudioCapture;
use config::AppConfig;
use output::TextOutput;
use std::sync::{Arc, Mutex};
use stt_parakeet::ParakeetSttEngine;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// STT engine wrapper (now only Parakeet/sherpa-onnx)
pub struct SttEngine {
    engine: ParakeetSttEngine,
}

impl SttEngine {
    pub fn new(config: &AppConfig) -> Result<Self, String> {
        let model_path = config.model_path.as_deref();
        let provider = config.stt_provider.as_deref();
        let engine = ParakeetSttEngine::new(model_path, &config.language, provider);

        if model_path.is_some() {
            engine.initialize().map_err(|e| e.to_string())?;
        }

        Ok(SttEngine { engine })
    }

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        self.engine
            .transcribe(audio_data)
            .map_err(|e| e.to_string())
    }

    pub fn backend_name(&self) -> &'static str {
        "sherpa-onnx Parakeet"
    }
}

struct StreamHandle {
    _stream: cpal::Stream,
}

unsafe impl Send for StreamHandle {}
unsafe impl Sync for StreamHandle {}

pub(crate) struct AppState {
    #[allow(dead_code)] // Used on non-Linux platforms
    audio_capture: Arc<Mutex<Option<AudioCapture>>>,
    #[allow(dead_code)] // Used on non-Linux platforms
    audio_stream: Arc<Mutex<Option<StreamHandle>>>,
    #[cfg(target_os = "linux")]
    audio_cli: Arc<Mutex<Option<audio_cli::AudioCaptureCli>>>,
    stt_engine: Arc<Mutex<Option<SttEngine>>>,
    config: Arc<Mutex<AppConfig>>,
    recording_ready: Arc<std::sync::atomic::AtomicBool>,
    pub recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            audio_capture: Arc::new(Mutex::new(None)),
            audio_stream: Arc::new(Mutex::new(None)),
            #[cfg(target_os = "linux")]
            audio_cli: Arc::new(Mutex::new(None)),
            stt_engine: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(AppConfig::load().unwrap_or_default())),
            recording_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            recording_start_time: Arc::new(Mutex::new(None)),
        }
    }
}

/// Get path to Python scripts (works in dev and production)
fn get_python_script_path(app_handle: &AppHandle, script_name: &str) -> Result<std::path::PathBuf, String> {
    // Try production path first (bundled resources)
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let prod_path = resource_dir.join("python").join(script_name);
        if prod_path.exists() {
            println!("[PATH] Using production path: {}", prod_path.display());
            return Ok(prod_path);
        }
    }

    // Fallback to dev path
    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("python")
        .join(script_name);
    if dev_path.exists() {
        println!("[PATH] Using dev path: {}", dev_path.display());
        return Ok(dev_path);
    }

    Err(format!(
        "Python script '{}' not found in production or dev paths",
        script_name
    ))
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
async fn init_stt(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let config = {
        let config = state.config.lock().unwrap();
        config.clone()
    };

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "initializing".to_string(),
                message: "Loading sherpa-onnx Parakeet model...".to_string(),
            },
        )
        .ok();

    println!(
        "[STT] Loading sherpa-onnx Parakeet model: {} (language: {})",
        config.model, config.language
    );

    let engine = SttEngine::new(&config).map_err(|e| {
        let error_msg = e.to_string();

        if error_msg.contains("Python not found") {
            "Python 3.8+ not found. Please install Python:\n  Linux: sudo apt install python3\n  macOS: brew install python3\n  Windows: Download from python.org".to_string()
        } else if error_msg.contains("sherpa") || error_msg.contains("onnx") {
            "sherpa-onnx not installed. Install from Settings → GPU & STT Backend.".to_string()
        } else if error_msg.contains("No model path") {
            "No model downloaded. Download a model from Settings → STT Models.".to_string()
        } else {
            error_msg
        }
    })?;

    println!("[STT] ✓ {} engine initialized", engine.backend_name());
    *state.stt_engine.lock().unwrap() = Some(engine);

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
async fn start_recording(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    do_start_recording(app_handle, &state).await
}

/// Helper function for start_recording - can be called from other modules
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
    println!(
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

    // On Linux: use audio_cli (parec) which works better with PipeWire
    #[cfg(target_os = "linux")]
    {
        let config = config::AppConfig::load()?;
        let capture = audio_cli::AudioCaptureCli::new();
        capture.start_recording(config.audio_device_name.as_deref())?;
        *state.audio_cli.lock().unwrap() = Some(capture);
        state.recording_ready.store(true, Ordering::SeqCst);
        println!(
            "[TIMING] T+{}ms: audio_cli started",
            t0.elapsed().as_millis()
        );
        Ok(())
    }

    // On Windows/macOS: use cpal
    #[cfg(not(target_os = "linux"))]
    {
        let config = config::AppConfig::load()?;
        let capture = AudioCapture::new(16000);
        let stream = capture.start_recording_with_device_name(
            config.audio_device_index,
            config.audio_device_name.as_deref(),
        )?;

        *state.audio_capture.lock().unwrap() = Some(capture);
        *state.audio_stream.lock().unwrap() = Some(StreamHandle { _stream: stream });

        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
        state.recording_ready.store(true, Ordering::SeqCst);
        println!("[RECORDING] ✅ cpal audio stream initialized and ready");
        Ok(())
    }
}

#[tauri::command]
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
    println!(
        "[TIMING] T+{}ms: do_stop_recording() entered",
        t0.elapsed().as_millis()
    );

    // Wait for recording to be ready (max 500ms)
    let mut waited = 0;
    while !state.recording_ready.load(Ordering::SeqCst) && waited < 500 {
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        waited += 10;
    }
    println!(
        "[TIMING] T+{}ms: waited {}ms for recording_ready",
        t0.elapsed().as_millis(),
        waited
    );

    if !state.recording_ready.load(Ordering::SeqCst) {
        println!("[RECORDING] ⚠️ Recording never became ready, proceeding anyway");
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

    println!(
        "[TIMING] T+{}ms: stopping audio capture...",
        t0.elapsed().as_millis()
    );

    // On Linux: use audio_cli (parec)
    #[cfg(target_os = "linux")]
    let audio_data = {
        let mut audio_cli = state.audio_cli.lock().unwrap();
        let capture = audio_cli.as_ref().ok_or("No active recording")?;
        let data = capture.stop_recording()?;
        *audio_cli = None;
        data
    };

    // On Windows/macOS: use cpal
    #[cfg(not(target_os = "linux"))]
    let audio_data = {
        let audio_capture = state.audio_capture.lock().unwrap();
        let capture = audio_capture.as_ref().ok_or("No active recording")?;
        capture.stop_recording();
        let data = capture.get_buffer();
        drop(audio_capture);
        *state.audio_stream.lock().unwrap() = None;
        data
    };

    println!(
        "[TIMING] T+{}ms: audio capture stopped, got {} samples ({:.2}s)",
        t0.elapsed().as_millis(),
        audio_data.len(),
        audio_data.len() as f32 / 16000.0
    );

    // DEBUG: Save audio to file for inspection (cross-platform temp dir)
    let debug_path = std::env::temp_dir().join("karaoketyper_debug.wav");
    if let Err(e) = save_debug_wav(&audio_data, &debug_path) {
        println!("[DEBUG] Failed to save debug WAV: {}", e);
    } else {
        println!(
            "[DEBUG] 🎵 Audio saved to: {} ({} samples, {:.2}s)",
            debug_path.display(),
            audio_data.len(),
            audio_data.len() as f32 / 16000.0
        );
    }
    println!("[TIMING] T+{}ms: debug WAV saved", t0.elapsed().as_millis());

    // Show first 20 samples for debugging
    if audio_data.len() >= 20 {
        println!("[DEBUG] First 20 samples: {:?}", &audio_data[..20]);
    }

    // Check for silence
    let non_zero = audio_data.iter().filter(|&&x| x.abs() > 0.001).count();
    println!(
        "[DEBUG] Non-silent samples: {}/{} ({:.1}%)",
        non_zero,
        audio_data.len(),
        100.0 * non_zero as f32 / audio_data.len().max(1) as f32
    );

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

    println!(
        "[TIMING] T+{}ms: starting transcription...",
        t0.elapsed().as_millis()
    );

    // Add 0.5 second of silence at the END to prevent Whisper from cutting off last words
    // This is a known issue with Whisper models - they can truncate the end of transcriptions
    let padding_samples = 8000; // 0.5s at 16kHz
    let mut padded_audio = audio_data.clone();
    padded_audio.extend(std::iter::repeat_n(0.0f32, padding_samples));
    println!(
        "[TIMING] T+{}ms: added {}ms padding at end ({} -> {} samples)",
        t0.elapsed().as_millis(),
        padding_samples * 1000 / 16000,
        audio_data.len(),
        padded_audio.len()
    );

    let stt_engine = state.stt_engine.lock().unwrap();
    let engine = stt_engine.as_ref().ok_or("STT engine not initialized")?;

    let transcription = engine
        .transcribe(&padded_audio)
        .map_err(|e| format!("Transcription error: {}", e))?;

    println!(
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
async fn show_settings(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn show_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn hide_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
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
    use std::process::Command;

    let mut devices = Vec::new();

    // Always add PipeWire default first (index 0) - this works with any PipeWire source
    devices.push(AudioDevice {
        name: "🎵 PipeWire Default".to_string(),
        raw_name: "pipewire".to_string(),
        index: 0,
    });

    // Use pactl to get real PipeWire sources
    let output = Command::new("pactl")
        .args(["list", "sources"])
        .output()
        .map_err(|e| format!("Failed to run pactl: {}", e))?;

    if !output.status.success() {
        println!("[DEVICES] pactl failed, returning default only");
        return Ok(devices);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut current_name: Option<String> = None;
    let mut current_description: Option<String> = None;
    let mut index = 1; // Start at 1 since pipewire is 0

    for line in stdout.lines() {
        let line = line.trim();

        // New source block
        if line.starts_with("Name:") {
            // Save previous device if complete
            if let (Some(name), Some(desc)) = (current_name.take(), current_description.take()) {
                // Skip monitor sources (they capture output audio, not mic input)
                if !name.contains(".monitor") {
                    let display_name = format_pipewire_device_name(&name, &desc);
                    devices.push(AudioDevice {
                        name: display_name,
                        raw_name: name,
                        index,
                    });
                    index += 1;
                }
            }
            current_name = Some(line.trim_start_matches("Name:").trim().to_string());
        } else if line.starts_with("Description:") {
            current_description = Some(line.trim_start_matches("Description:").trim().to_string());
        }
    }

    // Don't forget the last device
    if let (Some(name), Some(desc)) = (current_name, current_description) {
        if !name.contains(".monitor") {
            let display_name = format_pipewire_device_name(&name, &desc);
            devices.push(AudioDevice {
                name: display_name,
                raw_name: name,
                index,
            });
        }
    }

    println!("[DEVICES] Listed {} audio devices:", devices.len());
    for dev in &devices {
        println!(
            "[DEVICES]   [{}] {} -> {}",
            dev.index, dev.name, dev.raw_name
        );
    }

    Ok(devices)
}

#[cfg(target_os = "linux")]
fn format_pipewire_device_name(raw_name: &str, description: &str) -> String {
    // Bluetooth devices
    if raw_name.contains("bluez") {
        return format!("🎧 {}", description);
    }
    // Webcams
    if raw_name.contains("046d") || description.to_lowercase().contains("webcam") {
        return format!("📷 {}", description);
    }
    // USB microphones
    if raw_name.contains("usb-") || raw_name.contains("USB") {
        return format!("🎤 {}", description);
    }
    // Built-in audio
    if raw_name.contains("pci-") || raw_name.contains("alsa_input") {
        return format!("🔊 {}", description);
    }
    // Fallback
    format!("🎵 {}", description)
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
                index,
            })
        })
        .collect();

    if devices.is_empty() {
        return Err("No audio input devices found".to_string());
    }

    Ok(devices)
}

#[derive(Clone, serde::Serialize)]
struct AudioDevice {
    name: String,     // Display name for UI
    raw_name: String, // Actual cpal device name for audio.rs
    index: usize,     // Index in cpal device list
}

#[derive(Clone, serde::Serialize)]
struct StatusEvent {
    status: String,
    message: String,
}

#[tauri::command]
async fn load_settings(state: State<'_, AppState>) -> Result<AppConfig, String> {
    let config = state.config.lock().unwrap();
    Ok(config.clone())
}

#[tauri::command]
async fn save_settings(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    new_config: AppConfig,
) -> Result<(), String> {
    let old_shortcut = {
        let config = state.config.lock().unwrap();
        config.shortcut.clone()
    };

    new_config.save()?;

    {
        let mut config = state.config.lock().unwrap();
        *config = new_config.clone();
    }

    if old_shortcut != new_config.shortcut {
        update_global_shortcut(&app_handle, &old_shortcut, &new_config.shortcut)?;
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
        println!(
            "[SHORTCUT] Mouse button shortcut '{}' - handled by evdev, skipping Tauri registration",
            new_shortcut
        );
        return Ok(());
    }

    // For keyboard shortcuts, register with Tauri's global shortcut system
    // Note: This is mainly used for the overlay toggle, not for push-to-talk
    // Push-to-talk is handled by evdev on Linux for better Wayland compatibility
    println!(
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
async fn transcribe_audio(
    state: State<'_, AppState>,
    audio_data: Vec<f32>,
) -> Result<String, String> {
    println!(
        "[TRANSCRIBE] Received {} samples from frontend",
        audio_data.len()
    );

    if audio_data.is_empty() {
        return Err("No audio data received".to_string());
    }

    let duration = audio_data.len() as f32 / 16000.0;
    println!("[TRANSCRIBE] Audio duration: {:.2}s", duration);

    // Add 0.5 second of silence at the END to prevent Whisper from cutting off last words
    let padding_samples = 8000; // 0.5s at 16kHz
    let mut padded_audio = audio_data.clone();
    padded_audio.extend(std::iter::repeat_n(0.0f32, padding_samples));
    println!(
        "[TRANSCRIBE] Added {}ms padding at end ({} -> {} samples)",
        padding_samples * 1000 / 16000,
        audio_data.len(),
        padded_audio.len()
    );

    // Get STT engine
    let stt_engine = state.stt_engine.lock().unwrap();
    let engine = stt_engine.as_ref().ok_or("STT engine not initialized")?;

    // Transcribe the audio
    let transcription = engine
        .transcribe(&padded_audio)
        .map_err(|e| format!("Transcription error: {}", e))?;

    println!("[TRANSCRIBE] Result: '{}'", transcription);
    Ok(transcription)
}

#[tauri::command]
fn check_evdev_permissions() -> bool {
    use std::path::Path;
    let rules_file = Path::new("/etc/udev/rules.d/70-karaoketyper.rules");
    rules_file.exists()
}

#[tauri::command]
async fn install_evdev_permissions(app_handle: AppHandle) -> Result<String, String> {
    use tauri_plugin_shell::ShellExt;

    println!("[EVDEV INSTALL] Starting installation...");

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

    println!("[EVDEV INSTALL] Running script: {}", script_path.display());

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
                println!("[EVDEV INSTALL] Success: {}", stdout);
                Ok(
                    "evdev permissions installed successfully! Please restart the application."
                        .to_string(),
                )
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                eprintln!("[EVDEV INSTALL] Failed: {}", stderr);
                Err(format!("Installation failed: {}", stderr))
            }
        }
        Err(e) => {
            eprintln!("[EVDEV INSTALL] Command error: {}", e);
            Err(format!("Failed to execute installation: {}", e))
        }
    }
}

/// Check if keyboard input permissions are available (cross-platform)
#[tauri::command]
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
fn log_from_frontend(message: String) {
    println!("[FRONTEND] {}", message);
}

/// Check if Python 3.8+ is installed
#[tauri::command]
fn check_python_available() -> bool {
    use std::process::Command;

    // Try python3 first, then python
    for python_cmd in &["python3", "python"] {
        if let Ok(output) = Command::new(python_cmd).args(["--version"]).output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                println!(
                    "[PYTHON CHECK] {} --version: {}",
                    python_cmd,
                    version.trim()
                );
                // Check version >= 3.8
                if version.contains("3.8")
                    || version.contains("3.9")
                    || version.contains("3.10")
                    || version.contains("3.11")
                    || version.contains("3.12")
                    || version.contains("3.13")
                {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if sherpa-onnx is installed (for Parakeet STT)
#[tauri::command]
fn check_sherpa_onnx_installed() -> bool {
    use std::process::Command;

    let venv_python = get_venv_python();
    if venv_python.exists() {
        if let Ok(output) = Command::new(&venv_python)
            .args(["-c", "import sherpa_onnx; print(sherpa_onnx.__version__)"])
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                println!(
                    "[SHERPA CHECK] sherpa-onnx version (venv): {}",
                    version.trim()
                );
                return true;
            }
        }
    }

    // Fallback: check system Python
    for python_cmd in &["python3", "python"] {
        if let Ok(output) = Command::new(python_cmd)
            .args(["-c", "import sherpa_onnx; print(sherpa_onnx.__version__)"])
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                println!(
                    "[SHERPA CHECK] sherpa-onnx version (system): {}",
                    version.trim()
                );
                return true;
            }
        }
    }
    false
}

/// Detect GPU hardware and recommend STT backend
#[tauri::command]
async fn detect_gpu(app_handle: AppHandle) -> Result<serde_json::Value, String> {
    use std::process::Command;

    // Get path to gpu_detector.py (works in dev and production)
    let detector_path = get_python_script_path(&app_handle, "gpu_detector.py")?;

    // Try venv python first, fallback to system
    let python_cmd = {
        let venv_python = get_venv_python();
        if venv_python.exists() {
            venv_python.to_string_lossy().to_string()
        } else {
            "python3".to_string()
        }
    };

    let output = Command::new(&python_cmd)
        .args([detector_path.to_str().unwrap(), "detect"])
        .output()
        .map_err(|e| format!("Failed to run GPU detector: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("GPU detection failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse GPU detection result: {}", e))?;

    println!(
        "[GPU DETECT] Result: {}",
        serde_json::to_string_pretty(&result).unwrap_or_default()
    );

    Ok(result)
}

/// Get installation commands for a specific backend
#[tauri::command]
async fn get_stt_install_commands(
    app_handle: AppHandle,
    backend: String,
) -> Result<serde_json::Value, String> {
    use std::process::Command;

    // Get path to gpu_detector.py (works in dev and production)
    let detector_path = get_python_script_path(&app_handle, "gpu_detector.py")?;

    let python_cmd = {
        let venv_python = get_venv_python();
        if venv_python.exists() {
            venv_python.to_string_lossy().to_string()
        } else {
            "python3".to_string()
        }
    };

    let output = Command::new(&python_cmd)
        .args([
            detector_path.to_str().unwrap(),
            "install-commands",
            &backend,
        ])
        .output()
        .map_err(|e| format!("Failed to get install commands: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to get install commands: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse install commands: {}", e))?;

    Ok(result)
}

/// Install STT dependencies based on detected GPU (sherpa-onnx + appropriate runtime)
#[tauri::command]
async fn install_stt_dependencies(
    app_handle: AppHandle,
    backend: String,
) -> Result<String, String> {
    use std::process::Command;
    use tauri::Emitter;

    println!(
        "[STT INSTALL] Starting installation for backend: {}",
        backend
    );

    let venv_path = get_venv_path();
    let venv_python = get_venv_python();

    #[cfg(target_os = "windows")]
    let venv_pip = venv_path.join("Scripts").join("pip.exe");
    #[cfg(not(target_os = "windows"))]
    let venv_pip = venv_path.join("bin").join("pip");

    // Find system Python command
    let python_cmd = if Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        "python3"
    } else {
        "python"
    };

    // Step 1: Create venv if needed
    if !venv_python.exists() {
        println!("[STT INSTALL] Creating venv at: {}", venv_path.display());

        app_handle
            .emit(
                "stt-install-progress",
                serde_json::json!({
                    "status": "installing",
                    "step": 1,
                    "total_steps": 4,
                    "message": "Creating Python virtual environment..."
                }),
            )
            .ok();

        if let Some(parent) = venv_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        let venv_output = Command::new(python_cmd)
            .args(["-m", "venv", venv_path.to_str().unwrap()])
            .output()
            .map_err(|e| format!("Failed to create venv: {}", e))?;

        if !venv_output.status.success() {
            let stderr = String::from_utf8_lossy(&venv_output.stderr);
            return Err(format!("Failed to create venv: {}", stderr));
        }
    }

    // Step 2: Upgrade pip
    app_handle
        .emit(
            "stt-install-progress",
            serde_json::json!({
                "status": "installing",
                "step": 2,
                "total_steps": 4,
                "message": "Upgrading pip..."
            }),
        )
        .ok();

    let _ = Command::new(&venv_python)
        .args(["-m", "pip", "install", "--upgrade", "pip"])
        .output();

    // Step 3: Install base packages
    app_handle
        .emit(
            "stt-install-progress",
            serde_json::json!({
                "status": "installing",
                "step": 3,
                "total_steps": 4,
                "message": "Installing base dependencies..."
            }),
        )
        .ok();

    let base_packages = ["numpy", "soundfile", "huggingface_hub"];
    for pkg in &base_packages {
        let _ = Command::new(&venv_pip).args(["install", pkg]).output();
    }

    // Step 4: Install backend-specific packages
    let onnx_package = match backend.as_str() {
        "CUDAExecutionProvider" => {
            app_handle
                .emit(
                    "stt-install-progress",
                    serde_json::json!({
                        "status": "installing",
                        "step": 4,
                        "total_steps": 4,
                        "message": "Installing ONNX Runtime GPU (CUDA)..."
                    }),
                )
                .ok();
            "onnxruntime-gpu"
        }
        "MIGraphXExecutionProvider" | "ROCMExecutionProvider" => {
            app_handle
                .emit(
                    "stt-install-progress",
                    serde_json::json!({
                        "status": "installing",
                        "step": 4,
                        "total_steps": 4,
                        "message": "Installing ONNX Runtime ROCm (AMD)..."
                    }),
                )
                .ok();
            // Note: onnxruntime-rocm might need special index
            "onnxruntime" // Fallback to CPU for now, ROCm needs manual setup
        }
        "CoreMLExecutionProvider" => {
            app_handle
                .emit(
                    "stt-install-progress",
                    serde_json::json!({
                        "status": "installing",
                        "step": 4,
                        "total_steps": 4,
                        "message": "Installing ONNX Runtime (Apple Silicon)..."
                    }),
                )
                .ok();
            "onnxruntime" // CoreML is included in base onnxruntime on macOS
        }
        _ => {
            app_handle
                .emit(
                    "stt-install-progress",
                    serde_json::json!({
                        "status": "installing",
                        "step": 4,
                        "total_steps": 4,
                        "message": "Installing ONNX Runtime (CPU)..."
                    }),
                )
                .ok();
            "onnxruntime"
        }
    };

    // Install ONNX Runtime
    let onnx_output = Command::new(&venv_pip)
        .args(["install", onnx_package])
        .output()
        .map_err(|e| format!("Failed to install {}: {}", onnx_package, e))?;

    if !onnx_output.status.success() {
        let stderr = String::from_utf8_lossy(&onnx_output.stderr);
        println!(
            "[STT INSTALL] Warning: {} install issue: {}",
            onnx_package, stderr
        );
    }

    // Install sherpa-onnx
    app_handle
        .emit(
            "stt-install-progress",
            serde_json::json!({
                "status": "installing",
                "step": 4,
                "total_steps": 4,
                "message": "Installing sherpa-onnx..."
            }),
        )
        .ok();

    let sherpa_output = Command::new(&venv_pip)
        .args(["install", "sherpa-onnx"])
        .output()
        .map_err(|e| format!("Failed to install sherpa-onnx: {}", e))?;

    if sherpa_output.status.success() {
        app_handle
            .emit(
                "stt-install-progress",
                serde_json::json!({
                    "status": "success",
                    "message": "STT dependencies installed successfully!"
                }),
            )
            .ok();

        println!(
            "[STT INSTALL] ✅ Installation complete for backend: {}",
            backend
        );
        Ok(format!(
            "STT dependencies installed for {}! Restart to use.",
            backend
        ))
    } else {
        let stderr = String::from_utf8_lossy(&sherpa_output.stderr);
        let stdout = String::from_utf8_lossy(&sherpa_output.stdout);

        app_handle
            .emit(
                "stt-install-progress",
                serde_json::json!({
                    "status": "error",
                    "message": format!("Installation failed: {}", stderr)
                }),
            )
            .ok();

        Err(format!(
            "sherpa-onnx installation failed:\nstderr: {}\nstdout: {}",
            stderr, stdout
        ))
    }
}

/// Get system information for Settings UI
#[tauri::command]
async fn get_system_info() -> Result<serde_json::Value, String> {
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

    Ok(serde_json::json!({
        "os": os_name,
        "arch": arch,
        "display_server": display_server,
        "desktop_environment": desktop_env,
        "venv_path": get_venv_path().to_string_lossy(),
        "venv_exists": get_venv_python().exists()
    }))
}

/// Get the path to the app's Python venv
pub fn get_venv_path() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("karaoketyper")
        .join("venv")
}

/// Get the path to the venv's Python executable
pub fn get_venv_python() -> std::path::PathBuf {
    let venv = get_venv_path();
    #[cfg(target_os = "windows")]
    return venv.join("Scripts").join("python.exe");
    #[cfg(not(target_os = "windows"))]
    venv.join("bin").join("python")
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
fn get_app_data_info() -> Result<serde_json::Value, String> {
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

        items.push(serde_json::json!({
            "name": name,
            "path": path.to_string_lossy(),
            "exists": exists,
            "size_mb": size as f64 / 1024.0 / 1024.0
        }));
    }

    Ok(serde_json::json!({
        "items": items,
        "total_size_mb": total_size as f64 / 1024.0 / 1024.0
    }))
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
                println!("[CLEANUP] Removed: {}", path.display());
                cleaned.push(name);
            }
            Err(e) => {
                eprintln!("[CLEANUP] Failed to remove {}: {}", path.display(), e);
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
async fn get_installed_models() -> Result<Vec<model_downloader::ModelInfo>, String> {
    model_downloader::check_installed_models()
}

/// Download a model with progress events
#[tauri::command]
async fn download_model(app_handle: AppHandle, model_name: String) -> Result<(), String> {
    model_downloader::download_model(app_handle, model_name).await
}

/// Install system dependencies (Linux only - handles evdev permissions)
#[tauri::command]
async fn install_system_dependencies(app_handle: AppHandle) -> Result<String, String> {
    println!("[SETUP] 🚀 Checking system dependencies...");

    // Cross-platform keyboard simulation via enigo - no install needed
    println!("[SETUP] ✅ Text output: using enigo (cross-platform, no install needed)");

    // Linux-specific: evdev permissions for keyboard listening
    #[cfg(target_os = "linux")]
    {
        let evdev_ok = check_evdev_permissions();
        println!(
            "[SETUP] evdev permissions: {}",
            if evdev_ok { "OK" } else { "needed" }
        );

        if !evdev_ok {
            println!("[SETUP] Installing evdev permissions...");
            match install_evdev_permissions(app_handle.clone()).await {
                Ok(_) => {
                    println!("[SETUP] ✅ evdev permissions installed");
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
        println!("[SETUP] ✅ No special permissions needed on this platform");
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
async fn check_focused_window_extension() -> Result<serde_json::Value, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            use std::process::Command;

            let extension_path = get_gnome_extension_path();
            let is_installed = extension_path.join("extension.js").exists();

            let is_enabled = if is_installed {
                Command::new("gnome-extensions")
                    .args(["info", "focused-window-dbus@karaoketyper"])
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

            serde_json::json!({
                "installed": is_installed,
                "enabled": is_enabled,
                "is_gnome": is_gnome,
                "is_wayland": is_wayland,
                "path": extension_path.to_string_lossy(),
                "needs_restart": is_installed && !is_enabled
            })
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(serde_json::json!({
            "installed": false,
            "enabled": false,
            "is_gnome": false,
            "is_wayland": false,
            "path": "",
            "needs_restart": false
        }))
    }
}

/// Install the GNOME focused window extension
#[tauri::command]
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

            println!("[GNOME EXT] Created zip at: {}", zip_path.display());

            let install_result = std::process::Command::new("gnome-extensions")
                .args(["install", "--force", &zip_path.to_string_lossy()])
                .output()
                .map_err(|e| format!("Failed to run gnome-extensions install: {}", e))?;

            let _ = fs::remove_file(&zip_path);

            if !install_result.status.success() {
                let stderr = String::from_utf8_lossy(&install_result.stderr);
                return Err(format!("gnome-extensions install failed: {}", stderr));
            }

            println!("[GNOME EXT] ✅ Extension installed via gnome-extensions");

            std::thread::sleep(std::time::Duration::from_millis(500));

            let enable_result = std::process::Command::new("gnome-extensions")
                .args(["enable", "focused-window-dbus@karaoketyper"])
                .output();

            let cli_enabled = matches!(&enable_result, Ok(output) if output.status.success());

            let ext_uuid = "focused-window-dbus@karaoketyper";
            let current = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.shell", "enabled-extensions"])
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

                println!("[GNOME EXT] Setting enabled-extensions to: {}", new_list);
                let _ = std::process::Command::new("gsettings")
                    .args(["set", "org.gnome.shell", "enabled-extensions", &new_list])
                    .output();
            }

            if cli_enabled {
                println!("[GNOME EXT] ✅ Extension enabled");

                println!("[GNOME EXT] Initiating automatic logout...");
                let logout_result = std::process::Command::new("gnome-session-quit")
                    .args(["--logout", "--no-prompt"])
                    .spawn();

                match logout_result {
                    Ok(_) => {
                        println!("[GNOME EXT] ✓ Logout command sent");
                        Ok("Extension installée et activée !\n\nDéconnexion en cours...".to_string())
                    },
                    Err(e) => {
                        println!("[GNOME EXT] ⚠️ Logout failed: {}", e);
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
async fn remove_focused_window_extension() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            use std::fs;

            let extension_path = get_gnome_extension_path();

            if !extension_path.exists() {
                return Ok("Extension was not installed".to_string());
            }

            let _ = std::process::Command::new("gnome-extensions")
                .args(["disable", "focused-window-dbus@karaoketyper"])
                .output();

            fs::remove_dir_all(&extension_path)
                .map_err(|e| format!("Failed to remove extension: {}", e))?;

            println!(
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
fn get_autostart_enabled() -> bool {
    autostart::is_autostart_enabled()
}

#[tauri::command]
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
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .setup(|app| {
            // Auto-setup venv on first launch
            let venv_python = get_venv_python();
            if !venv_python.exists() {
                println!("[STARTUP] First launch detected - setting up Python venv...");
                let app_handle_venv = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match install_stt_dependencies(app_handle_venv, "CPU".to_string()).await {
                        Ok(msg) => println!("[STARTUP] ✅ Venv setup complete: {}", msg),
                        Err(e) => eprintln!("[STARTUP] ⚠️  Venv setup failed: {}", e),
                    }
                });
            } else {
                println!("[STARTUP] Venv already exists at: {}", venv_python.display());
            }

            // Initialize STT engine at startup
            println!("[STARTUP] Initializing STT engine...");
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = app_handle.state::<AppState>();
                match init_stt(app_handle.clone(), state).await {
                    Ok(_) => println!("[STARTUP] ✅ STT engine initialized successfully"),
                    Err(e) => {
                        eprintln!("[STARTUP] ⚠️  STT initialization skipped: {}", e);
                        eprintln!("[STARTUP] Open Settings to configure STT backend");
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
            println!("[SHORTCUT DEBUG] Display server: {}", display_server);

            if display_server == "wayland" {
                println!("[SHORTCUT WARNING] ⚠️  Running on Wayland - global shortcuts may not work");
                println!("[SHORTCUT WARNING] Modifiers (Ctrl, Alt) are often blocked by Wayland compositor");
                println!("[SHORTCUT WARNING] Consider using X11 session or simple F-key shortcuts");
            }

            let shortcut = {
                let state = app.state::<AppState>();
                let config = state.config.lock().unwrap();
                config.shortcut.clone()
            };

            println!("[SHORTCUT DEBUG] Loading primary shortcut from config: '{}'", shortcut);
            println!("[SHORTCUT DEBUG] ⚠️  Ctrl+Space handled by evdev only - Global Shortcuts disabled");

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
                        println!("[SHORTCUT DEBUG] F10 pressed - showing settings");
                        if let Some(settings_win) = app_handle.get_webview_window("settings") {
                            let _ = settings_win.show();
                            let _ = settings_win.set_focus();
                        }
                    }
                }) {
                    Ok(_) => {
                        println!("[SHORTCUT DEBUG] ✅ Successfully registered shortcut: '{}'", shortcut_name);
                        registered_shortcuts.push(shortcut_name);
                    }
                    Err(e) => {
                        eprintln!("[SHORTCUT WARNING] ⚠️  Failed to register shortcut '{}': {}", shortcut_name, e);
                    }
                }
            }

            if registered_shortcuts.is_empty() {
                eprintln!("[SHORTCUT WARNING] ⚠️  Failed to register ANY global shortcuts!");
                if display_server == "wayland" {
                    eprintln!("[SHORTCUT INFO] On Wayland, global F-key shortcuts are often blocked by other apps.");
                    eprintln!("[SHORTCUT INFO] Don't worry - evdev handles Ctrl+Space for push-to-talk! 👍");
                } else {
                    eprintln!("[SHORTCUT INFO] No global shortcuts available, but evdev handles Ctrl+Space! 👍");
                }
            } else {
                println!("[SHORTCUT DEBUG] 📋 Active shortcuts: {:?}", registered_shortcuts);
                if display_server == "wayland" && !registered_shortcuts.contains(&shortcut) {
                    println!("[SHORTCUT WARNING] ⚠️  Your configured shortcut '{}' doesn't work on Wayland", shortcut);
                    println!("[SHORTCUT WARNING] Using fallback: {:?}", registered_shortcuts);
                }
            }

            // Start keyboard listener for push-to-talk (cross-platform)
            // - Linux: uses evdev for best Wayland support
            // - Windows/macOS: uses rdev
            // NOTE: Delayed start to avoid Bluetooth audio interference during device enumeration
            println!("[KEYBOARD] Starting push-to-talk listener (delayed)...");
            let app_handle_keyboard = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                // Delay startup to let Bluetooth audio devices stabilize
                // This prevents crackling/noise when the app launches with Bluetooth headsets
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                match keyboard_listener::start_keyboard_listener(app_handle_keyboard).await {
                    Ok(_) => {
                        println!("[KEYBOARD] ✅ Keyboard listener active");
                    }
                    Err(e) => {
                        eprintln!("[KEYBOARD] ❌ Keyboard listener failed: {}", e);
                        eprintln!("[KEYBOARD] Push-to-talk will not work.");

                        #[cfg(target_os = "linux")]
                        {
                            eprintln!("[KEYBOARD] Add user to 'input' group:");
                            eprintln!("[KEYBOARD]   sudo usermod -aG input $USER");
                            eprintln!("[KEYBOARD]   Then log out and back in.");
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
                println!("[WINDOW] Close requested for window: {}", label);

                // Hide window instead of closing (tray app behavior)
                if let Err(e) = window.hide() {
                    eprintln!("[WINDOW] Failed to hide window {}: {}", label, e);
                } else {
                    println!("[WINDOW] Window {} hidden successfully", label);
                }
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
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
            check_python_available,
            check_sherpa_onnx_installed,
            detect_gpu,
            get_stt_install_commands,
            install_stt_dependencies,
            get_system_info,
            check_focused_window_extension,
            install_focused_window_extension,
            remove_focused_window_extension,
            get_autostart_enabled,
            set_autostart_enabled,
            get_app_data_info,
            cleanup_app_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
