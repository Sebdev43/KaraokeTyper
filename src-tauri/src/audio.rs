use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, Stream};
use hound::{WavSpec, WavWriter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// AudioCapture uses cpal - used on Windows/macOS, Linux uses audio_cli
#[allow(dead_code)]
pub struct AudioCapture {
    pub is_recording: Arc<Mutex<bool>>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    target_sample_rate: u32,
    actual_sample_rate: Arc<Mutex<u32>>,
    recording_start_time: Arc<Mutex<Option<std::time::Instant>>>,
}

#[allow(dead_code)] // Used on Windows/macOS, Linux uses audio_cli
impl AudioCapture {
    pub fn new(target_sample_rate: u32) -> Self {
        Self {
            is_recording: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            target_sample_rate,
            actual_sample_rate: Arc::new(Mutex::new(target_sample_rate)),
            recording_start_time: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start_recording(&self) -> Result<Stream, String> {
        let host = cpal::default_host();

        // List all input devices for debugging
        let devices: Vec<_> = host
            .input_devices()
            .map_err(|e| format!("Failed to enumerate devices: {}", e))?
            .collect();

        println!("[AUDIO] 📋 Available input devices:");
        for (i, d) in devices.iter().enumerate() {
            let n = d.name().unwrap_or_else(|_| "?".to_string());
            println!("[AUDIO]   [{}] {}", i, n);
        }

        // CRITICAL: The "pipewire" device in cpal DOES NOT produce callbacks!
        // Use "default" which routes through ALSA→PipeWire bridge and works.
        let device = devices
            .iter()
            // First: "default" device - works via ALSA→PipeWire bridge
            .find(|d| d.name().map(|n| n == "default").unwrap_or(false))
            // Second: USB devices (webcams like Logitech C270)
            .or_else(|| {
                devices.iter().find(|d| {
                    d.name()
                        .map(|n| n.starts_with("hw:CARD=U"))
                        .unwrap_or(false)
                })
            })
            // Third: any hw:CARD with input capability
            .or_else(|| {
                devices
                    .iter()
                    .find(|d| d.name().map(|n| n.starts_with("hw:CARD=")).unwrap_or(false))
            })
            // Last resort: first device that's NOT "pipewire"
            .or_else(|| {
                devices
                    .iter()
                    .find(|d| d.name().map(|n| n != "pipewire").unwrap_or(true))
            })
            .cloned()
            .ok_or("No input device available")?;

        let actual_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        println!("[AUDIO] 🎤 Selected device: {}", actual_name);

        self.start_recording_from_device(device)
    }

    /// Start recording from a specific cpal device
    fn start_recording_from_device(&self, device: cpal::Device) -> Result<Stream, String> {
        let _actual_name = device.name().unwrap_or_else(|_| "Unknown".to_string());

        // Get default config to know the sample format and supported rates
        let default_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;

        let device_sample_rate = default_config.sample_rate().0;

        // Try target sample rate first, fallback to device native rate
        let (config, actual_rate) = self
            .try_create_config(&device, self.target_sample_rate)
            .or_else(|_| {
                println!(
                    "[AUDIO] ⚠️  Device doesn't support {}Hz, using native {}Hz (will resample)",
                    self.target_sample_rate, device_sample_rate
                );
                self.try_create_config(&device, device_sample_rate)
            })?;

        println!(
            "[AUDIO] 📊 Capturing at {}Hz (target: {}Hz)",
            actual_rate, self.target_sample_rate
        );

        // Store actual sample rate for resampling
        *self.actual_sample_rate.lock().unwrap() = actual_rate;

        let is_recording = Arc::clone(&self.is_recording);
        let audio_buffer = Arc::clone(&self.audio_buffer);

        *is_recording.lock().unwrap() = true;
        audio_buffer.lock().unwrap().clear();
        *self.recording_start_time.lock().unwrap() = Some(std::time::Instant::now());

        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => {
                self.build_stream::<f32>(&device, &config, is_recording, audio_buffer)
            }
            cpal::SampleFormat::I16 => {
                self.build_stream::<i16>(&device, &config, is_recording, audio_buffer)
            }
            cpal::SampleFormat::U16 => {
                self.build_stream::<u16>(&device, &config, is_recording, audio_buffer)
            }
            _ => return Err("Unsupported sample format".to_string()),
        }?;

        stream
            .play()
            .map_err(|e| format!("Stream play error: {}", e))?;

        println!(
            "[AUDIO] ✅ Stream started at {:?}",
            std::time::Instant::now()
        );
        Ok(stream)
    }

    pub fn start_recording_with_device(&self, device_index: usize) -> Result<Stream, String> {
        self.start_recording_with_device_name(device_index, None)
    }

    /// Find device by name (robust, works across OS versions and cpal updates)
    pub fn start_recording_with_device_name(
        &self,
        _device_index: usize,
        device_name: Option<&str>,
    ) -> Result<Stream, String> {
        let host = cpal::default_host();

        // Get all input devices
        let devices: Vec<_> = host
            .input_devices()
            .map_err(|e| format!("Failed to enumerate devices: {}", e))?
            .collect();

        if devices.is_empty() {
            return Err("No input devices available".to_string());
        }

        // List all available devices for debugging
        println!("[AUDIO] 📋 Available input devices:");
        for (i, d) in devices.iter().enumerate() {
            let n = d.name().unwrap_or_else(|_| "?".to_string());
            println!("[AUDIO]   [{}] {}", i, n);
        }

        // IMPORTANT: On Linux with PipeWire/ALSA, the "default" and "pipewire" devices
        // often don't produce callbacks. USB devices (hw:CARD=U*) usually work.
        //
        // Strategy 1: Try to find by name (with partial matching)
        // Strategy 2: Find USB devices (most reliable on Linux)
        // Strategy 3: Use pipewire device
        // Strategy 4: Fall back to other devices
        let device = if let Some(name) = device_name {
            // Try partial name matching
            let found = devices.iter().find(|d| {
                if let Ok(n) = d.name() {
                    n.to_lowercase().contains(&name.to_lowercase())
                        || name.to_lowercase().contains(&n.to_lowercase())
                } else {
                    false
                }
            });

            if let Some(dev) = found {
                println!(
                    "[AUDIO] 🎤 Found device by name: {}",
                    dev.name().unwrap_or_default()
                );
                dev.clone()
            } else {
                // Name not found - find a working device
                println!("[AUDIO] ⚠️  Device '{}' not found by name", name);
                find_working_device(&devices, &host)?
            }
        } else {
            // No name provided - find a working device
            find_working_device(&devices, &host)?
        };

        fn find_working_device(
            devices: &[cpal::Device],
            host: &cpal::Host,
        ) -> Result<cpal::Device, String> {
            use cpal::traits::DeviceTrait;

            // CRITICAL: The "pipewire" device in cpal DOES NOT produce callbacks!
            // It's a known bug - the stream opens but no audio data arrives.
            //
            // Working solutions (from open-source projects research):
            // 1. "default" device - routes through ALSA→PipeWire bridge, callbacks work
            // 2. hw:CARD=* devices - direct hardware access, reliable
            // 3. Host default_input_device() - usually same as "default"
            //
            // NEVER use the device named "pipewire" directly!

            // Priority 1: "default" device - works via ALSA→PipeWire bridge
            if let Some(dev) = devices
                .iter()
                .find(|d| d.name().map(|n| n == "default").unwrap_or(false))
            {
                println!("[AUDIO] 🎤 Using 'default' device (ALSA→PipeWire bridge)");
                return Ok(dev.clone());
            }

            // Priority 2: Host default input device
            if let Some(dev) = host.default_input_device() {
                let name = dev.name().unwrap_or_default();
                // Skip if it's the broken "pipewire" device
                if name != "pipewire" {
                    println!("[AUDIO] 🎤 Using host default device: {}", name);
                    return Ok(dev);
                }
            }

            // Priority 3: USB devices (hw:CARD=U*) - webcams, USB mics
            if let Some(dev) = devices.iter().find(|d| {
                d.name()
                    .map(|n| n.starts_with("hw:CARD=U"))
                    .unwrap_or(false)
            }) {
                println!(
                    "[AUDIO] 🎤 Using USB device: {}",
                    dev.name().unwrap_or_default()
                );
                return Ok(dev.clone());
            }

            // Priority 4: Any hw:CARD device (direct hardware)
            if let Some(dev) = devices
                .iter()
                .find(|d| d.name().map(|n| n.starts_with("hw:CARD=")).unwrap_or(false))
            {
                println!(
                    "[AUDIO] 🎤 Using hardware device: {}",
                    dev.name().unwrap_or_default()
                );
                return Ok(dev.clone());
            }

            // Priority 5: First available that's NOT "pipewire"
            if let Some(dev) = devices
                .iter()
                .find(|d| d.name().map(|n| n != "pipewire").unwrap_or(true))
            {
                println!(
                    "[AUDIO] 🎤 Using first available device: {}",
                    dev.name().unwrap_or_default()
                );
                return Ok(dev.clone());
            }

            Err("No working input device found".to_string())
        }

        let actual_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        println!("[AUDIO] 🎤 Recording from: {}", actual_name);

        // Get default config to know the sample format and supported rates
        let default_config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;

        let device_sample_rate = default_config.sample_rate().0;

        // Try target sample rate first, fallback to device native rate
        let (config, actual_rate) = self
            .try_create_config(&device, self.target_sample_rate)
            .or_else(|_| {
                println!(
                    "[AUDIO] ⚠️  Device doesn't support {}Hz, using native {}Hz (will resample)",
                    self.target_sample_rate, device_sample_rate
                );
                self.try_create_config(&device, device_sample_rate)
            })?;

        println!(
            "[AUDIO] 📊 Capturing at {}Hz (target: {}Hz)",
            actual_rate, self.target_sample_rate
        );

        // Store actual sample rate for resampling
        *self.actual_sample_rate.lock().unwrap() = actual_rate;

        let is_recording = Arc::clone(&self.is_recording);
        let audio_buffer = Arc::clone(&self.audio_buffer);

        *is_recording.lock().unwrap() = true;
        audio_buffer.lock().unwrap().clear();
        *self.recording_start_time.lock().unwrap() = Some(std::time::Instant::now());

        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => {
                self.build_stream::<f32>(&device, &config, is_recording, audio_buffer)
            }
            cpal::SampleFormat::I16 => {
                self.build_stream::<i16>(&device, &config, is_recording, audio_buffer)
            }
            cpal::SampleFormat::U16 => {
                self.build_stream::<u16>(&device, &config, is_recording, audio_buffer)
            }
            _ => return Err("Unsupported sample format".to_string()),
        }?;

        stream
            .play()
            .map_err(|e| format!("Stream play error: {}", e))?;
        println!(
            "[AUDIO] ✅ Stream started at {:?}",
            std::time::Instant::now()
        );
        Ok(stream)
    }

    /// Try to create a stream config with the given sample rate
    fn try_create_config(
        &self,
        device: &cpal::Device,
        sample_rate: u32,
    ) -> Result<(cpal::StreamConfig, u32), String> {
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        // Validate by checking supported configs
        let supported = device
            .supported_input_configs()
            .map_err(|e| format!("Failed to get supported configs: {}", e))?;

        for supported_config in supported {
            if supported_config.min_sample_rate().0 <= sample_rate
                && supported_config.max_sample_rate().0 >= sample_rate
                && supported_config.channels() >= 1
            {
                return Ok((config, sample_rate));
            }
        }

        Err(format!("Sample rate {}Hz not supported", sample_rate))
    }

    fn build_stream<T>(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        is_recording: Arc<Mutex<bool>>,
        audio_buffer: Arc<Mutex<Vec<f32>>>,
    ) -> Result<Stream, String>
    where
        T: Sample + cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let err_fn = |err| eprintln!("[AUDIO] ❌ Stream error: {}", err);
        let sample_count = Arc::new(Mutex::new(0usize));
        let sample_count_clone = Arc::clone(&sample_count);
        let sample_rate = config.sample_rate.0;

        println!("[AUDIO] 🔧 Building input stream at {}Hz...", sample_rate);

        let callback_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let callback_count_clone = Arc::clone(&callback_count);
        let last_recording_state = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let last_recording_state_clone = Arc::clone(&last_recording_state);

        let stream = device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    let call_num =
                        callback_count_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    // Debug first few callbacks
                    if call_num < 3 {
                        println!("[AUDIO] 🔔 Callback #{}: {} samples", call_num, data.len());
                    }

                    let recording = *is_recording.lock().unwrap();

                    // Log state transitions
                    let was_recording = last_recording_state_clone
                        .swap(recording, std::sync::atomic::Ordering::Relaxed);
                    if recording != was_recording {
                        println!(
                            "[AUDIO] 📍 Recording state changed: {} -> {} at callback #{}",
                            was_recording, recording, call_num
                        );
                    }

                    if recording {
                        let mut buffer = audio_buffer.lock().unwrap();
                        let mut count = sample_count_clone.lock().unwrap();
                        for &sample in data {
                            buffer.push(sample.to_sample());
                        }
                        *count += data.len();
                        // Print progress every ~0.5 second of audio
                        let samples_per_half_sec = sample_rate as usize / 2;
                        if *count % samples_per_half_sec < data.len() {
                            println!(
                                "[AUDIO] 📊 Captured {} samples ({:.2}s at {}Hz)",
                                *count,
                                *count as f32 / sample_rate as f32,
                                sample_rate
                            );
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("Failed to build stream: {}", e))?;

        println!("[AUDIO] ✅ Input stream built successfully");
        Ok(stream)
    }

    pub fn stop_recording(&self) {
        *self.is_recording.lock().unwrap() = false;
        let buffer_size = self.audio_buffer.lock().unwrap().len();
        let actual_rate = *self.actual_sample_rate.lock().unwrap();
        let start_time = self.recording_start_time.lock().unwrap().take();

        let elapsed_ms = start_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);

        println!("[AUDIO] 🛑 Recording stopped after {}ms", elapsed_ms);
        println!(
            "[AUDIO] 🛑 Buffer: {} samples at {}Hz = {:.2}s of audio",
            buffer_size,
            actual_rate,
            buffer_size as f32 / actual_rate as f32
        );

        // Warn if there's a timing discrepancy
        let expected_samples = (elapsed_ms as f32 / 1000.0) * actual_rate as f32;
        if buffer_size > 0 && (buffer_size as f32) < expected_samples * 0.5 {
            println!("[AUDIO] ⚠️  WARNING: Only captured {:.1}% of expected audio! ({} vs {:.0} expected)",
                     100.0 * buffer_size as f32 / expected_samples, buffer_size, expected_samples);
            println!(
                "[AUDIO] ⚠️  This suggests audio callbacks aren't running or is_recording is false"
            );
        }
    }

    pub fn save_wav(&self, path: PathBuf) -> Result<(), String> {
        let buffer = self.get_buffer(); // This is already resampled to target rate

        let spec = WavSpec {
            channels: 1,
            sample_rate: self.target_sample_rate, // Use target rate since buffer is resampled
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = WavWriter::create(path, spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        for &sample in buffer.iter() {
            writer
                .write_sample(sample)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;

        Ok(())
    }

    /// Get audio buffer, resampled to target rate if necessary
    pub fn get_buffer(&self) -> Vec<f32> {
        let buffer = self.audio_buffer.lock().unwrap().clone();
        let actual_rate = *self.actual_sample_rate.lock().unwrap();

        if actual_rate == self.target_sample_rate {
            return buffer;
        }

        // Resample to target rate (linear interpolation)
        println!(
            "[AUDIO] 🔄 Resampling from {}Hz to {}Hz...",
            actual_rate, self.target_sample_rate
        );
        self.resample(&buffer, actual_rate, self.target_sample_rate)
    }

    /// Simple linear interpolation resampling
    fn resample(&self, input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
        if input.is_empty() {
            return Vec::new();
        }

        // Calculate output length: input_samples * (to_rate / from_rate)
        // Example: 48000 samples at 48kHz -> 16000 samples at 16kHz
        let output_len =
            ((input.len() as f64) * (to_rate as f64) / (from_rate as f64)).ceil() as usize;
        let mut output = Vec::with_capacity(output_len);

        // Ratio of input samples per output sample
        let step = from_rate as f64 / to_rate as f64;

        for i in 0..output_len {
            let src_idx = i as f64 * step;
            let idx0 = src_idx.floor() as usize;

            // Safety check for bounds
            if idx0 >= input.len() {
                break;
            }

            let idx1 = (idx0 + 1).min(input.len() - 1);
            let frac = src_idx - idx0 as f64;

            // Linear interpolation
            let sample = input[idx0] as f64 * (1.0 - frac) + input[idx1] as f64 * frac;
            output.push(sample as f32);
        }

        println!(
            "[AUDIO] ✅ Resampled {} -> {} samples ({}Hz -> {}Hz)",
            input.len(),
            output.len(),
            from_rate,
            to_rate
        );
        output
    }
}
