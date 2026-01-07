//! Audio capture module - GStreamer on Linux (like Handy), CPAL fallback for other platforms
//!
//! Key design: always_on_microphone = false
//! - Pipeline is created ONLY when PTT is pressed
//! - Pipeline is destroyed when PTT is released
//! - Mic is NOT constantly active (no system tray indicator)

use std::sync::{Arc, Mutex};

/// Audio capture state and buffer
pub struct AudioCapture {
    pub is_recording: Arc<Mutex<bool>>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    target_sample_rate: u32,
    actual_sample_rate: Arc<Mutex<u32>>,
    recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
    #[cfg(target_os = "linux")]
    pipeline: Arc<Mutex<Option<gstreamer::Element>>>,
}

impl AudioCapture {
    pub fn new(target_sample_rate: u32) -> Self {
        Self {
            is_recording: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            target_sample_rate,
            actual_sample_rate: Arc::new(Mutex::new(target_sample_rate)),
            recording_start_time: Arc::new(Mutex::new(None)),
            #[cfg(target_os = "linux")]
            pipeline: Arc::new(Mutex::new(None)),
        }
    }

    /// Get target sample rate (for Whisper compatibility)
    pub fn target_sample_rate(&self) -> u32 {
        self.target_sample_rate
    }
}

// ============================================================================
// LINUX: GStreamer implementation (like Handy app)
// ============================================================================
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::*;
    use gstreamer::prelude::*;
    use gstreamer_app::AppSink;
    use std::sync::atomic::{AtomicBool, Ordering};

    impl AudioCapture {
        /// Start recording - creates GStreamer pipeline ON DEMAND
        /// This is the key difference: mic is NOT constantly active
        pub fn start_recording_gst(&self, device_name: Option<&str>) -> Result<(), String> {
            // Initialize GStreamer if needed
            gstreamer::init().map_err(|e| format!("GStreamer init failed: {}", e))?;

            // Clear buffer and set recording state
            self.audio_buffer.lock().unwrap().clear();
            *self.recording_start_time.lock().unwrap() = Some(std::time::Instant::now());
            *self.is_recording.lock().unwrap() = true;

            // Build GStreamer pipeline for audio capture
            // Pipeline: pulsesrc -> audioconvert -> audioresample -> appsink
            let pipeline_str = if let Some(name) = device_name {
                format!(
                    "pulsesrc device=\"{}\" ! audioconvert ! audioresample ! \
                     audio/x-raw,format=F32LE,channels=1,rate={} ! appsink name=sink",
                    name, self.target_sample_rate
                )
            } else {
                format!(
                    "pulsesrc ! audioconvert ! audioresample ! \
                     audio/x-raw,format=F32LE,channels=1,rate={} ! appsink name=sink",
                    self.target_sample_rate
                )
            };

            crate::dev_log!("[AUDIO/GST] 🔧 Creating pipeline: {}", pipeline_str);

            let pipeline = gstreamer::parse::launch(&pipeline_str)
                .map_err(|e| format!("Pipeline creation failed: {}", e))?;

            // Get appsink and configure it
            let appsink = pipeline
                .clone()
                .dynamic_cast::<gstreamer::Bin>()
                .map_err(|_| "Pipeline is not a Bin")?
                .by_name("sink")
                .ok_or("appsink not found")?
                .dynamic_cast::<AppSink>()
                .map_err(|_| "sink is not an AppSink")?;

            // Configure appsink for low latency
            appsink.set_max_buffers(10);
            appsink.set_drop(false);
            appsink.set_sync(false);

            // Setup callback to collect audio samples
            let audio_buffer = Arc::clone(&self.audio_buffer);
            let is_recording = Arc::clone(&self.is_recording);
            let sample_count = Arc::new(AtomicBool::new(false));
            let sample_count_clone = Arc::clone(&sample_count);
            let target_rate = self.target_sample_rate;

            appsink.set_callbacks(
                gstreamer_app::AppSinkCallbacks::builder()
                    .new_sample(move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gstreamer::FlowError::Error)?;
                        let buffer = sample.buffer().ok_or(gstreamer::FlowError::Error)?;
                        let map = buffer.map_readable().map_err(|_| gstreamer::FlowError::Error)?;

                        // Convert bytes to f32 samples (F32LE format)
                        let samples: &[f32] = bytemuck_cast_slice(&map);

                        if *is_recording.lock().unwrap() {
                            let mut buf = audio_buffer.lock().unwrap();
                            buf.extend_from_slice(samples);

                            // Log first callback
                            if !sample_count_clone.swap(true, Ordering::Relaxed) {
                                crate::dev_log!(
                                    "[AUDIO/GST] 🔔 First audio callback: {} samples",
                                    samples.len()
                                );
                            }

                            // Log every ~0.5s
                            let samples_per_half_sec = target_rate as usize / 2;
                            if buf.len() % samples_per_half_sec < samples.len() {
                                crate::dev_log!(
                                    "[AUDIO/GST] 📊 Captured {} samples ({:.2}s)",
                                    buf.len(),
                                    buf.len() as f32 / target_rate as f32
                                );
                            }
                        }

                        Ok(gstreamer::FlowSuccess::Ok)
                    })
                    .build(),
            );

            // Start pipeline
            pipeline
                .set_state(gstreamer::State::Playing)
                .map_err(|e| format!("Failed to start pipeline: {:?}", e))?;

            // Store pipeline reference
            *self.pipeline.lock().unwrap() = Some(pipeline);
            *self.actual_sample_rate.lock().unwrap() = self.target_sample_rate;

            crate::dev_log!("[AUDIO/GST] ✅ Pipeline started - recording at {}Hz", self.target_sample_rate);
            Ok(())
        }

        /// Stop recording - DESTROYS the GStreamer pipeline
        /// Mic is released completely (always_on_microphone = false)
        pub fn stop_recording_gst(&self) {
            *self.is_recording.lock().unwrap() = false;

            // Stop and destroy pipeline
            if let Some(pipeline) = self.pipeline.lock().unwrap().take() {
                let _ = pipeline.set_state(gstreamer::State::Null);
                crate::dev_log!("[AUDIO/GST] 🛑 Pipeline stopped and destroyed");
            }

            let buffer_size = self.audio_buffer.lock().unwrap().len();
            let actual_rate = *self.actual_sample_rate.lock().unwrap();
            let start_time = self.recording_start_time.lock().unwrap().take();
            let elapsed_ms = start_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);

            crate::dev_log!(
                "[AUDIO/GST] 📊 Recording stopped: {}ms, {} samples ({:.2}s at {}Hz)",
                elapsed_ms,
                buffer_size,
                buffer_size as f32 / actual_rate as f32,
                actual_rate
            );
        }

        /// Get audio buffer (already at target sample rate from GStreamer)
        pub fn get_buffer(&self) -> Vec<f32> {
            self.audio_buffer.lock().unwrap().clone()
        }
    }

    /// Safe cast from bytes to f32 slice
    fn bytemuck_cast_slice(bytes: &[u8]) -> &[f32] {
        // F32LE = 4 bytes per sample, little endian
        let len = bytes.len() / 4;
        unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, len) }
    }
}

// ============================================================================
// Windows/macOS: CPAL fallback implementation
// ============================================================================
#[cfg(not(target_os = "linux"))]
mod other_impl {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::Sample;

    // Keep the existing CPAL implementation for Windows/macOS
    impl AudioCapture {
        pub fn start_recording_cpal(&self, device_name: Option<&str>) -> Result<cpal::Stream, String> {
            let host = cpal::default_host();
            let devices: Vec<_> = host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                .collect();

            if devices.is_empty() {
                return Err("No input devices available".to_string());
            }

            let device = if let Some(name) = device_name {
                devices
                    .iter()
                    .find(|d| d.name().map(|n| n.contains(name)).unwrap_or(false))
                    .cloned()
                    .unwrap_or_else(|| host.default_input_device().unwrap())
            } else {
                host.default_input_device()
                    .ok_or("No default input device")?
            };

            let config = cpal::StreamConfig {
                channels: 1,
                sample_rate: cpal::SampleRate(self.target_sample_rate),
                buffer_size: cpal::BufferSize::Default,
            };

            let is_recording = Arc::clone(&self.is_recording);
            let audio_buffer = Arc::clone(&self.audio_buffer);

            *is_recording.lock().unwrap() = true;
            audio_buffer.lock().unwrap().clear();
            *self.recording_start_time.lock().unwrap() = Some(std::time::Instant::now());

            let stream = device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        if *is_recording.lock().unwrap() {
                            audio_buffer.lock().unwrap().extend_from_slice(data);
                        }
                    },
                    |err| eprintln!("Stream error: {}", err),
                    None,
                )
                .map_err(|e| format!("Failed to build stream: {}", e))?;

            stream.play().map_err(|e| format!("Failed to play stream: {}", e))?;
            *self.actual_sample_rate.lock().unwrap() = self.target_sample_rate;

            Ok(stream)
        }

        pub fn stop_recording_cpal(&self) {
            *self.is_recording.lock().unwrap() = false;
        }

        pub fn get_buffer(&self) -> Vec<f32> {
            self.audio_buffer.lock().unwrap().clone()
        }
    }
}

// ============================================================================
// Cross-platform API
// ============================================================================

/// Result type for audio operations
pub type AudioResult<T> = Result<T, String>;

/// Platform-agnostic recording handle
pub enum RecordingHandle {
    #[cfg(target_os = "linux")]
    GStreamer, // Pipeline is stored in AudioCapture
    #[cfg(not(target_os = "linux"))]
    Cpal(cpal::Stream),
}

impl AudioCapture {
    /// Start recording (platform-specific implementation)
    pub fn start_recording(&self, device_name: Option<&str>) -> AudioResult<RecordingHandle> {
        #[cfg(target_os = "linux")]
        {
            self.start_recording_gst(device_name)?;
            Ok(RecordingHandle::GStreamer)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let stream = self.start_recording_cpal(device_name)?;
            Ok(RecordingHandle::Cpal(stream))
        }
    }

    /// Stop recording (platform-specific implementation)
    pub fn stop_recording(&self) {
        #[cfg(target_os = "linux")]
        self.stop_recording_gst();
        #[cfg(not(target_os = "linux"))]
        self.stop_recording_cpal();
    }
}
