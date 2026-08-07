//! Sample-rate conversion from the capture device's rate down to the model's 16 kHz.
//!
//! Most devices hand us 44.1 or 48 kHz and the speech models want 16 kHz, so something
//! has to decimate. Doing it by picking every Nth sample — which is what this module
//! replaced — is wrong in two ways that both cost transcription accuracy:
//!
//! - **No anti-aliasing.** Everything above the 8 kHz Nyquist limit folds back down into
//!   the 0-8 kHz band the model listens to, so sibilance and any high-frequency noise
//!   reappear as spurious low-frequency content that was never spoken.
//! - **No memory between calls.** Audio arrives in whatever chunk size the driver feels
//!   like, and an index that restarts at zero each time makes the sampling grid jump at
//!   every buffer boundary — a discontinuity roughly every 85 ms.
//!
//! `AudioResampler` fixes both by keeping rubato's FFT resampler and its input buffer
//! alive across calls. That state is the whole point, and it is also the hazard: audio
//! that entered the resampler is still inside it, so a session boundary has to be
//! handled deliberately — `finish` to get the real audio out, `reset` to drop what is
//! left. Neither is optional; see their doc comments for which failure each prevents.

use rubato::{FftFixedIn, Resampler};

/// Input frames we hand rubato per call.
///
/// Note this is *not* how much audio the resampler holds. rubato derives its own FFT
/// block size from the rate pair and it is usually larger — 1026 frames at 48 kHz,
/// 1323 at 44.1 kHz — so it keeps unprocessed input of its own on top of ours, plus an
/// overlap-add tail. `finish` is sized against that, not against this constant.
const CHUNK_IN: usize = 1024;

/// Silent chunks `finish` feeds through to push the last real audio out.
///
/// Four chunks is 4096 input frames, comfortably more than the largest amount rubato
/// can be holding at any rate this app sees. A fixed count rather than a computed one
/// because the excess is silence, and `finish` trims it back off.
const FLUSH_CHUNKS: usize = 4;

pub(crate) struct AudioResampler {
    /// `None` when the device already gives us the target rate.
    resampler: Option<FftFixedIn<f32>>,
    in_buf: Vec<f32>,
    /// Pre-allocated so the steady-state path really does not allocate: rubato's
    /// `process` returns a fresh `Vec<Vec<f32>>` on every call, `process_into_buffer`
    /// does not.
    out_buf: Vec<Vec<f32>>,
    /// Whether anything has been fed since the last `reset`.
    ///
    /// `in_buf.is_empty()` is *not* a usable proxy for "the resampler is empty", and
    /// assuming it was is how the end of every recording went missing on hardware whose
    /// period size divides `CHUNK_IN`: `in_buf` is empty exactly then, while rubato is
    /// still holding a block of real audio that the following `reset` would destroy.
    fed: bool,
}

impl AudioResampler {
    pub(crate) fn new(in_hz: u32, out_hz: u32) -> Self {
        if in_hz == out_hz {
            return Self::passthrough();
        }

        match FftFixedIn::<f32>::new(
            in_hz as usize,
            out_hz as usize,
            CHUNK_IN,
            /* sub_chunks */ 1,
            /* channels */ 1,
        ) {
            Ok(resampler) => {
                let out_buf = resampler.output_buffer_allocate(true);
                Self {
                    resampler: Some(resampler),
                    in_buf: Vec::with_capacity(CHUNK_IN),
                    out_buf,
                    fed: false,
                }
            }
            Err(e) => {
                // Passing through means handing the model audio at the wrong rate, which
                // transcribes as gibberish that this app then types into whatever the
                // user has focused. Loud on purpose: `dev_log!` compiles to nothing in
                // release, which is exactly where this would need to be visible.
                eprintln!(
                    "[RESAMPLER] cannot convert {in_hz}Hz to {out_hz}Hz ({e}); audio will \
                     be at the wrong rate and transcription will be wrong"
                );
                Self::passthrough()
            }
        }
    }

    fn passthrough() -> Self {
        Self {
            resampler: None,
            in_buf: Vec::new(),
            out_buf: Vec::new(),
            fed: false,
        }
    }

    /// Feed captured samples in; converted audio is appended to `out`.
    ///
    /// `out` is appended to, not cleared — callers reusing a scratch buffer clear it
    /// themselves. Input that does not complete a chunk stays buffered until the next
    /// call, which is what keeps the output phase-continuous across driver callbacks.
    /// Appending nothing is normal, and always happens on the first call after a reset.
    pub(crate) fn push(&mut self, mut src: &[f32], out: &mut Vec<f32>) {
        if src.is_empty() {
            return;
        }
        self.fed = true;

        let Some(resampler) = self.resampler.as_mut() else {
            out.extend_from_slice(src);
            return;
        };

        while !src.is_empty() {
            let take = (CHUNK_IN - self.in_buf.len()).min(src.len());
            self.in_buf.extend_from_slice(&src[..take]);
            src = &src[take..];

            if self.in_buf.len() >= CHUNK_IN {
                Self::run(resampler, &self.in_buf, &mut self.out_buf, out);
                self.in_buf.clear();
            }
        }
    }

    /// Drain the end of a recording into `out`.
    ///
    /// When the user releases the key, real audio is still in three places: the partial
    /// chunk in `in_buf`, rubato's own unprocessed input (its FFT block is larger than
    /// `CHUNK_IN`), and its overlap-add tail. How much depends on the rate pair — around
    /// 43 ms at 48 kHz, and 64 ms of pure overlap delay on an 8 kHz Bluetooth headset.
    /// Skipping this loses the last word of every dictation, because the `reset` that
    /// follows throws all of it away.
    ///
    /// Silence is fed in to displace that audio, then trimmed back off so the recording
    /// does not gain a tail of dead air.
    pub(crate) fn finish(&mut self, out: &mut Vec<f32>) {
        if !self.fed {
            return;
        }
        self.fed = false;

        let Some(resampler) = self.resampler.as_mut() else {
            return;
        };

        let start = out.len();

        // A partial chunk gets padded; an empty one becomes a full silent chunk, which
        // is exactly what the chunk-aligned case needs.
        self.in_buf.resize(CHUNK_IN, 0.0);
        for _ in 0..FLUSH_CHUNKS {
            Self::run(resampler, &self.in_buf, &mut self.out_buf, out);
            self.in_buf.clear();
            self.in_buf.resize(CHUNK_IN, 0.0);
        }
        self.in_buf.clear();

        // Drop the silence we just fed through, keeping the audio it displaced.
        let kept = out[start..]
            .iter()
            .rposition(|s| *s != 0.0)
            .map_or(start, |i| start + i + 1);
        out.truncate(kept);
    }

    /// Drop whatever is still inside, without emitting it.
    ///
    /// Two distinct reasons:
    ///
    /// - After `finish`, the silence used to displace the real audio is itself sitting
    ///   in rubato's overlap buffers. Clearing it stops that colouring what comes next.
    /// - When a recording never reaches `finish` — abandoned, an error path, a Start
    ///   with no matching Stop — the overlap still holds the previous audio, which would
    ///   otherwise surface at the beginning of the following recording.
    ///
    /// Only the second has a negative control below; the first is why `reset` is still
    /// called on Stop even though `finish` has just run.
    pub(crate) fn reset(&mut self) {
        self.in_buf.clear();
        self.fed = false;
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
    }

    /// One FFT pass, appending whatever came out. Shared by `push` and `finish` so the
    /// two cannot drift apart on buffer handling.
    fn run(
        resampler: &mut FftFixedIn<f32>,
        input: &[f32],
        scratch: &mut [Vec<f32>],
        out: &mut Vec<f32>,
    ) {
        match resampler.process_into_buffer(&[input], scratch, None) {
            Ok((_, frames_out)) => out.extend_from_slice(&scratch[0][..frames_out]),
            Err(e) => crate::dev_log!("[RESAMPLER] process failed: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN_HZ: u32 = 48_000;
    const OUT_HZ: u32 = 16_000;

    fn tone(sample_rate: u32, freq: f64, secs: f64) -> Vec<f32> {
        let n = (sample_rate as f64 * secs) as usize;
        (0..n)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate as f64).sin() as f32
            })
            .collect()
    }

    fn collect(r: &mut AudioResampler, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        r.push(input, &mut out);
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().map(|s| s.abs()).fold(0.0, f32::max)
    }

    /// Rough energy at one frequency, by correlating against a reference sinusoid.
    fn magnitude_at(samples: &[f32], sample_rate: u32, freq: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in samples.iter().enumerate() {
            let phase = 2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate as f64;
            re += s as f64 * phase.cos();
            im += s as f64 * phase.sin();
        }
        2.0 * (re * re + im * im).sqrt() / samples.len() as f64
    }

    #[test]
    fn equal_rates_pass_through_untouched() {
        let mut r = AudioResampler::new(OUT_HZ, OUT_HZ);
        let input = tone(OUT_HZ, 440.0, 0.05);
        assert_eq!(collect(&mut r, &input), input);
    }

    #[test]
    fn a_full_recording_keeps_essentially_all_of_its_audio() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let input = tone(IN_HZ, 1000.0, 1.0);
        let mut out = collect(&mut r, &input);
        r.finish(&mut out);

        let expected = OUT_HZ as usize;
        let lost_ms = expected.saturating_sub(out.len()) * 1000 / OUT_HZ as usize;
        assert!(lost_ms < 5, "lost {}ms of a 1s recording", lost_ms);
    }

    /// Audio arrives in whatever sizes the driver chooses; the seams must not show.
    #[test]
    fn chunked_input_matches_single_shot() {
        let input = tone(IN_HZ, 1000.0, 0.5);

        let mut whole = AudioResampler::new(IN_HZ, OUT_HZ);
        let reference = collect(&mut whole, &input);

        // Deliberately awkward sizes, none a multiple of CHUNK_IN.
        let mut chunked = AudioResampler::new(IN_HZ, OUT_HZ);
        let mut got = Vec::new();
        let mut offset = 0;
        for size in [37usize, 1023, 4096, 1, 512].iter().cycle() {
            if offset >= input.len() {
                break;
            }
            let end = (offset + size).min(input.len());
            chunked.push(&input[offset..end], &mut got);
            offset = end;
        }

        assert_eq!(got.len(), reference.len());
        for (i, (a, b)) in got.iter().zip(reference.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "sample {} differs: {} vs {}", i, a, b);
        }
    }

    /// A 1 kHz tone is squarely inside the speech band and must survive conversion.
    #[test]
    fn a_speech_band_tone_survives_conversion() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let out = collect(&mut r, &tone(IN_HZ, 1000.0, 0.5));

        let settled = &out[out.len() / 4..];
        let signal = magnitude_at(settled, OUT_HZ, 1000.0);
        assert!(
            signal > 0.8,
            "1kHz tone lost too much amplitude: {}",
            signal
        );
    }

    /// The anti-aliasing property, and the reason this module exists.
    ///
    /// 9 kHz is above the 8 kHz Nyquist limit of the 16 kHz output, so it cannot be
    /// represented. A correct resampler filters it out. Decimation without a filter
    /// folds it down to |16000 - 9000| = 7 kHz, planting a loud tone in the middle of
    /// the speech band that was never spoken.
    #[test]
    fn a_tone_above_nyquist_is_rejected_not_folded_down() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let out = collect(&mut r, &tone(IN_HZ, 9000.0, 0.5));

        let settled = &out[out.len() / 4..];
        let alias = magnitude_at(settled, OUT_HZ, 7000.0);
        assert!(
            alias < 0.05,
            "9kHz folded down to 7kHz with magnitude {} — anti-aliasing is not working",
            alias
        );
        assert!(peak(settled) < 0.1, "above-Nyquist energy leaked through");
    }

    /// The regression that made this module's whole point moot on common hardware: a
    /// driver period dividing `CHUNK_IN` leaves `in_buf` empty at Stop while rubato is
    /// still holding real audio, so keying `finish` off `in_buf` dropped the last word.
    #[test]
    fn finish_drains_a_recording_that_ends_on_a_chunk_boundary() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let input: Vec<f32> = (0..4 * CHUNK_IN)
            .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / IN_HZ as f64).sin() as f32)
            .collect();

        let pushed = collect(&mut r, &input);
        assert!(
            r.in_buf.is_empty(),
            "precondition: recording ended chunk-aligned"
        );

        let mut flushed = Vec::new();
        r.finish(&mut flushed);

        assert!(!flushed.is_empty(), "chunk-aligned recording lost its tail");
        assert!(
            peak(&flushed) > 0.5,
            "the flushed tail is silence, not the last word"
        );

        let expected = input.len() * OUT_HZ as usize / IN_HZ as usize;
        assert!(
            pushed.len() + flushed.len() >= expected,
            "lost {} of {} samples",
            expected.saturating_sub(pushed.len() + flushed.len()),
            expected
        );
    }

    #[test]
    fn finish_drains_a_recording_that_ends_mid_chunk() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let input = tone(IN_HZ, 1000.0, 0.1); // 4800 frames: 4 chunks + 704
        let pushed = collect(&mut r, &input);
        assert!(!r.in_buf.is_empty(), "precondition: ended mid-chunk");

        let mut flushed = Vec::new();
        r.finish(&mut flushed);

        assert!(!flushed.is_empty(), "the end of the recording was dropped");
        assert!(peak(&flushed) > 0.5, "flushed tail is silence, not audio");

        let expected = input.len() * OUT_HZ as usize / IN_HZ as usize;
        assert!(
            pushed.len() + flushed.len() >= expected,
            "lost {} of {} samples",
            expected.saturating_sub(pushed.len() + flushed.len()),
            expected
        );
    }

    /// `finish` pads with silence to flush; that padding must not end up in the audio.
    #[test]
    fn finish_does_not_append_a_tail_of_silence() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let _ = collect(&mut r, &tone(IN_HZ, 1000.0, 0.1));

        let mut flushed = Vec::new();
        r.finish(&mut flushed);

        assert!(
            *flushed.last().expect("nothing flushed") != 0.0,
            "flush left a run of trailing silence in the recording"
        );
    }

    #[test]
    fn finish_leaves_no_tail_for_the_next_session() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);

        let mut sink = Vec::new();
        r.push(&[0.5f32; 100], &mut sink);
        r.finish(&mut sink);

        // One fresh full chunk must produce at most one chunk's worth of output. If
        // `finish` had left its padded input behind, this would emit more.
        let emitted = collect(&mut r, &[0.25f32; CHUNK_IN]);
        assert!(
            emitted.len() <= CHUNK_IN * OUT_HZ as usize / IN_HZ as usize + 1,
            "finish() leaked its padded tail: {} samples",
            emitted.len()
        );
    }

    /// A recording that ended normally must not bleed into the next one.
    #[test]
    fn no_crosstalk_after_a_finished_recording() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);

        let mut sink = Vec::new();
        r.push(&tone(IN_HZ, 1000.0, 0.5), &mut sink);
        r.finish(&mut sink);
        r.reset();

        let out = collect(&mut r, &vec![0.0f32; 4 * CHUNK_IN]);
        assert!(
            peak(&out) < 0.01,
            "audio from the previous recording leaked in: peak {}",
            peak(&out)
        );
    }

    /// `reset` guards the case `finish` cannot: a recording that never reached Stop —
    /// an aborted dictation, an error path, a Start with no matching Stop.
    #[test]
    fn reset_clears_the_state_of_an_unfinished_recording() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);

        let _ = collect(&mut r, &tone(IN_HZ, 1000.0, 0.5));
        // no finish() — the recording was abandoned
        r.reset();

        let out = collect(&mut r, &vec![0.0f32; 4 * CHUNK_IN]);
        assert!(
            peak(&out) < 0.01,
            "abandoned recording leaked into the next one: peak {}",
            peak(&out)
        );
    }

    /// Negative control for the test above: without `reset`, that abandoned recording
    /// really does leak. If this stops failing, `reset` has become redundant and should
    /// be reconsidered rather than kept out of habit.
    #[test]
    fn without_reset_an_unfinished_recording_leaks() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);

        let _ = collect(&mut r, &tone(IN_HZ, 1000.0, 0.5));
        // deliberately neither finish() nor reset()

        let out = collect(&mut r, &vec![0.0f32; 4 * CHUNK_IN]);
        assert!(
            peak(&out) > 0.01,
            "expected leakage without reset, got peak {}",
            peak(&out)
        );
    }

    #[test]
    fn reset_is_safe_in_passthrough_mode() {
        let mut r = AudioResampler::new(OUT_HZ, OUT_HZ);
        let mut sink = Vec::new();
        r.push(&[1.0f32; 200], &mut sink);
        r.reset();
        r.finish(&mut sink);
        assert_eq!(collect(&mut r, &[0.0f32; 10]), vec![0.0f32; 10]);
    }

    #[test]
    fn finish_without_any_input_emits_nothing() {
        let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
        let mut out = Vec::new();
        r.finish(&mut out);
        assert!(out.is_empty());
    }

    /// MEASUREMENT (AC7): quantify the old nearest-neighbour decimator against this one
    /// on signals whose correct answer is known analytically. The assertion at the end
    /// makes it a gate; the numbers are the point, and print with `--nocapture`.
    #[test]
    fn measured_comparison_against_the_old_decimator() {
        /// The implementation this module replaced, verbatim from audio.rs.
        fn old_downsample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
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

        // Emulate real capture: the driver hands over chunks, not one contiguous block.
        fn old_chunked(input: &[f32]) -> Vec<f32> {
            let mut out = Vec::new();
            for chunk in input.chunks(4096) {
                out.extend(old_downsample(chunk, IN_HZ, OUT_HZ));
            }
            out
        }
        fn new_chunked(input: &[f32]) -> Vec<f32> {
            let mut r = AudioResampler::new(IN_HZ, OUT_HZ);
            let mut out = Vec::new();
            for chunk in input.chunks(4096) {
                r.push(chunk, &mut out);
            }
            r.finish(&mut out);
            out
        }

        eprintln!("\n=== RESAMPLING QUALITY: 48kHz -> 16kHz ===\n");

        let hi = tone(IN_HZ, 9000.0, 0.5);
        let old_hi = old_chunked(&hi);
        let new_hi = new_chunked(&hi);
        let old_alias = magnitude_at(&old_hi[old_hi.len() / 4..], OUT_HZ, 7000.0);
        let new_alias = magnitude_at(&new_hi[new_hi.len() / 4..], OUT_HZ, 7000.0);
        eprintln!("9kHz tone -> spurious 7kHz energy in the speech band");
        eprintln!("  old (nearest-neighbour): {:.4}", old_alias);
        eprintln!("  new (FFT anti-aliased):  {:.4}", new_alias);
        eprintln!(
            "  rejection: {:.1} dB\n",
            20.0 * (old_alias / new_alias.max(1e-9)).log10()
        );

        let lo = tone(IN_HZ, 1000.0, 0.5);
        let old_lo = old_chunked(&lo);
        let new_lo = new_chunked(&lo);
        // Middle half only, for both: the new path's `finish` appends the recording's
        // end transient, which the old one simply discarded. Measuring the settled
        // region compares the resamplers rather than what each does at the edges.
        let settled =
            |v: &[f32]| -> f64 { magnitude_at(&v[v.len() / 4..3 * v.len() / 4], OUT_HZ, 1000.0) };
        eprintln!("1kHz tone -> retained amplitude, settled region (1.0 is perfect)");
        eprintln!("  old: {:.4}", settled(&old_lo));
        eprintln!("  new: {:.4}\n", settled(&new_lo));

        // Chunk-boundary continuity, against the same input resampled in one block.
        let mut whole = AudioResampler::new(IN_HZ, OUT_HZ);
        let mut ideal = Vec::new();
        whole.push(&lo, &mut ideal);
        whole.finish(&mut ideal);
        let old_single = old_downsample(&lo, IN_HZ, OUT_HZ);

        let seam_err = |chunked: &[f32], single: &[f32]| -> f32 {
            let n = chunked.len().min(single.len());
            (0..n)
                .map(|i| (chunked[i] - single[i]).abs())
                .fold(0.0, f32::max)
        };
        eprintln!("chunked vs single-block -> max per-sample deviation");
        eprintln!("  old: {:.6}", seam_err(&old_lo, &old_single));
        eprintln!("  new: {:.6}\n", seam_err(&new_lo, &ideal));

        assert!(
            new_alias < old_alias / 10.0,
            "no meaningful aliasing improvement"
        );
    }
}
