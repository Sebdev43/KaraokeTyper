use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, Sample, SampleFormat, Stream, StreamConfig};
use hound::{WavSpec, WavWriter};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct AudioCapture {
    pub is_recording: Arc<Mutex<bool>>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
}

impl AudioCapture {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            is_recording: Arc::new(Mutex::new(false)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            sample_rate,
        }
    }

    pub fn start_recording(&self) -> Result<Stream, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No input device available")?;

        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?;

        let sample_format = config.sample_format();
        let config: StreamConfig = config.into();

        let is_recording = Arc::clone(&self.is_recording);
        let audio_buffer = Arc::clone(&self.audio_buffer);

        *is_recording.lock().unwrap() = true;
        audio_buffer.lock().unwrap().clear();

        let stream = match sample_format {
            SampleFormat::F32 => self.build_stream::<f32>(&device, &config, is_recording, audio_buffer)?,
            SampleFormat::I16 => self.build_stream::<i16>(&device, &config, is_recording, audio_buffer)?,
            SampleFormat::U16 => self.build_stream::<u16>(&device, &config, is_recording, audio_buffer)?,
            _ => return Err("Unsupported sample format".to_string()),
        };

        stream.play().map_err(|e| format!("Failed to play stream: {}", e))?;
        Ok(stream)
    }

    fn build_stream<T>(
        &self,
        device: &Device,
        config: &StreamConfig,
        is_recording: Arc<Mutex<bool>>,
        audio_buffer: Arc<Mutex<Vec<f32>>>,
    ) -> Result<Stream, String>
    where
        T: Sample + cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let channels = config.channels as usize;

        let stream = device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    if *is_recording.lock().unwrap() {
                        let mut buffer = audio_buffer.lock().unwrap();
                        for &sample in data.iter() {
                            buffer.push(cpal::Sample::from_sample(sample));
                        }
                    }
                },
                move |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        Ok(stream)
    }

    pub fn stop_recording(&self) {
        *self.is_recording.lock().unwrap() = false;
    }

    pub fn save_wav(&self, path: PathBuf) -> Result<(), String> {
        let buffer = self.audio_buffer.lock().unwrap();

        let spec = WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = WavWriter::create(path, spec)
            .map_err(|e| format!("Failed to create WAV file: {}", e))?;

        for &sample in buffer.iter() {
            let amplitude = i16::MAX as f32;
            writer
                .write_sample((sample * amplitude) as i16)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV file: {}", e))?;

        Ok(())
    }

    pub fn get_buffer(&self) -> Vec<f32> {
        self.audio_buffer.lock().unwrap().clone()
    }
}
