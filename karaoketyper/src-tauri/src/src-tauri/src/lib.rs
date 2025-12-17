mod audio;
mod output;
mod stt;

use audio::AudioCapture;
use cpal::Stream;
use output::TextOutput;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use stt::STTEngine;
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    audio_stream: Arc<Mutex<Option<Stream>>>,
    audio_capture: Arc<Mutex<Option<AudioCapture>>>,
    stt_engine: Arc<Mutex<Option<STTEngine>>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusEvent {
    status: String,
    message: String,
}

#[tauri::command]
async fn init_stt(app_handle: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let model_path = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("models")
        .join("ggml-base.bin");

    if !model_path.exists() {
        return Err(format!(
            "Whisper model not found at {:?}. Please download it first.",
            model_path
        ));
    }

    let engine = STTEngine::new(model_path)?;
    *state.stt_engine.lock().unwrap() = Some(engine);

    Ok(())
}

#[tauri::command]
async fn start_recording(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "recording".to_string(),
                message: "Recording started".to_string(),
            },
        )
        .map_err(|e| format!("Failed to emit event: {}", e))?;

    let capture = AudioCapture::new(16000);
    let stream = capture.start_recording()?;

    *state.audio_capture.lock().unwrap() = Some(capture);
    *state.audio_stream.lock().unwrap() = Some(stream);

    Ok(())
}

#[tauri::command]
async fn stop_recording(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "processing".to_string(),
                message: "Processing audio...".to_string(),
            },
        )
        .map_err(|e| format!("Failed to emit event: {}", e))?;

    let capture_guard = state.audio_capture.lock().unwrap();
    let capture = capture_guard
        .as_ref()
        .ok_or("No active recording")?;

    capture.stop_recording();
    drop(state.audio_stream.lock().unwrap().take());

    let audio_buffer = capture.get_buffer();

    let stt_guard = state.stt_engine.lock().unwrap();
    let stt = stt_guard
        .as_ref()
        .ok_or("STT engine not initialized")?;

    let text = stt.transcribe(&audio_buffer)?;

    app_handle
        .emit(
            "status-changed",
            StatusEvent {
                status: "idle".to_string(),
                message: "Ready".to_string(),
            },
        )
        .map_err(|e| format!("Failed to emit event: {}", e))?;

    Ok(text)
}

#[tauri::command]
async fn type_text(text: String) -> Result<(), String> {
    let mut output = TextOutput::new(8)?;
    output.type_text(&text)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            init_stt,
            start_recording,
            stop_recording,
            type_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
