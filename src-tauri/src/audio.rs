//! Audio capture module - CPAL (cross-platform)
//!
//! Uses CPAL for audio capture on all platforms:
//! - Linux: ALSA backend (transparent PipeWire support via pipewire-alsa)
//! - Windows: WASAPI backend
//! - macOS: CoreAudio backend
//!
//! Architecture (worker thread — inspired by Handy):
//! - `open()` spawns a worker thread that builds the CPAL stream and starts it playing.
//!   Audio samples flow continuously through an mpsc channel into the worker.
//! - `start_recording()` sends `Cmd::Start` — the worker clears its buffer and begins
//!   accumulating samples. This is instant regardless of stream state.
//! - `stop_recording()` sends `Cmd::Stop` — the worker drains remaining samples,
//!   runs VAD flush, and returns the buffer via a reply channel.
//! - `close()` sends `Cmd::Shutdown` and joins the worker thread.
//!
//! This architecture eliminates PTT race conditions: commands are queued and processed
//! only after the stream is fully open, because `run_worker()` only starts its loop
//! after `stream.play()` succeeds.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use crate::vad::{SmoothedVad, VadFrame, VoiceActivityDetector, SILERO_FRAME_SAMPLES};

// ── Commands processed by the worker thread ──────────────────────────────────

enum Cmd {
    Start,
    Stop(mpsc::Sender<Vec<f32>>),
    Shutdown,
}

// ── Scope guard (RAII cleanup on drop, including panic unwind) ────────────────

struct ScopeGuard<T, F: FnOnce(T)> {
    value: Option<T>,
    f: Option<F>,
}

impl<T, F: FnOnce(T)> Drop for ScopeGuard<T, F> {
    fn drop(&mut self) {
        if let (Some(v), Some(f)) = (self.value.take(), self.f.take()) {
            f(v);
        }
    }
}

fn scopeguard<T, F: FnOnce(T)>(value: T, f: F) -> ScopeGuard<T, F> {
    ScopeGuard {
        value: Some(value),
        f: Some(f),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Validate device names to prevent command injection via pactl
fn is_valid_device_name(name: &str) -> bool {
    name.chars().all(|c| c.is_alphanumeric() || "_.:-,=".contains(c))
}

/// Delay before auto-closing the stream after recording stops (releases mic device)
const STREAM_AUTO_CLOSE_SECS: u64 = 10;

// ── Public API ───────────────────────────────────────────────────────────────

/// Audio capture using a dedicated worker thread.
///
/// The worker owns the CPAL stream and processes samples continuously.
/// Commands (`Start`/`Stop`/`Shutdown`) are sent via an mpsc channel.
pub struct AudioCapture {
    target_sample_rate: u32,
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    /// VAD engine — shared with the worker via Arc<Mutex<>>
    vad: Arc<Mutex<Option<SmoothedVad>>>,
    /// Whether the worker is currently open (stream active)
    is_open: Arc<AtomicBool>,
    /// Whether current device is Bluetooth (for auto-close logic)
    is_bluetooth: AtomicBool,
    /// Generation counter for Bluetooth auto-close timer
    stop_generation: Arc<AtomicU64>,
}

impl AudioCapture {
    pub fn new(target_sample_rate: u32) -> Self {
        Self {
            target_sample_rate,
            cmd_tx: None,
            worker_handle: None,
            vad: Arc::new(Mutex::new(None)),
            is_open: Arc::new(AtomicBool::new(false)),
            is_bluetooth: AtomicBool::new(false),
            stop_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Set the Voice Activity Detection engine.
    pub fn set_vad(&self, vad: SmoothedVad) {
        *self.vad.lock().unwrap() = Some(vad);
    }

    /// Check if a stream is currently open.
    pub fn is_stream_open(&self) -> bool {
        self.is_open.load(Ordering::SeqCst)
    }

    /// Open the audio stream for a device.
    ///
    /// Spawns a worker thread that:
    /// 1. Resolves the CPAL device (slow for ALSA — ~1-2s for enumeration)
    /// 2. Builds and starts the input stream
    /// 3. Runs the worker loop processing samples and commands
    ///
    /// Device matching:
    /// - `None`, `"pipewire"`, `"default"`, `""` → system default (fast, no enumeration)
    /// - `"plughw:CARD=..."` → ALSA hardware device (direct CPAL open)
    /// - PipeWire source name (bluez, alsa_input, etc.) → route via pactl + CPAL default
    pub fn open_stream(&mut self, device_name: Option<&str>) -> Result<(), String> {
        // Close any existing worker first
        self.close_stream();

        // Track if this is a Bluetooth device (for auto-close logic)
        let is_bt = device_name.map(|n| n.starts_with("bluez_input.")).unwrap_or(false);
        self.is_bluetooth.store(is_bt, Ordering::Relaxed);

        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>();

        let target_rate = self.target_sample_rate;
        let vad = Arc::clone(&self.vad);
        let is_open = Arc::clone(&self.is_open);
        let device_name_owned = device_name.map(|s| s.to_string());

        let worker = std::thread::spawn(move || {
            // Guard: ensure is_open is always set to false on exit (normal or panic)
            let _guard = scopeguard(Arc::clone(&is_open), |flag| {
                flag.store(false, Ordering::SeqCst);
                crate::dev_log!("[AUDIO/WORKER] Worker thread exited, stream released");
            });

            // ── Step 1: Resolve device (can be slow for ALSA) ──
            let device = match resolve_device(device_name_owned.as_deref()) {
                Ok(d) => d,
                Err(e) => {
                    crate::dev_log!("[AUDIO/WORKER] Failed to resolve device: {}", e);
                    return;
                }
            };

            let device_display = device.name().unwrap_or_else(|_| "unknown".to_string());
            crate::dev_log!("[AUDIO/WORKER] Selected device: {}", device_display);

            // ── Step 2: Negotiate config and build stream ──
            let (config, actual_rate) = match negotiate_config(&device, target_rate) {
                Ok(c) => c,
                Err(e) => {
                    crate::dev_log!("[AUDIO/WORKER] Config negotiation failed: {}", e);
                    return;
                }
            };

            crate::dev_log!(
                "[AUDIO/WORKER] Stream config: {}ch, {}Hz (target: {}Hz)",
                config.channels,
                actual_rate,
                target_rate
            );

            let sample_tx_clone = sample_tx;
            let stream = match device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Resample if needed, then send to worker
                    let samples = if actual_rate != target_rate {
                        downsample(data, actual_rate, target_rate)
                    } else {
                        data.to_vec()
                    };
                    let _ = sample_tx_clone.send(samples);
                },
                |err| {
                    crate::dev_log!("[AUDIO/WORKER] Stream error: {}", err);
                },
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    crate::dev_log!("[AUDIO/WORKER] Failed to build stream: {}", e);
                    return;
                }
            };

            // ── Step 3: Start playing (mic is ON, samples flow) ──
            if let Err(e) = stream.play() {
                crate::dev_log!("[AUDIO/WORKER] Failed to play stream: {}", e);
                return;
            }

            is_open.store(true, Ordering::SeqCst);
            crate::dev_log!(
                "[AUDIO/WORKER] Stream opened and playing at {}Hz from '{}'",
                actual_rate,
                device_display
            );

            // ── Step 4: Worker loop — process samples + commands ──
            run_worker(vad, sample_rx, cmd_rx);

            // _guard drop sets is_open=false and logs on exit (including panic unwind)
        });

        self.cmd_tx = Some(cmd_tx);
        self.worker_handle = Some(worker);

        // Note: we return immediately. The worker thread opens the device asynchronously.
        // Commands sent before the worker loop starts are queued in the mpsc channel and
        // processed as soon as the stream is ready. This eliminates the race condition.
        Ok(())
    }

    /// Close the audio stream and release the device.
    pub fn close_stream(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(h) = self.worker_handle.take() {
            if let Err(panic_payload) = h.join() {
                crate::dev_log!(
                    "[AUDIO/WORKER] Worker thread panicked: {:?}",
                    panic_payload.downcast_ref::<&str>().unwrap_or(&"unknown panic")
                );
            }
        }
        self.is_open.store(false, Ordering::SeqCst);
    }

    /// Start recording — sends `Cmd::Start` to the worker (instant).
    /// The worker clears its buffer and begins accumulating samples.
    pub fn start_recording(&self) -> Result<(), String> {
        // Cancel any pending Bluetooth auto-close timer
        self.stop_generation.fetch_add(1, Ordering::SeqCst);

        let tx = self.cmd_tx.as_ref().ok_or("Audio worker not running")?;
        tx.send(Cmd::Start)
            .map_err(|_| "Audio worker channel closed".to_string())?;

        crate::dev_log!("[AUDIO/CPAL] Start command sent to worker");
        Ok(())
    }

    /// Send a Stop command to the worker. Returns a `StopHandle` to wait for results.
    ///
    /// This is a non-blocking send — call `StopHandle::wait()` AFTER dropping the Mutex
    /// to avoid holding it during the blocking recv (up to 5s).
    pub fn stop_recording(&self) -> Result<StopHandle, String> {
        let tx = self.cmd_tx.as_ref().ok_or("Audio worker not running")?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(Cmd::Stop(reply_tx))
            .map_err(|_| "Audio worker channel closed".to_string())?;

        // Schedule Bluetooth auto-close if applicable
        if self.is_bluetooth.load(Ordering::Relaxed) {
            let gen = self.stop_generation.fetch_add(1, Ordering::SeqCst) + 1;
            let gen_ref = Arc::clone(&self.stop_generation);
            let is_open = Arc::clone(&self.is_open);
            let cmd_tx = self.cmd_tx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(STREAM_AUTO_CLOSE_SECS));
                if gen_ref.load(Ordering::SeqCst) == gen && is_open.load(Ordering::SeqCst) {
                    if let Some(tx) = cmd_tx {
                        let _ = tx.send(Cmd::Shutdown);
                    }
                    is_open.store(false, Ordering::SeqCst);
                    crate::dev_log!(
                        "[AUDIO/CPAL] BT auto-close: sent Shutdown after {}s idle",
                        STREAM_AUTO_CLOSE_SECS
                    );
                }
            });

            crate::dev_log!(
                "[AUDIO/CPAL] Will auto-close in {}s if idle (BT device)",
                STREAM_AUTO_CLOSE_SECS
            );
        }

        Ok(StopHandle { reply_rx })
    }

}

/// Handle returned by `stop_recording()`. Call `wait()` to get the audio buffer.
/// This lets you drop the `AudioCapture` Mutex before blocking on the reply.
pub struct StopHandle {
    reply_rx: mpsc::Receiver<Vec<f32>>,
}

impl StopHandle {
    /// Wait for the worker to return the audio buffer (blocks up to 5s).
    pub fn wait(self) -> Result<Vec<f32>, String> {
        let audio_data = self
            .reply_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "Timeout waiting for audio data from worker".to_string())?;

        crate::dev_log!(
            "[AUDIO/CPAL] Recording stopped: {} samples ({:.2}s at 16000Hz)",
            audio_data.len(),
            audio_data.len() as f32 / 16000.0
        );

        Ok(audio_data)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.close_stream();
    }
}

// ── Worker thread ────────────────────────────────────────────────────────────

fn run_worker(
    vad: Arc<Mutex<Option<SmoothedVad>>>,
    sample_rx: mpsc::Receiver<Vec<f32>>,
    cmd_rx: mpsc::Receiver<Cmd>,
) {
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut frame_accumulator: Vec<f32> = Vec::new();
    let mut recording = false;
    let mut first_callback_logged = false;

    loop {
        // Wait for audio samples with timeout — allows Shutdown to be processed
        // even if the device stops producing audio (e.g. Bluetooth disconnect).
        let raw = match sample_rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(s) => Some(s),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // stream closed
        };

        // Process audio only if recording and we have data
        if let Some(ref samples) = raw {
            if recording {
                if !first_callback_logged {
                    crate::dev_log!(
                        "[AUDIO/WORKER] First audio chunk while recording: {} samples",
                        samples.len()
                    );
                    first_callback_logged = true;
                }
                process_audio(samples, &vad, &mut frame_accumulator, &mut audio_buffer);
            }
        }

        // Non-blocking check for commands (processed even on timeout — no audio needed)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::Start => {
                    audio_buffer.clear();
                    frame_accumulator.clear();
                    recording = true;
                    first_callback_logged = false;

                    // Reset VAD state for new session
                    if let Ok(mut guard) = vad.lock() {
                        if let Some(ref mut v) = *guard {
                            v.reset();
                        }
                    }

                    crate::dev_log!("[AUDIO/WORKER] Recording started (buffer cleared)");
                }
                Cmd::Stop(reply_tx) => {
                    recording = false;

                    // Drain any audio chunks captured but not yet consumed
                    while let Ok(remaining) = sample_rx.try_recv() {
                        process_audio(&remaining, &vad, &mut frame_accumulator, &mut audio_buffer);
                    }

                    // Flush VAD accumulator — process any remaining partial frame
                    flush_vad_accumulator(&vad, &mut frame_accumulator, &mut audio_buffer);

                    let samples = std::mem::take(&mut audio_buffer);
                    crate::dev_log!(
                        "[AUDIO/WORKER] Stop: returning {} samples ({:.2}s)",
                        samples.len(),
                        samples.len() as f32 / 16000.0
                    );
                    let _ = reply_tx.send(samples);
                }
                Cmd::Shutdown => {
                    crate::dev_log!("[AUDIO/WORKER] Shutdown command received");
                    return;
                }
            }
        }
    }
}

/// Process audio samples through VAD (if enabled) into the buffer.
fn process_audio(
    samples: &[f32],
    vad: &Arc<Mutex<Option<SmoothedVad>>>,
    frame_acc: &mut Vec<f32>,
    audio_buffer: &mut Vec<f32>,
) {
    let Ok(mut vad_guard) = vad.lock() else {
        // If VAD lock fails, pass through raw audio
        audio_buffer.extend_from_slice(samples);
        return;
    };

    if let Some(ref mut vad_engine) = *vad_guard {
        // VAD enabled: accumulate into frames of SILERO_FRAME_SAMPLES
        frame_acc.extend_from_slice(samples);

        while frame_acc.len() >= SILERO_FRAME_SAMPLES {
            let frame: Vec<f32> = frame_acc.drain(..SILERO_FRAME_SAMPLES).collect();
            match vad_engine.push_frame(&frame) {
                Ok(VadFrame::Speech(speech_data)) => {
                    audio_buffer.extend_from_slice(speech_data);
                }
                Ok(VadFrame::Noise) => {} // discard silence
                Err(e) => {
                    // On VAD error, pass through raw audio
                    crate::dev_log!("[AUDIO] VAD error: {}, falling back to passthrough", e);
                    audio_buffer.extend_from_slice(&frame);
                }
            }
        }
    } else {
        // No VAD: passthrough
        audio_buffer.extend_from_slice(samples);
    }
}

/// Flush remaining samples in the VAD accumulator as raw audio.
fn flush_vad_accumulator(
    vad: &Arc<Mutex<Option<SmoothedVad>>>,
    frame_acc: &mut Vec<f32>,
    audio_buffer: &mut Vec<f32>,
) {
    if frame_acc.is_empty() {
        return;
    }

    let Ok(mut vad_guard) = vad.lock() else {
        audio_buffer.append(frame_acc);
        return;
    };

    if let Some(ref mut vad_engine) = *vad_guard {
        // Pad remaining samples to a full frame and process
        if !frame_acc.is_empty() {
            let mut final_frame = std::mem::take(frame_acc);
            final_frame.resize(SILERO_FRAME_SAMPLES, 0.0);
            match vad_engine.push_frame(&final_frame) {
                Ok(VadFrame::Speech(speech_data)) => {
                    audio_buffer.extend_from_slice(speech_data);
                }
                Ok(VadFrame::Noise) => {}
                Err(_) => {
                    audio_buffer.extend_from_slice(&final_frame);
                }
            }
        }
    } else {
        audio_buffer.append(frame_acc);
    }
}

// ── Device resolution (extracted for worker thread) ──────────────────────────

fn resolve_device(device_name: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    match device_name {
        None | Some("pipewire") | Some("default") | Some("") => {
            crate::dev_log!("[AUDIO/WORKER] Using default input device (fast path)");
            host.default_input_device()
                .ok_or("No default input device available".to_string())
        }
        Some(name) if name.starts_with("plughw:") => {
            crate::dev_log!("[AUDIO/WORKER] Opening ALSA device: {}", name);
            let found = host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false));

            match found {
                Some(dev) => Ok(dev),
                None => Err(format!(
                    "ALSA device '{}' not found (not yet available?)",
                    name
                )),
            }
        }
        Some(name) => {
            crate::dev_log!(
                "[AUDIO/WORKER] PipeWire source: {}, setting as default...",
                name
            );

            if !is_valid_device_name(name) {
                return Err(format!("Invalid device name: '{}'", name));
            }

            // Bluetooth: switch card to headset profile (HFP) for mic access
            if name.starts_with("bluez_input.") {
                let mac = name
                    .strip_prefix("bluez_input.")
                    .and_then(|m| m.rsplitn(2, '.').last())
                    .unwrap_or("");
                let card_name = format!("bluez_card.{}", mac);
                crate::dev_log!(
                    "[AUDIO/BT] Switching {} to headset-head-unit for mic access",
                    card_name
                );
                match std::process::Command::new("pactl")
                    .args(["set-card-profile", &card_name, "headset-head-unit"])
                    .output()
                {
                    Ok(o) if !o.status.success() => crate::dev_log!(
                        "[AUDIO/BT] pactl set-card-profile failed: {}",
                        String::from_utf8_lossy(&o.stderr)
                    ),
                    Err(e) => crate::dev_log!("[AUDIO/BT] Failed to run pactl: {}", e),
                    _ => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            match std::process::Command::new("pactl")
                .args(["set-default-source", name])
                .output()
            {
                Ok(o) if !o.status.success() => crate::dev_log!(
                    "[AUDIO/WORKER] pactl set-default-source failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => crate::dev_log!("[AUDIO/WORKER] Failed to run pactl: {}", e),
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            crate::dev_log!("[AUDIO/WORKER] Opening CPAL default (routed to PipeWire source)");
            host.default_input_device()
                .ok_or("No default input device available".to_string())
        }
    }
}

// ── Config negotiation (extracted for worker thread) ─────────────────────────

fn negotiate_config(
    device: &cpal::Device,
    target_sample_rate: u32,
) -> Result<(cpal::StreamConfig, u32), String> {
    let desired = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(target_sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    if let Ok(supported_ranges) = device.supported_input_configs() {
        for range in supported_ranges {
            if range.channels() == 1
                && range.min_sample_rate().0 <= target_sample_rate
                && range.max_sample_rate().0 >= target_sample_rate
            {
                return Ok((desired, target_sample_rate));
            }
        }
    }

    let default_config = device
        .default_input_config()
        .map_err(|e| format!("No supported input config: {}", e))?;

    let native_rate = default_config.sample_rate().0;
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(native_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    crate::dev_log!(
        "[AUDIO/WORKER] Device doesn't support {}Hz, using {}Hz with resampling",
        target_sample_rate,
        native_rate
    );

    Ok((config, native_rate))
}

// ── Downsampling ─────────────────────────────────────────────────────────────

/// Simple downsampling (nearest-neighbor) from source rate to target rate
fn downsample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (samples.len() as f64 / ratio) as usize;
    (0..output_len)
        .map(|i| {
            let src_idx = (i as f64 * ratio) as usize;
            samples[src_idx.min(samples.len() - 1)]
        })
        .collect()
}
