use std::path::PathBuf;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext};

pub struct STTEngine {
    ctx: WhisperContext,
    language: String,
}

impl STTEngine {
    pub fn new(model_path: PathBuf, language: String) -> Result<Self, String> {
        let ctx = WhisperContext::new(&model_path.to_string_lossy())
            .map_err(|e| format!("Failed to load Whisper model: {}", e))?;

        Ok(Self { ctx, language })
    }

    pub fn transcribe(&self, audio_data: &[f32]) -> Result<String, String> {
        // Calculate audio statistics for debugging
        let num_samples = audio_data.len();
        let duration_secs = num_samples as f32 / 16000.0;

        // Calculate RMS (Root Mean Square) for volume level
        let sum_squares: f32 = audio_data.iter().map(|x| x * x).sum();
        let rms = (sum_squares / num_samples as f32).sqrt();
        let max_amplitude = audio_data.iter().map(|x| x.abs()).fold(0.0f32, f32::max);

        println!("[STT] Audio: {} samples ({:.2}s), RMS: {:.4}, Max: {:.4}",
                 num_samples, duration_secs, rms, max_amplitude);

        if num_samples < 16000 {
            println!("[STT] ⚠️  Audio too short (< 1 second) - may not transcribe well");
        }

        if max_amplitude < 0.01 {
            println!("[STT] ⚠️  Audio level very low - microphone may not be working");
        }

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(&self.language));
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);

        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {}", e))?;

        state
            .full(params, audio_data)
            .map_err(|e| format!("Failed to process audio: {}", e))?;

        let num_segments = state
            .full_n_segments()
            .map_err(|e| format!("Failed to get segments: {}", e))?;

        println!("[STT] Whisper detected {} speech segments", num_segments);

        if num_segments == 0 {
            println!("[STT] ⚠️  No speech detected - try speaking louder or closer to microphone");
            return Ok(String::new());
        }

        let mut transcription = String::new();
        for i in 0..num_segments {
            let segment = state
                .full_get_segment_text(i)
                .map_err(|e| format!("Failed to get segment text: {}", e))?;
            println!("[STT] Segment {}: '{}'", i, segment.trim());
            transcription.push_str(&segment);
            transcription.push(' ');
        }

        let final_text = transcription.trim().to_string();
        println!("[STT] ✅ Final transcription: '{}'", final_text);

        Ok(final_text)
    }
}
