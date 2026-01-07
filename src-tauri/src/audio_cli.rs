use ringbuf::{traits::*, HeapRb};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Pre-buffer duration in milliseconds (captures audio before PTT is pressed)
const PRE_BUFFER_MS: usize = 500;
/// Pre-buffer size in samples (16kHz * PRE_BUFFER_MS / 1000)
const PRE_BUFFER_SAMPLES: usize = 16000 * PRE_BUFFER_MS / 1000; // 8000 samples = 500ms

/// Get D-Bus environment variables needed for PulseAudio/PipeWire commands
/// when launched from .desktop file (where these might not be inherited)
fn get_dbus_env() -> (String, String) {
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    let dbus_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={}/bus", xdg_runtime));
    (xdg_runtime, dbus_addr)
}

/// Audio capture using CLI tools (parec/pw-record) with pre-buffering.
///
/// Uses a ring buffer to continuously capture audio, ensuring the beginning
/// of speech is never lost when push-to-talk is pressed.
pub struct AudioCaptureCli {
    /// Whether we're actively recording (PTT pressed)
    pub is_recording: Arc<Mutex<bool>>,
    /// The main audio buffer that accumulates during recording
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    /// Ring buffer for pre-capture (last 500ms of audio)
    pre_buffer: Arc<Mutex<HeapRb<f32>>>,
    /// The continuous capture process
    capture_process: Arc<Mutex<Option<Child>>>,
    /// Thread handle for the capture reader
    capture_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Whether continuous capture is running
    is_capturing: Arc<Mutex<bool>>,
    /// Recording start time for logging
    recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
    /// Device name for capture
    device_name: Arc<Mutex<Option<String>>>,
}

impl AudioCaptureCli {
    pub fn new() -> Self {
        Self {
            is_recording: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            pre_buffer: Arc::new(Mutex::new(HeapRb::new(PRE_BUFFER_SAMPLES))),
            capture_process: Arc::new(Mutex::new(None)),
            capture_thread: Arc::new(Mutex::new(None)),
            is_capturing: Arc::new(Mutex::new(false)),
            recording_start_time: Arc::new(Mutex::new(None)),
            device_name: Arc::new(Mutex::new(None)),
        }
    }

    /// Start continuous audio capture (runs in background, fills pre-buffer)
    /// This should be called once at app startup or when device changes
    pub fn start_continuous_capture(&self, device_name: Option<&str>) -> Result<(), String> {
        // Check if already capturing
        if *self.is_capturing.lock().unwrap() {
            crate::dev_log!("[AUDIO_CLI] ⚠️ Continuous capture already running");
            return Ok(());
        }

        // Store device name for potential restarts
        *self.device_name.lock().unwrap() = device_name.map(|s| s.to_string());

        let is_recording = Arc::clone(&self.is_recording);
        let audio_buffer = Arc::clone(&self.audio_buffer);
        let pre_buffer = Arc::clone(&self.pre_buffer);
        let capture_process = Arc::clone(&self.capture_process);
        let is_capturing = Arc::clone(&self.is_capturing);

        // Build parec command
        let mut args = vec![
            "--format=s16le".to_string(),
            "--rate=16000".to_string(),
            "--channels=1".to_string(),
            "--latency-msec=10".to_string(),
            "--raw".to_string(),
        ];

        if let Some(dev) = device_name {
            if dev != "pipewire" && dev != "default" {
                args.push(format!("--device={}", dev));
                crate::dev_log!("[AUDIO_CLI] 🎤 Continuous capture from device: {}", dev);
            }
        }

        let (xdg_runtime, dbus_addr) = get_dbus_env();
        let mut child = Command::new("parec")
            .args(&args)
            .env("XDG_RUNTIME_DIR", &xdg_runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                format!(
                    "Failed to start parec: {}. Install with: sudo apt install pulseaudio-utils",
                    e
                )
            })?;

        let stdout = child.stdout.take().ok_or("Failed to get parec stdout")?;
        *capture_process.lock().unwrap() = Some(child);
        *is_capturing.lock().unwrap() = true;

        crate::dev_log!(
            "[AUDIO_CLI] ✅ Started continuous capture (pre-buffer: {}ms = {} samples)",
            PRE_BUFFER_MS,
            PRE_BUFFER_SAMPLES
        );

        // Spawn thread to continuously read audio data
        let handle = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::with_capacity(4096, stdout);
            let mut buf = [0u8; 4096];

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF - pipe closed
                        break;
                    }
                    Ok(n) => {
                        let recording = *is_recording.lock().unwrap();

                        // Convert S16LE bytes to f32 samples
                        for chunk in buf[..n].chunks_exact(2) {
                            let sample_i16 = i16::from_le_bytes([chunk[0], chunk[1]]);
                            let sample_f32 = sample_i16 as f32 / 32768.0;

                            if recording {
                                // If recording, add directly to audio buffer
                                audio_buffer.lock().unwrap().push(sample_f32);
                            } else {
                                // If not recording, add to ring pre-buffer (overwrites oldest)
                                let mut pb = pre_buffer.lock().unwrap();
                                // Push to ring buffer - if full, oldest is automatically dropped
                                let _ = pb.try_push(sample_f32);
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            crate::dev_log!("[AUDIO_CLI] ❌ Capture read error: {}", e);
                            break;
                        }
                    }
                }
            }

            *is_capturing.lock().unwrap() = false;
            crate::dev_log!("[AUDIO_CLI] 📍 Continuous capture thread finished");
        });

        *self.capture_thread.lock().unwrap() = Some(handle);

        Ok(())
    }

    /// Stop continuous capture
    pub fn stop_continuous_capture(&self) {
        if let Some(mut child) = self.capture_process.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *self.is_capturing.lock().unwrap() = false;
        crate::dev_log!("[AUDIO_CLI] 🛑 Continuous capture stopped");
    }

    /// Start recording (PTT pressed)
    /// Copies pre-buffer content to audio buffer and starts accumulating
    pub fn start_recording(&self, device_name: Option<&str>) -> Result<(), String> {
        // Ensure continuous capture is running
        if !*self.is_capturing.lock().unwrap() {
            self.start_continuous_capture(device_name)?;
            // Give parec a moment to start
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Clear audio buffer and copy pre-buffer content
        {
            let mut audio_buf = self.audio_buffer.lock().unwrap();
            let mut pre_buf = self.pre_buffer.lock().unwrap();

            audio_buf.clear();

            // Copy all samples from pre-buffer to audio buffer
            let pre_samples: Vec<f32> = pre_buf.iter().copied().collect();
            let pre_count = pre_samples.len();
            audio_buf.extend(pre_samples);

            // Clear pre-buffer
            pre_buf.clear();

            crate::dev_log!(
                "[AUDIO_CLI] 🎙️ Recording started - copied {} pre-buffer samples ({:.0}ms)",
                pre_count,
                pre_count as f32 / 16.0
            );
        }

        // Set recording state
        *self.is_recording.lock().unwrap() = true;
        *self.recording_start_time.lock().unwrap() = Some(std::time::Instant::now());

        Ok(())
    }

    /// Stop recording and return the audio buffer
    pub fn stop_recording(&self) -> Result<Vec<f32>, String> {
        *self.is_recording.lock().unwrap() = false;

        // Give a moment for any in-flight samples to be processed
        std::thread::sleep(std::time::Duration::from_millis(50));

        let buffer = self.audio_buffer.lock().unwrap().clone();
        let elapsed = self
            .recording_start_time
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_millis())
            .unwrap_or(0);

        crate::dev_log!("[AUDIO_CLI] 🛑 Recording stopped after {}ms", elapsed);
        crate::dev_log!(
            "[AUDIO_CLI] 🛑 Buffer: {} samples = {:.2}s of audio",
            buffer.len(),
            buffer.len() as f32 / 16000.0
        );

        if buffer.is_empty() {
            return Err("No audio data captured".to_string());
        }

        // Check for non-silent samples
        let non_silent = buffer.iter().filter(|&&s| s.abs() > 0.01).count();
        crate::dev_log!(
            "[AUDIO_CLI] 📊 Non-silent samples: {}/{} ({:.1}%)",
            non_silent,
            buffer.len(),
            100.0 * non_silent as f32 / buffer.len() as f32
        );

        Ok(buffer)
    }

    /// Get the audio buffer (for compatibility with existing code)
    #[allow(dead_code)]
    pub fn get_buffer(&self) -> Vec<f32> {
        self.audio_buffer.lock().unwrap().clone()
    }

    /// Check if continuous capture is running
    #[allow(dead_code)]
    pub fn is_capturing(&self) -> bool {
        *self.is_capturing.lock().unwrap()
    }
}

impl Drop for AudioCaptureCli {
    fn drop(&mut self) {
        self.stop_continuous_capture();
    }
}

/// Check if CLI audio tools are available
#[allow(dead_code)]
pub fn check_cli_tools() -> Option<&'static str> {
    if Command::new("parec").arg("--version").output().is_ok() {
        return Some("parec");
    }
    if Command::new("pw-record").arg("--version").output().is_ok() {
        return Some("pw-record");
    }
    None
}
