use std::collections::VecDeque;
use std::path::Path;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

const WHISPER_SAMPLE_RATE: u32 = 16000;
const SILERO_FRAME_MS: u32 = 30;
pub const SILERO_FRAME_SAMPLES: usize = (WHISPER_SAMPLE_RATE * SILERO_FRAME_MS / 1000) as usize; // 480

pub enum VadFrame<'a> {
    Speech(&'a [f32]),
    Noise,
}

pub trait VoiceActivityDetector: Send + Sync {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>, String>;
    fn is_voice(&mut self, frame: &[f32]) -> Result<bool, String> {
        Ok(matches!(self.push_frame(frame)?, VadFrame::Speech(_)))
    }
    fn reset(&mut self) {}
}

pub struct SileroVad {
    session: Session,
    h_data: Vec<f32>,
    c_data: Vec<f32>,
    threshold: f32,
}

impl SileroVad {
    pub fn new<P: AsRef<Path>>(model_path: P, threshold: f32) -> Result<Self, String> {
        if !(0.0..=1.0).contains(&threshold) {
            return Err("threshold must be between 0.0 and 1.0".to_string());
        }

        let mut builder = Session::builder()
            .map_err(|e| format!("ORT session builder error: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| format!("ORT optimization error: {e}"))?
            .with_intra_threads(1)
            .map_err(|e| format!("ORT threads error: {e}"))?
            .with_inter_threads(1)
            .map_err(|e| format!("ORT threads error: {e}"))?;

        let session = builder
            .commit_from_file(model_path.as_ref())
            .map_err(|e| format!("Failed to load VAD model: {e}"))?;

        Ok(Self {
            session,
            h_data: vec![0.0f32; 128],
            c_data: vec![0.0f32; 128],
            threshold,
        })
    }

    fn compute(&mut self, samples: &[f32]) -> Result<f32, String> {
        let input_tensor = Tensor::from_array(([1usize, samples.len()], samples.to_vec()))
            .map_err(|e| format!("VAD input tensor error: {e}"))?;

        let sr_tensor = Tensor::from_array(([1usize], vec![WHISPER_SAMPLE_RATE as i64]))
            .map_err(|e| format!("VAD sr tensor error: {e}"))?;

        let h_tensor = Tensor::from_array(([2usize, 1, 64], self.h_data.clone()))
            .map_err(|e| format!("VAD h tensor error: {e}"))?;

        let c_tensor = Tensor::from_array(([2usize, 1, 64], self.c_data.clone()))
            .map_err(|e| format!("VAD c tensor error: {e}"))?;

        let result = self
            .session
            .run(ort::inputs![
                "input" => input_tensor,
                "sr" => sr_tensor,
                "h" => h_tensor,
                "c" => c_tensor
            ])
            .map_err(|e| format!("VAD inference error: {e}"))?;

        // Update internal state tensors
        let (_, hn_data) = result
            .get("hn")
            .ok_or("Missing hn output")?
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("VAD hn extract error: {e}"))?;
        self.h_data = hn_data.to_vec();

        let (_, cn_data) = result
            .get("cn")
            .ok_or("Missing cn output")?
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("VAD cn extract error: {e}"))?;
        self.c_data = cn_data.to_vec();

        let (_, output_data) = result
            .get("output")
            .ok_or("Missing output")?
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("VAD output extract error: {e}"))?;

        let prob = *output_data.first().ok_or("Empty VAD output")?;
        Ok(prob)
    }
}

impl VoiceActivityDetector for SileroVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>, String> {
        if frame.len() != SILERO_FRAME_SAMPLES {
            return Err(format!(
                "expected {SILERO_FRAME_SAMPLES} samples, got {}",
                frame.len()
            ));
        }
        let prob = self.compute(frame)?;
        if prob > self.threshold {
            Ok(VadFrame::Speech(frame))
        } else {
            Ok(VadFrame::Noise)
        }
    }
}

pub struct SmoothedVad {
    inner_vad: Box<dyn VoiceActivityDetector>,
    prefill_frames: usize,
    hangover_frames: usize,
    onset_frames: usize,
    frame_buffer: VecDeque<Vec<f32>>,
    hangover_counter: usize,
    onset_counter: usize,
    in_speech: bool,
    temp_out: Vec<f32>,
}

impl SmoothedVad {
    pub fn new(
        inner_vad: Box<dyn VoiceActivityDetector>,
        prefill_frames: usize,
        hangover_frames: usize,
        onset_frames: usize,
    ) -> Self {
        Self {
            inner_vad,
            prefill_frames,
            hangover_frames,
            onset_frames,
            frame_buffer: VecDeque::new(),
            hangover_counter: 0,
            onset_counter: 0,
            in_speech: false,
            temp_out: Vec::new(),
        }
    }
}

impl VoiceActivityDetector for SmoothedVad {
    fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>, String> {
        // Ring of the last (prefill_frames + 1) frames. Recycle the evicted Vec
        // instead of allocating a fresh one (and freeing the old) every frame.
        let capacity = self.prefill_frames + 1;
        let mut buf = if self.frame_buffer.len() >= capacity {
            let mut recycled = self.frame_buffer.pop_front().unwrap_or_default();
            recycled.clear();
            recycled
        } else {
            Vec::with_capacity(frame.len())
        };
        buf.extend_from_slice(frame);
        self.frame_buffer.push_back(buf);
        // Defensive: keep the invariant even if `capacity` shrank between calls.
        while self.frame_buffer.len() > capacity {
            self.frame_buffer.pop_front();
        }

        let is_voice = self.inner_vad.is_voice(frame)?;

        match (self.in_speech, is_voice) {
            (false, true) => {
                self.onset_counter += 1;
                if self.onset_counter >= self.onset_frames {
                    self.in_speech = true;
                    self.hangover_counter = self.hangover_frames;
                    self.onset_counter = 0;

                    self.temp_out.clear();
                    for buf in &self.frame_buffer {
                        self.temp_out.extend(buf);
                    }
                    Ok(VadFrame::Speech(&self.temp_out))
                } else {
                    Ok(VadFrame::Noise)
                }
            }
            (true, true) => {
                self.hangover_counter = self.hangover_frames;
                Ok(VadFrame::Speech(frame))
            }
            (true, false) => {
                if self.hangover_counter > 0 {
                    self.hangover_counter -= 1;
                    Ok(VadFrame::Speech(frame))
                } else {
                    self.in_speech = false;
                    Ok(VadFrame::Noise)
                }
            }
            (false, false) => {
                self.onset_counter = 0;
                Ok(VadFrame::Noise)
            }
        }
    }

    fn reset(&mut self) {
        self.frame_buffer.clear();
        self.hangover_counter = 0;
        self.onset_counter = 0;
        self.in_speech = false;
        self.temp_out.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockVad {
        results: Vec<bool>,
        index: usize,
    }

    impl MockVad {
        fn new(results: Vec<bool>) -> Self {
            Self { results, index: 0 }
        }
    }

    impl VoiceActivityDetector for MockVad {
        fn push_frame<'a>(&'a mut self, frame: &'a [f32]) -> Result<VadFrame<'a>, String> {
            let is_speech = self.results.get(self.index).copied().unwrap_or(false);
            self.index += 1;
            if is_speech {
                Ok(VadFrame::Speech(frame))
            } else {
                Ok(VadFrame::Noise)
            }
        }
    }

    #[test]
    fn silero_vad_rejects_invalid_threshold() {
        assert!(SileroVad::new("/nonexistent", 1.5).is_err());
        assert!(SileroVad::new("/nonexistent", -0.1).is_err());
    }

    #[test]
    fn smoothed_vad_onset_detection() {
        let mock = MockVad::new(vec![true, true, false]);
        let mut vad = SmoothedVad::new(Box::new(mock), 3, 2, 2);
        let frame = vec![0.0f32; SILERO_FRAME_SAMPLES];

        assert!(matches!(vad.push_frame(&frame).unwrap(), VadFrame::Noise));
        assert!(matches!(
            vad.push_frame(&frame).unwrap(),
            VadFrame::Speech(_)
        ));
    }

    #[test]
    fn smoothed_vad_hangover() {
        let mock = MockVad::new(vec![true, true, false, false, false]);
        let mut vad = SmoothedVad::new(Box::new(mock), 1, 2, 2);
        let frame = vec![0.0f32; SILERO_FRAME_SAMPLES];

        vad.push_frame(&frame).unwrap(); // onset 1
        vad.push_frame(&frame).unwrap(); // onset 2 -> speech
        assert!(matches!(
            vad.push_frame(&frame).unwrap(),
            VadFrame::Speech(_)
        )); // hangover 2
        assert!(matches!(
            vad.push_frame(&frame).unwrap(),
            VadFrame::Speech(_)
        )); // hangover 1
        assert!(matches!(vad.push_frame(&frame).unwrap(), VadFrame::Noise));
    }

    #[test]
    fn smoothed_vad_reset() {
        let mock = MockVad::new(vec![true, true, true]);
        let mut vad = SmoothedVad::new(Box::new(mock), 2, 2, 1);
        let frame = vec![0.0f32; SILERO_FRAME_SAMPLES];

        vad.push_frame(&frame).unwrap();
        assert!(vad.in_speech);
        vad.reset();
        assert!(!vad.in_speech);
        assert!(vad.frame_buffer.is_empty());
    }

    #[test]
    fn smoothed_vad_buffer_stays_bounded() {
        // With buffer recycling, pushing far more frames than the prefill window
        // must keep the ring bounded at prefill_frames + 1 (no unbounded growth).
        let prefill = 3;
        let mock = MockVad::new(vec![false; 50]);
        let mut vad = SmoothedVad::new(Box::new(mock), prefill, 2, 2);
        let frame = vec![0.0f32; SILERO_FRAME_SAMPLES];
        for _ in 0..50 {
            vad.push_frame(&frame).unwrap();
        }
        assert_eq!(vad.frame_buffer.len(), prefill + 1);
        // Recycled buffers must still hold a full frame's worth of samples.
        assert!(vad
            .frame_buffer
            .iter()
            .all(|b| b.len() == SILERO_FRAME_SAMPLES));
    }
}
