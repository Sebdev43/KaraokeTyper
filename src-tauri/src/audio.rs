//! Audio capture module - CPAL (cross-platform)
//!
//! Uses CPAL for audio capture on all platforms:
//! - Linux: ALSA backend (transparent PipeWire support via pipewire-alsa)
//! - Windows: WASAPI backend
//! - macOS: CoreAudio backend
//!
//! Architecture (persistent stream for instant PTT):
//! - `open_stream()` creates the stream (slow ~1-2s) — done once on first PTT
//! - `start_recording()` clears buffer + plays → mic is ON (instant)
//! - `stop_recording()` pauses the stream → mic callbacks stop (instant)
//! - Stream stays open (paused) between recordings for instant next PTT
//! - `close_stream()` fully releases the device (on device change or app exit)

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Validate device names to prevent command injection via pactl
fn is_valid_device_name(name: &str) -> bool {
    name.chars().all(|c| c.is_alphanumeric() || "_.:-,=".contains(c))
}

/// Wrapper to make cpal::Stream usable across threads.
/// cpal::Stream is !Send+!Sync as a conservative choice, but the stream handle
/// is safe to move between threads — we only store it and call play/pause/drop.
struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

impl SendStream {
    fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.0.play()
    }
    fn pause(&self) -> Result<(), cpal::PauseStreamError> {
        self.0.pause()
    }
}

/// Delay before auto-closing the stream after recording stops (releases mic device)
const STREAM_AUTO_CLOSE_SECS: u64 = 10;

/// Audio capture state and buffer
pub struct AudioCapture {
    audio_buffer: Arc<Mutex<Vec<f32>>>,
    target_sample_rate: u32,
    actual_sample_rate: Arc<Mutex<u32>>,
    recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
    /// CPAL stream — created once, controlled via play()/pause()
    stream: Arc<Mutex<Option<SendStream>>>,
    /// Tracks first callback per recording session (for debug logging)
    first_callback_fired: Arc<AtomicBool>,
    /// Counter incremented each stop_recording; auto-close thread checks this
    stop_generation: Arc<AtomicU64>,
    /// Whether current stream is a Bluetooth device (auto-close only for BT)
    is_bluetooth: Arc<AtomicBool>,
}

impl AudioCapture {
    pub fn new(target_sample_rate: u32) -> Self {
        Self {
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            target_sample_rate,
            actual_sample_rate: Arc::new(Mutex::new(target_sample_rate)),
            recording_start_time: Arc::new(Mutex::new(None)),
            stream: Arc::new(Mutex::new(None)),
            first_callback_fired: Arc::new(AtomicBool::new(false)),
            stop_generation: Arc::new(AtomicU64::new(0)),
            is_bluetooth: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open the audio stream for a device. The stream starts PAUSED (mic OFF).
    /// Call this once at init or when the user changes device in Settings.
    ///
    /// Device matching:
    /// - `None`, `"pipewire"`, `"default"`, `""` → system default (fast, no enumeration)
    /// - `"plughw:CARD=..."` → ALSA hardware device (direct CPAL open)
    /// - PipeWire source name (bluez, alsa_input, etc.) → route via pactl + CPAL default
    pub fn open_stream(&self, device_name: Option<&str>) -> Result<(), String> {
        // Close any existing stream first
        self.close_stream();

        // Track if this is a Bluetooth device (for auto-close logic)
        let is_bt = device_name.map(|n| n.starts_with("bluez_input.")).unwrap_or(false);
        self.is_bluetooth.store(is_bt, Ordering::Relaxed);

        let host = cpal::default_host();

        let device = match device_name {
            None | Some("pipewire") | Some("default") | Some("") => {
                crate::dev_log!("[AUDIO/CPAL] Using default input device (fast path)");
                host.default_input_device()
                    .ok_or("No default input device available")?
            }
            Some(name) if name.starts_with("plughw:") => {
                // ALSA hardware device — find directly in CPAL
                crate::dev_log!("[AUDIO/CPAL] Opening ALSA device: {}", name);
                let found = host
                    .input_devices()
                    .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                    .find(|d| d.name().map(|n| n == name).unwrap_or(false));

                match found {
                    Some(dev) => dev,
                    None => {
                        // Don't fallback to default — return error so pre-open fails cleanly
                        // and first PTT retries (USB device may not be ready at startup)
                        return Err(format!("ALSA device '{}' not found (not yet available?)", name));
                    }
                }
            }
            Some(name) => {
                // PipeWire source name (Bluetooth, virtual, etc.)
                // Route it as PipeWire default, then open CPAL default device
                crate::dev_log!("[AUDIO/CPAL] PipeWire source: {}, setting as default...", name);

                if !is_valid_device_name(name) {
                    return Err(format!("Invalid device name: '{}'", name));
                }

                // Bluetooth: switch card to headset profile (HFP) so mic source exists
                // In A2DP mode there's no input source — we must switch to HFP first
                if name.starts_with("bluez_input.") {
                    let mac = name.strip_prefix("bluez_input.")
                        .and_then(|m| m.rsplitn(2, '.').last())
                        .unwrap_or("");
                    let card_name = format!("bluez_card.{}", mac);
                    crate::dev_log!("[AUDIO/BT] Switching {} to headset-head-unit for mic access", card_name);
                    match std::process::Command::new("pactl")
                        .args(["set-card-profile", &card_name, "headset-head-unit"])
                        .output()
                    {
                        Ok(o) if !o.status.success() => crate::dev_log!("[AUDIO/BT] pactl set-card-profile failed: {}", String::from_utf8_lossy(&o.stderr)),
                        Err(e) => crate::dev_log!("[AUDIO/BT] Failed to run pactl: {}", e),
                        _ => {}
                    }
                    // Wait for PipeWire to create the input source
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }

                match std::process::Command::new("pactl")
                    .args(["set-default-source", name])
                    .output()
                {
                    Ok(o) if !o.status.success() => crate::dev_log!("[AUDIO/CPAL] pactl set-default-source failed: {}", String::from_utf8_lossy(&o.stderr)),
                    Err(e) => crate::dev_log!("[AUDIO/CPAL] Failed to run pactl: {}", e),
                    _ => {}
                }
                // Small delay for PipeWire to switch
                std::thread::sleep(std::time::Duration::from_millis(100));
                crate::dev_log!("[AUDIO/CPAL] Opening CPAL default (routed to PipeWire source)");
                host.default_input_device()
                    .ok_or("No default input device available")?
            }
        };

        let device_display = device.name().unwrap_or_else(|_| "unknown".to_string());
        crate::dev_log!("[AUDIO/CPAL] Selected device: {}", device_display);

        // Negotiate sample rate
        let (config, actual_rate) = self.negotiate_config(&device)?;

        crate::dev_log!(
            "[AUDIO/CPAL] Stream config: {}ch, {}Hz (target: {}Hz)",
            config.channels,
            actual_rate,
            self.target_sample_rate
        );

        let audio_buffer = Arc::clone(&self.audio_buffer);
        let first_callback_fired = Arc::clone(&self.first_callback_fired);
        let target_rate = self.target_sample_rate;

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Resample if needed
                    let samples = if actual_rate != target_rate {
                        downsample(data, actual_rate, target_rate)
                    } else {
                        data.to_vec()
                    };

                    let Ok(mut buf) = audio_buffer.lock() else { return; };
                    buf.extend_from_slice(&samples);

                    if !first_callback_fired.swap(true, Ordering::Relaxed) {
                        crate::dev_log!(
                            "[AUDIO/CPAL] First audio callback: {} samples (resampled: {})",
                            data.len(),
                            actual_rate != target_rate
                        );
                    }
                },
                |err| {
                    crate::dev_log!("[AUDIO/CPAL] Stream error: {}", err);
                },
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        // Stream starts PAUSED — mic is OFF until start_recording() calls play()
        // Note: CPAL streams start in playing state, so we pause immediately
        stream
            .pause()
            .map_err(|e| format!("Failed to pause stream: {}", e))?;

        *self.actual_sample_rate.lock().unwrap() = actual_rate;
        *self.stream.lock().unwrap() = Some(SendStream(stream));

        crate::dev_log!(
            "[AUDIO/CPAL] Stream opened (PAUSED) at {}Hz from '{}'",
            actual_rate,
            device_display
        );
        Ok(())
    }

    /// Close the audio stream and release the device.
    pub fn close_stream(&self) {
        *self.stream.lock().unwrap() = None;
    }

    /// Check if a stream is currently open.
    pub fn is_stream_open(&self) -> bool {
        self.stream.lock().unwrap().is_some()
    }

    /// Start recording — calls stream.play() (instant, ~0ms).
    pub fn start_recording(&self) -> Result<(), String> {
        // Cancel any pending auto-close timer
        self.stop_generation.fetch_add(1, Ordering::SeqCst);

        let stream_guard = self.stream.lock().unwrap();
        let stream = stream_guard.as_ref().ok_or("Audio stream not open")?;

        // Clear buffer and reset state for this recording session
        self.audio_buffer.lock().unwrap().clear();
        self.first_callback_fired.store(false, Ordering::Relaxed);
        *self.recording_start_time.lock().unwrap() = Some(std::time::Instant::now());

        // Resume the stream — mic is now ON, callbacks start firing
        stream.play().map_err(|e| format!("Failed to play stream: {}", e))?;

        crate::dev_log!("[AUDIO/CPAL] Recording started (stream.play)");
        Ok(())
    }

    /// Stop recording — pauses the stream, then auto-closes it after a timeout.
    /// If PTT is pressed again before the timeout, the stream is still open (instant).
    /// After the timeout, the stream closes and releases the mic device (important for Bluetooth).
    pub fn stop_recording(&self) {
        if let Some(stream) = self.stream.lock().unwrap().as_ref() {
            if let Err(e) = stream.pause() {
                crate::dev_log!("[AUDIO/CPAL] Failed to pause stream: {}", e);
            }
        }

        let buffer_size = self.audio_buffer.lock().unwrap().len();
        let actual_rate = *self.actual_sample_rate.lock().unwrap();
        let start_time = self.recording_start_time.lock().unwrap().take();
        let elapsed_ms = start_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);

        crate::dev_log!(
            "[AUDIO/CPAL] Recording stopped: {}ms, {} samples ({:.2}s at {}Hz)",
            elapsed_ms,
            buffer_size,
            buffer_size as f32 / actual_rate as f32,
            actual_rate
        );

        // Auto-close only for Bluetooth devices (to release HFP profile → restore A2DP)
        // Non-BT devices keep the stream open (paused) for instant next PTT
        if self.is_bluetooth.load(Ordering::Relaxed) {
            let gen = self.stop_generation.fetch_add(1, Ordering::SeqCst) + 1;
            let gen_ref = Arc::clone(&self.stop_generation);
            let stream_ref = Arc::clone(&self.stream);

            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(STREAM_AUTO_CLOSE_SECS));
                // Only close if no new recording happened since we scheduled this
                if gen_ref.load(Ordering::SeqCst) == gen {
                    let Ok(mut guard) = stream_ref.lock() else { return; };
                    if guard.is_some() {
                        *guard = None;
                        crate::dev_log!(
                            "[AUDIO/CPAL] Stream auto-closed after {}s idle (BT mic released)",
                            STREAM_AUTO_CLOSE_SECS
                        );
                    }
                }
            });

            crate::dev_log!(
                "[AUDIO/CPAL] Stream paused — will auto-close in {}s if idle (BT device)",
                STREAM_AUTO_CLOSE_SECS
            );
        } else {
            crate::dev_log!("[AUDIO/CPAL] Stream paused — ready for next PTT (instant)");
        }
    }

    /// Get recorded audio buffer.
    pub fn get_buffer(&self) -> Vec<f32> {
        self.audio_buffer.lock().unwrap().clone()
    }

    /// Negotiate stream config: try 16kHz mono, fallback to device native rate.
    fn negotiate_config(
        &self,
        device: &cpal::Device,
    ) -> Result<(cpal::StreamConfig, u32), String> {
        let desired = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(self.target_sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        // Check if device supports our desired config
        if let Ok(supported_ranges) = device.supported_input_configs() {
            for range in supported_ranges {
                if range.channels() == 1
                    && range.min_sample_rate().0 <= self.target_sample_rate
                    && range.max_sample_rate().0 >= self.target_sample_rate
                {
                    return Ok((desired, self.target_sample_rate));
                }
            }
        }

        // Fallback: use device default config, mono channel
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
            "[AUDIO/CPAL] Device doesn't support {}Hz, using {}Hz with resampling",
            self.target_sample_rate,
            native_rate
        );

        Ok((config, native_rate))
    }
}

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
