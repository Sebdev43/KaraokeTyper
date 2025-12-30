use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Get D-Bus environment variables needed for PulseAudio/PipeWire commands
/// when launched from .desktop file (where these might not be inherited)
fn get_dbus_env() -> (String, String) {
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    let dbus_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={}/bus", xdg_runtime));
    (xdg_runtime, dbus_addr)
}

/// Audio capture using CLI tools (parec/pw-record) - fallback for when cpal doesn't work.
/// This is the approach used by nerd-dictation and other reliable Linux voice tools.
pub struct AudioCaptureCli {
    pub is_recording: Arc<Mutex<bool>>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    recording_process: Arc<Mutex<Option<Child>>>,
    recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl AudioCaptureCli {
    pub fn new() -> Self {
        Self {
            is_recording: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            recording_process: Arc::new(Mutex::new(None)),
            recording_start_time: Arc::new(Mutex::new(None)),
        }
    }

    /// Start recording using parec (PulseAudio) or pw-record (PipeWire)
    /// device_name: Optional PipeWire/PulseAudio source name (e.g., "bluez_input.XX_XX_XX")
    pub fn start_recording(&self, device_name: Option<&str>) -> Result<(), String> {
        let is_recording = Arc::clone(&self.is_recording);
        let audio_buffer = Arc::clone(&self.audio_buffer);
        let recording_process = Arc::clone(&self.recording_process);
        let recording_start_time = Arc::clone(&self.recording_start_time);

        // Clear buffer and set recording state
        *is_recording.lock().unwrap() = true;
        audio_buffer.lock().unwrap().clear();
        *recording_start_time.lock().unwrap() = Some(std::time::Instant::now());

        // Build parec command with optional device
        // Format: signed 16-bit little-endian, mono, 16kHz
        // Low latency settings for responsive push-to-talk
        let mut args = vec![
            "--format=s16le".to_string(),
            "--rate=16000".to_string(),
            "--channels=1".to_string(),
            "--latency-msec=10".to_string(),
            "--raw".to_string(),
        ];

        // Add device argument if specified
        if let Some(dev) = device_name {
            if dev != "pipewire" && dev != "default" {
                args.push(format!("--device={}", dev));
                crate::dev_log!("[AUDIO_CLI] 🎤 Recording from device: {}", dev);
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

        crate::dev_log!("[AUDIO_CLI] ✅ Started parec recording (16kHz mono S16LE)");

        let stdout = child.stdout.take().ok_or("Failed to get parec stdout")?;
        *recording_process.lock().unwrap() = Some(child);

        // Spawn thread to read audio data
        // Use larger buffer for more reliable capture
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::with_capacity(4096, stdout);
            let mut buf = [0u8; 4096]; // 2048 samples at 16-bit = 128ms chunks

            // Read until EOF (when parec is killed, pipe closes)
            // Don't check is_recording here - read ALL remaining data from pipe
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF - pipe closed (parec killed)
                        break;
                    }
                    Ok(n) => {
                        // Convert S16LE bytes to f32 samples
                        let mut buffer = audio_buffer.lock().unwrap();
                        for chunk in buf[..n].chunks_exact(2) {
                            let sample_i16 = i16::from_le_bytes([chunk[0], chunk[1]]);
                            let sample_f32 = sample_i16 as f32 / 32768.0;
                            buffer.push(sample_f32);
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            crate::dev_log!("[AUDIO_CLI] ❌ Read error: {}", e);
                            break;
                        }
                    }
                }
            }

            crate::dev_log!("[AUDIO_CLI] 📍 Reader thread finished");
        });

        Ok(())
    }

    /// Stop recording and return the audio buffer
    pub fn stop_recording(&self) -> Result<Vec<f32>, String> {
        *self.is_recording.lock().unwrap() = false;

        // Kill the parec process
        if let Some(mut child) = self.recording_process.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Give the reader thread time to drain remaining data from pipe
        std::thread::sleep(std::time::Duration::from_millis(150));

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
    #[allow(dead_code)] // Kept for API compatibility
    pub fn get_buffer(&self) -> Vec<f32> {
        self.audio_buffer.lock().unwrap().clone()
    }
}

/// Check if CLI audio tools are available
#[allow(dead_code)] // Kept for runtime tool detection
pub fn check_cli_tools() -> Option<&'static str> {
    // Try parec first (most common)
    if Command::new("parec").arg("--version").output().is_ok() {
        return Some("parec");
    }
    // Try pw-record (PipeWire native)
    if Command::new("pw-record").arg("--version").output().is_ok() {
        return Some("pw-record");
    }
    None
}
