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
    /// Forget everything the detector carries between frames, at a dictation
    /// boundary. Required rather than defaulted to a no-op: a detector that
    /// silently keeps its state across sessions eats the first words of the
    /// next one (see `SileroVad::reset`).
    fn reset(&mut self);
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
    /// Zero the recurrent state. Silero is an LSTM: `h`/`c` summarise everything
    /// it has heard, and at the end of a dictation they hold "this has been
    /// silence for a while". Carried into the next one, that bias can keep the
    /// detector below its threshold for several seconds of real speech — the
    /// beginning of the dictation is then dropped as noise before any engine
    /// sees it.
    fn reset(&mut self) {
        self.h_data.fill(0.0);
        self.c_data.fill(0.0);
    }

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
        // First: the wrapped detector carries its own state (see `SileroVad::reset`).
        self.inner_vad.reset();
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockVad {
        results: Vec<bool>,
        index: usize,
        /// Shared so the test can still read it after the mock is boxed away.
        resets: Arc<AtomicUsize>,
    }

    impl MockVad {
        fn new(results: Vec<bool>) -> Self {
            Self {
                results,
                index: 0,
                resets: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl VoiceActivityDetector for MockVad {
        fn reset(&mut self) {
            self.resets.fetch_add(1, Ordering::SeqCst);
            self.index = 0;
        }

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

    #[test]
    fn reset_reaches_the_wrapped_detector() {
        let mock = MockVad::new(vec![true; 4]);
        let resets = Arc::clone(&mock.resets);
        let mut vad = SmoothedVad::new(Box::new(mock), 15, 15, 2);

        vad.reset();

        assert_eq!(
            resets.load(Ordering::SeqCst),
            1,
            "a dictation boundary must clear the detector's own state, or it \
             carries the previous session's bias into the first words of the next"
        );
    }

    #[test]
    fn silero_reset_clears_the_recurrent_state() {
        let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models/silero_vad_v4.onnx");
        assert!(
            model.exists(),
            "the bundled VAD model moved: {}",
            model.display()
        );
        let Ok(mut vad) = SileroVad::new(&model, 0.3) else {
            // No usable ONNX runtime here; the propagation test above still covers
            // the wiring, which is what regressed. Loud, so it is never a silent pass.
            eprintln!("SKIPPED silero_reset_clears_the_recurrent_state: ORT unavailable");
            return;
        };

        // A ramp is enough to move the LSTM state away from zero.
        let frame: Vec<f32> = (0..SILERO_FRAME_SAMPLES)
            .map(|i| ((i as f32) / 50.0).sin() * 0.3)
            .collect();
        for _ in 0..5 {
            vad.push_frame(&frame).expect("frame accepted");
        }
        assert!(
            vad.h_data
                .iter()
                .chain(vad.c_data.iter())
                .any(|v| *v != 0.0),
            "the test needs a non-zero state to prove reset does something"
        );

        vad.reset();

        assert!(
            vad.h_data
                .iter()
                .chain(vad.c_data.iter())
                .all(|v| *v == 0.0),
            "reset must zero h/c, or Silero starts the next dictation biased towards silence"
        );
    }
}
