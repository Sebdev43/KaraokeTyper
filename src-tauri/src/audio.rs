//! Audio capture module - CPAL (cross-platform)
//!
//! Uses CPAL for audio capture on all platforms:
//! - Linux: ALSA backend (transparent PipeWire support via pipewire-alsa)
//! - Windows: WASAPI backend
//! - macOS: CoreAudio backend
//!
//! Architecture (worker thread — inspired by Handy):
//! - `open()` spawns a worker thread that builds the CPAL stream and starts it playing.
//!   Audio samples flow continuously through an mpsc channel into the worker.
//! - `start_recording()` sends `Cmd::Start` — the worker clears its buffer and begins
//!   accumulating samples. This is instant regardless of stream state.
//! - `stop_recording()` sends `Cmd::Stop` — the worker drains remaining samples,
//!   runs VAD flush, and returns the buffer via a reply channel.
//! - `close()` sends `Cmd::Shutdown` and joins the worker thread.
//! - The worker releases a dead stream on its own: a playing input stream delivers
//!   continuously, so a long gap without samples while idle means the device is gone
//!   (cpal may or may not have reported it). The next `open_stream()` rebuilds it.
//!
//! This architecture eliminates PTT race conditions: commands are queued and processed
//! only after the stream is fully open, because `run_worker()` only starts its loop
//! after `stream.play()` succeeds.

use crate::resampler::AudioResampler;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

/// Pre-roll duration: audio captured *before* the user presses PTT is prepended
/// to the recording so the first syllable of the phrase isn't lost to the
/// reaction-time gap between speaking and pressing the hotkey.
/// 500 ms @ 16 kHz = 8000 samples (comparable to Handy's 450 ms prefill).
const PRE_ROLL_MS: usize = 500;

use crate::vad::{SmoothedVad, VadFrame, VoiceActivityDetector, SILERO_FRAME_SAMPLES};

// ── Commands processed by the worker thread ──────────────────────────────────

enum Cmd {
    Start,
    Stop(mpsc::Sender<Vec<f32>>),
    Shutdown,
}

// ── Scope guard (RAII cleanup on drop, including panic unwind) ────────────────

struct ScopeGuard<T, F: FnOnce(T)> {
    value: Option<T>,
    f: Option<F>,
}

impl<T, F: FnOnce(T)> Drop for ScopeGuard<T, F> {
    fn drop(&mut self) {
        if let (Some(v), Some(f)) = (self.value.take(), self.f.take()) {
            f(v);
        }
    }
}

fn scopeguard<T, F: FnOnce(T)>(value: T, f: F) -> ScopeGuard<T, F> {
    ScopeGuard {
        value: Some(value),
        f: Some(f),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Validate device names to prevent command injection via pactl
fn is_valid_device_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_alphanumeric() || "_.:-,=".contains(c))
}

/// Gap without samples after which an idle worker gives its stream up: a playing input
/// stream delivers continuously (silence included), so this long a gap means the device
/// is gone — on PipeWire a lost device stalls the ALSA plugin without any error callback.
const STALLED_STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Same, before the first sample ever arrived: some devices (Bluetooth HFP) take a few
/// seconds to start delivering, and giving up early would waste the warm-up open.
const STARTUP_STREAM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Delay before auto-closing the stream after recording stops (releases mic device)
const STREAM_AUTO_CLOSE_SECS: u64 = 10;

/// Capture buffers the worker may fall behind by before samples start being dropped.
///
/// The queue used to be unbounded, which was survivable while the callback decimated to
/// 16 kHz first. It no longer does — the worker resamples now — so the same stall costs
/// roughly three times the memory per second, and nothing capped it. At a typical
/// 1024-frame period this is about four seconds of audio and under a megabyte.
///
/// Dropping is the right failure: the alternative in a real-time callback is blocking,
/// which does not make the audio arrive, it makes the *device* glitch as well.
const SAMPLE_QUEUE_BUFFERS: usize = 200;

// ── Public API ───────────────────────────────────────────────────────────────

/// Audio capture using a dedicated worker thread.
///
/// The worker owns the CPAL stream and processes samples continuously.
/// Commands (`Start`/`Stop`/`Shutdown`) are sent via an mpsc channel.
pub struct AudioCapture {
    target_sample_rate: u32,
    cmd_tx: Option<mpsc::Sender<Cmd>>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    /// VAD engine — shared with the worker via Arc<Mutex<>>
    vad: Arc<Mutex<Option<SmoothedVad>>>,
    /// Whether the worker is currently open (stream active)
    is_open: Arc<AtomicBool>,
    /// Set by cpal's error callback, cleared by the next data callback and by
    /// `close_stream()`: `true` means cpal reported an error and no audio has arrived
    /// since. Lets `is_stream_open()` report the loss at once, before the worker
    /// notices the gap itself.
    stream_error: Arc<AtomicBool>,
    /// Whether current device is Bluetooth (for auto-close logic)
    is_bluetooth: AtomicBool,
    /// Generation counter for Bluetooth auto-close timer
    stop_generation: Arc<AtomicU64>,
}

impl AudioCapture {
    pub fn new(target_sample_rate: u32) -> Self {
        Self {
            target_sample_rate,
            cmd_tx: None,
            worker_handle: None,
            vad: Arc::new(Mutex::new(None)),
            is_open: Arc::new(AtomicBool::new(false)),
            stream_error: Arc::new(AtomicBool::new(false)),
            is_bluetooth: AtomicBool::new(false),
            stop_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Set the Voice Activity Detection engine.
    pub fn set_vad(&self, vad: SmoothedVad) {
        *self.vad.lock().unwrap() = Some(vad);
    }

    /// Check if a stream is currently open and has not reported an error.
    pub fn is_stream_open(&self) -> bool {
        self.is_open.load(Ordering::SeqCst) && !self.stream_error.load(Ordering::SeqCst)
    }

    /// Open the audio stream for a device.
    ///
    /// Spawns a worker thread that:
    /// 1. Resolves the CPAL device (slow for ALSA — ~1-2s for enumeration)
    /// 2. Builds and starts the input stream
    /// 3. Runs the worker loop processing samples and commands
    ///
    /// Device matching:
    /// - `None`, `"pipewire"`, `"default"`, `""` → system default (fast, no enumeration)
    /// - `"plughw:CARD=..."` → ALSA hardware device (direct CPAL open)
    /// - PipeWire source name (bluez, alsa_input, etc.) → route via pactl + CPAL default
    pub fn open_stream(&mut self, device_name: Option<&str>) -> Result<(), String> {
        // Close any existing worker first
        self.close_stream();

        // Track if this is a Bluetooth device (for auto-close logic)
        let is_bt = device_name
            .map(|n| n.starts_with("bluez_input."))
            .unwrap_or(false);
        self.is_bluetooth.store(is_bt, Ordering::Relaxed);

        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (sample_tx, sample_rx) = mpsc::sync_channel::<Vec<f32>>(SAMPLE_QUEUE_BUFFERS);

        let target_rate = self.target_sample_rate;
        let vad = Arc::clone(&self.vad);
        let is_open = Arc::clone(&self.is_open);
        let stream_error = Arc::clone(&self.stream_error);
        let device_name_owned = device_name.map(|s| s.to_string());

        let worker = std::thread::spawn(move || {
            // Guard: ensure is_open is always set to false on exit (normal or panic)
            let _guard = scopeguard(Arc::clone(&is_open), |flag| {
                flag.store(false, Ordering::SeqCst);
                crate::dev_log!("[AUDIO/WORKER] Worker thread exited, stream released");
            });

            // ── Step 1: Resolve device (can be slow for ALSA) ──
            let device = match resolve_device(device_name_owned.as_deref()) {
                Ok(d) => d,
                Err(e) => {
                    crate::dev_log!("[AUDIO/WORKER] Failed to resolve device: {}", e);
                    return;
                }
            };

            let device_display = device.name().unwrap_or_else(|_| "unknown".to_string());
            crate::dev_log!("[AUDIO/WORKER] Selected device: {}", device_display);

            // ── Step 2: Negotiate config and build stream ──
            let (config, actual_rate) = match negotiate_config(&device, target_rate) {
                Ok(c) => c,
                Err(e) => {
                    crate::dev_log!("[AUDIO/WORKER] Config negotiation failed: {}", e);
                    return;
                }
            };

            crate::dev_log!(
                "[AUDIO/WORKER] Stream config: {}ch, {}Hz (target: {}Hz)",
                config.channels,
                actual_rate,
                target_rate
            );

            let sample_tx_clone = sample_tx;
            // Dropped-buffer counter, reported once per burst rather than per buffer:
            // a stalled worker would otherwise turn one problem into a log flood.
            let mut dropped: u64 = 0;
            let stream_error_data = Arc::clone(&stream_error);
            let stream = match device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Audio is flowing, so whatever cpal reported was transient (ALSA
                    // reports an overrun it hits mid-read, then recovers by itself).
                    stream_error_data.store(false, Ordering::SeqCst);
                    // Rate conversion used to happen here, on the audio thread. It now
                    // happens on the worker, where it can take as long as it needs
                    // without risking a dropout, and where it can keep state across
                    // callbacks — which is what makes it correct rather than merely
                    // cheap. What remains is still not RT-safe (`to_vec` and the channel
                    // send both allocate), but it is a great deal less work than an FFT.
                    //
                    // `try_send`, never `send`: the queue is bounded now, and blocking
                    // the audio thread to wait for a slow worker would add a device-level
                    // glitch on top of the backlog it was waiting on.
                    match sample_tx_clone.try_send(data.to_vec()) {
                        Ok(()) => {
                            if dropped > 0 {
                                crate::dev_log!(
                                    "[AUDIO/CPAL] Worker caught up after dropping {} buffers",
                                    dropped
                                );
                                dropped = 0;
                            }
                        }
                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                            // Say it the first time, then stay quiet until it recovers.
                            if dropped == 0 {
                                crate::dev_log!(
                                    "[AUDIO/CPAL] Sample queue full ({} buffers) — worker is \
                                     behind, dropping audio",
                                    SAMPLE_QUEUE_BUFFERS
                                );
                            }
                            dropped += 1;
                        }
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {}
                    }
                },
                move |err| {
                    crate::dev_log!("[AUDIO/WORKER] Stream error: {}", err);
                    stream_error.store(true, Ordering::SeqCst);
                },
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    crate::dev_log!("[AUDIO/WORKER] Failed to build stream: {}", e);
                    return;
                }
            };

            // ── Step 3: Start playing (mic is ON, samples flow) ──
            if let Err(e) = stream.play() {
                crate::dev_log!("[AUDIO/WORKER] Failed to play stream: {}", e);
                return;
            }

            is_open.store(true, Ordering::SeqCst);
            crate::dev_log!(
                "[AUDIO/WORKER] Stream opened and playing at {}Hz from '{}'",
                actual_rate,
                device_display
            );

            // ── Step 4: Worker loop — process samples + commands ──
            run_worker(vad, sample_rx, cmd_rx, actual_rate, target_rate);

            // The worker no longer accepts commands: say so before the stream teardown
            // below, so a PTT cannot land in between. (_guard repeats it on any exit,
            // including a panic unwind.)
            is_open.store(false, Ordering::SeqCst);
        });

        self.cmd_tx = Some(cmd_tx);
        self.worker_handle = Some(worker);

        // Note: we return immediately. The worker thread opens the device asynchronously.
        // Commands sent before the worker loop starts are queued in the mpsc channel and
        // processed as soon as the stream is ready. This eliminates the race condition.
        Ok(())
    }

    /// Close the audio stream and release the device.
    pub fn close_stream(&mut self) {
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(h) = self.worker_handle.take() {
            if let Err(panic_payload) = h.join() {
                crate::dev_log!(
                    "[AUDIO/WORKER] Worker thread panicked: {:?}",
                    panic_payload
                        .downcast_ref::<&str>()
                        .unwrap_or(&"unknown panic")
                );
            }
        }
        self.is_open.store(false, Ordering::SeqCst);
        // Joined above: no callback of that stream can set this again.
        self.stream_error.store(false, Ordering::SeqCst);
    }

    /// Start recording — sends `Cmd::Start` to the worker (instant).
    /// The worker clears its buffer and begins accumulating samples.
    pub fn start_recording(&self) -> Result<(), String> {
        // Cancel any pending Bluetooth auto-close timer
        self.stop_generation.fetch_add(1, Ordering::SeqCst);

        let tx = self.cmd_tx.as_ref().ok_or("Audio worker not running")?;
        tx.send(Cmd::Start)
            .map_err(|_| "Audio worker channel closed".to_string())?;

        crate::dev_log!("[AUDIO/CPAL] Start command sent to worker");
        Ok(())
    }

    /// Send a Stop command to the worker. Returns a `StopHandle` to wait for results.
    ///
    /// This is a non-blocking send — call `StopHandle::wait()` AFTER dropping the Mutex
    /// to avoid holding it during the blocking recv (up to 5s).
    pub fn stop_recording(&self) -> Result<StopHandle, String> {
        let tx = self.cmd_tx.as_ref().ok_or("Audio worker not running")?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(Cmd::Stop(reply_tx))
            .map_err(|_| "Audio worker channel closed".to_string())?;

        // Schedule Bluetooth auto-close if applicable
        if self.is_bluetooth.load(Ordering::Relaxed) {
            let gen = self.stop_generation.fetch_add(1, Ordering::SeqCst) + 1;
            let gen_ref = Arc::clone(&self.stop_generation);
            let is_open = Arc::clone(&self.is_open);
            let cmd_tx = self.cmd_tx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(STREAM_AUTO_CLOSE_SECS));
                if gen_ref.load(Ordering::SeqCst) == gen && is_open.load(Ordering::SeqCst) {
                    if let Some(tx) = cmd_tx {
                        let _ = tx.send(Cmd::Shutdown);
                    }
                    is_open.store(false, Ordering::SeqCst);
                    crate::dev_log!(
                        "[AUDIO/CPAL] BT auto-close: sent Shutdown after {}s idle",
                        STREAM_AUTO_CLOSE_SECS
                    );
                }
            });

            crate::dev_log!(
                "[AUDIO/CPAL] Will auto-close in {}s if idle (BT device)",
                STREAM_AUTO_CLOSE_SECS
            );
        }

        Ok(StopHandle { reply_rx })
    }
}

/// Handle returned by `stop_recording()`. Call `wait()` to get the audio buffer.
/// This lets you drop the `AudioCapture` Mutex before blocking on the reply.
pub struct StopHandle {
    reply_rx: mpsc::Receiver<Vec<f32>>,
}

impl StopHandle {
    /// Wait for the worker to return the audio buffer (blocks up to 5s).
    pub fn wait(self) -> Result<Vec<f32>, String> {
        let audio_data = self
            .reply_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "Timeout waiting for audio data from worker".to_string())?;

        crate::dev_log!(
            "[AUDIO/CPAL] Recording stopped: {} samples ({:.2}s at 16000Hz)",
            audio_data.len(),
            audio_data.len() as f32 / 16000.0
        );

        Ok(audio_data)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.close_stream();
    }
}

// ── Worker thread ────────────────────────────────────────────────────────────

fn run_worker(
    vad: Arc<Mutex<Option<SmoothedVad>>>,
    sample_rx: mpsc::Receiver<Vec<f32>>,
    cmd_rx: mpsc::Receiver<Cmd>,
    actual_rate: u32,
    target_rate: u32,
) {
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut frame_accumulator: Vec<f32> = Vec::new();
    let mut recording = false;
    let mut first_callback_logged = false;
    let mut last_audio = std::time::Instant::now();
    let mut ever_received = false;

    // Everything below this point works in target-rate samples. The device may deliver
    // any rate, so conversion happens here, once, on the way in — and the resampler is
    // stateful, which is what keeps the output continuous across callback boundaries.
    let mut resampler = AudioResampler::new(actual_rate, target_rate);
    // Reused across chunks; the resampler holds its own output buffer, so together they
    // keep the steady-state path free of per-chunk allocation.
    let mut converted: Vec<f32> = Vec::new();

    // Ring buffer holding the last PRE_ROLL_MS of audio captured while NOT recording.
    // Prepended to audio_buffer on Cmd::Start to recover the phrase onset.
    let pre_roll_capacity: usize = (target_rate as usize) * PRE_ROLL_MS / 1000;
    let mut pre_roll: VecDeque<f32> = VecDeque::with_capacity(pre_roll_capacity);

    loop {
        // Wait for audio samples with timeout — allows Shutdown to be processed
        // even if the device stops producing audio (e.g. Bluetooth disconnect).
        let raw = match sample_rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(s) => {
                last_audio = std::time::Instant::now();
                ever_received = true;
                Some(s)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // stream closed
        };

        // Process audio only if recording and we have data
        if let Some(ref captured) = raw {
            converted.clear();
            resampler.push(captured, &mut converted);
            let samples = &converted[..];
            if recording {
                if !first_callback_logged {
                    crate::dev_log!(
                        "[AUDIO/WORKER] First audio chunk while recording: {} samples ({} captured)",
                        samples.len(),
                        captured.len()
                    );
                    first_callback_logged = true;
                }
                process_audio(samples, &vad, &mut frame_accumulator, &mut audio_buffer);
            } else if pre_roll_capacity > 0 {
                // Not recording: keep the last PRE_ROLL_MS of audio as pre-roll.
                for &s in samples.iter() {
                    if pre_roll.len() == pre_roll_capacity {
                        pre_roll.pop_front();
                    }
                    pre_roll.push_back(s);
                }
            }
        }

        // Non-blocking check for commands (processed even on timeout — no audio needed)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::Start => {
                    audio_buffer.clear();
                    frame_accumulator.clear();
                    recording = true;
                    first_callback_logged = false;

                    // Deliberately NOT resetting the resampler here, unlike Handy, whose
                    // resampler is per-session. Ours runs continuously: the stream stays
                    // open between dictations to keep push-to-talk instant, so at this
                    // moment the resampler's state *is* the tail of the pre-roll we are
                    // about to prepend. Resetting would punch a ~21 ms hole exactly at the
                    // seam between the pre-roll and the new audio. The state is cleared on
                    // Stop instead, which is where a real session boundary exists.

                    // Reset VAD state for new session
                    if let Ok(mut guard) = vad.lock() {
                        if let Some(ref mut v) = *guard {
                            v.reset();
                        }
                    }

                    // Prepend pre-roll: raw samples captured just before PTT press.
                    // Bypasses VAD — this audio is already known to precede speech onset
                    // and running it through VAD would risk discarding it as silence.
                    let pre_roll_len = pre_roll.len();
                    if pre_roll_len > 0 {
                        audio_buffer.reserve(pre_roll_len);
                        audio_buffer.extend(pre_roll.drain(..));
                        crate::dev_log!(
                            "[AUDIO/WORKER] Recording started (pre-roll {} samples / {}ms prepended)",
                            pre_roll_len,
                            pre_roll_len * 1000 / target_rate as usize
                        );
                    } else {
                        crate::dev_log!("[AUDIO/WORKER] Recording started (no pre-roll available)");
                    }
                }
                Cmd::Stop(reply_tx) => {
                    // Note: recording=false is set before the wait intentionally.
                    // New Cmd::Start during the wait is deferred to the next loop iteration.
                    recording = false;

                    // First-PTT race fix: if buffer is empty, wait for audio to arrive.
                    // On first PTT, ALSA can take 2s+ to open. Both Start and Stop
                    // queue before the stream produces any samples, resulting in 0 audio.
                    // Wait up to 500ms for the first chunk — instant on subsequent PTTs.
                    if audio_buffer.is_empty() {
                        crate::dev_log!(
                            "[AUDIO/WORKER] Buffer empty on Stop — waiting up to 500ms for audio..."
                        );
                        let deadline =
                            std::time::Instant::now() + std::time::Duration::from_millis(500);
                        let mut data_received = false;
                        while std::time::Instant::now() < deadline {
                            match sample_rx.recv_timeout(std::time::Duration::from_millis(10)) {
                                Ok(captured) => {
                                    converted.clear();
                                    resampler.push(&captured, &mut converted);
                                    process_audio(
                                        &converted,
                                        &vad,
                                        &mut frame_accumulator,
                                        &mut audio_buffer,
                                    );
                                    // A buffer arriving no longer implies usable output:
                                    // the resampler needs a full FFT block before it emits
                                    // anything, and the first pass after a reset always
                                    // emits zero. Breaking on "a buffer arrived" would end
                                    // this wait having captured nothing at all.
                                    if !converted.is_empty() {
                                        data_received = true;
                                        break;
                                    }
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        if data_received {
                            crate::dev_log!(
                                "[AUDIO/WORKER] Audio arrived, buffer: {} samples",
                                audio_buffer.len()
                            );
                        }
                    }

                    // Drain any audio chunks captured but not yet consumed
                    while let Ok(remaining) = sample_rx.try_recv() {
                        converted.clear();
                        resampler.push(&remaining, &mut converted);
                        process_audio(&converted, &vad, &mut frame_accumulator, &mut audio_buffer);
                    }

                    // Drain the resampler itself: tens of milliseconds of real audio are
                    // still inside it, and that is the end of the last word. Goes through
                    // process_audio like everything else so the sample order relative to
                    // the VAD accumulator stays right.
                    converted.clear();
                    resampler.finish(&mut converted);
                    if !converted.is_empty() {
                        process_audio(&converted, &vad, &mut frame_accumulator, &mut audio_buffer);
                    }
                    // The flush fed silence through to displace that audio; drop that
                    // state so it does not colour the pre-roll captured from here on.
                    // Costs a short window where the pre-roll ring stays empty while the
                    // resampler refills — tens of milliseconds, against a 500 ms ring.
                    resampler.reset();

                    // Flush VAD accumulator — process any remaining partial frame
                    flush_vad_accumulator(&vad, &mut frame_accumulator, &mut audio_buffer);

                    let samples = std::mem::take(&mut audio_buffer);
                    crate::dev_log!(
                        "[AUDIO/WORKER] Stop: returning {} samples ({:.2}s)",
                        samples.len(),
                        samples.len() as f32 / 16000.0
                    );
                    let _ = reply_tx.send(samples);
                }
                Cmd::Shutdown => {
                    crate::dev_log!("[AUDIO/WORKER] Shutdown command received");
                    return;
                }
            }
        }

        // Decided after the command drain, so a `Cmd::Start` queued during the last
        // wait is honoured rather than dropped with the channel. While recording, stay
        // up so `Cmd::Stop` can return what was captured; the exit follows right after.
        let timeout = if ever_received {
            STALLED_STREAM_TIMEOUT
        } else {
            STARTUP_STREAM_TIMEOUT
        };
        if !recording && last_audio.elapsed() >= timeout {
            crate::dev_log!(
                "[AUDIO/WORKER] No audio for {:?} while idle — releasing device",
                timeout
            );
            return;
        }
    }
}

/// Process audio samples through VAD (if enabled) into the buffer.
fn process_audio(
    samples: &[f32],
    vad: &Arc<Mutex<Option<SmoothedVad>>>,
    frame_acc: &mut Vec<f32>,
    audio_buffer: &mut Vec<f32>,
) {
    let Ok(mut vad_guard) = vad.lock() else {
        // If VAD lock fails, pass through raw audio
        audio_buffer.extend_from_slice(samples);
        return;
    };

    if let Some(ref mut vad_engine) = *vad_guard {
        // VAD enabled: accumulate into frames of SILERO_FRAME_SAMPLES
        frame_acc.extend_from_slice(samples);

        // Process complete frames in place over a cursor, then drain once. Avoids
        // a fresh Vec allocation + memmove per 480-sample frame (~33/s).
        let mut consumed = 0;
        while frame_acc.len() - consumed >= SILERO_FRAME_SAMPLES {
            let frame = &frame_acc[consumed..consumed + SILERO_FRAME_SAMPLES];
            match vad_engine.push_frame(frame) {
                Ok(VadFrame::Speech(speech_data)) => {
                    audio_buffer.extend_from_slice(speech_data);
                }
                Ok(VadFrame::Noise) => {} // discard silence
                Err(e) => {
                    // On VAD error, pass through raw audio
                    crate::dev_log!("[AUDIO] VAD error: {}, falling back to passthrough", e);
                    audio_buffer.extend_from_slice(frame);
                }
            }
            consumed += SILERO_FRAME_SAMPLES;
        }
        if consumed > 0 {
            frame_acc.drain(..consumed);
        }
    } else {
        // No VAD: passthrough
        audio_buffer.extend_from_slice(samples);
    }
}

/// Flush remaining samples in the VAD accumulator as raw audio.
fn flush_vad_accumulator(
    vad: &Arc<Mutex<Option<SmoothedVad>>>,
    frame_acc: &mut Vec<f32>,
    audio_buffer: &mut Vec<f32>,
) {
    if frame_acc.is_empty() {
        return;
    }

    let Ok(mut vad_guard) = vad.lock() else {
        audio_buffer.append(frame_acc);
        return;
    };

    if let Some(ref mut vad_engine) = *vad_guard {
        // Pad remaining samples to a full frame and process
        if !frame_acc.is_empty() {
            let mut final_frame = std::mem::take(frame_acc);
            final_frame.resize(SILERO_FRAME_SAMPLES, 0.0);
            match vad_engine.push_frame(&final_frame) {
                Ok(VadFrame::Speech(speech_data)) => {
                    audio_buffer.extend_from_slice(speech_data);
                }
                Ok(VadFrame::Noise) => {}
                Err(_) => {
                    audio_buffer.extend_from_slice(&final_frame);
                }
            }
        }
    } else {
        audio_buffer.append(frame_acc);
    }
}

// ── Device resolution (extracted for worker thread) ──────────────────────────

fn resolve_device(device_name: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    match device_name {
        None | Some("pipewire") | Some("default") | Some("") => {
            crate::dev_log!("[AUDIO/WORKER] Using default input device (fast path)");
            host.default_input_device()
                .ok_or("No default input device available".to_string())
        }
        Some(name) if name.starts_with("plughw:") => {
            crate::dev_log!("[AUDIO/WORKER] Opening ALSA device: {}", name);
            let found = host
                .input_devices()
                .map_err(|e| format!("Failed to enumerate devices: {}", e))?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false));

            match found {
                Some(dev) => Ok(dev),
                None => Err(format!(
                    "ALSA device '{}' not found (not yet available?)",
                    name
                )),
            }
        }
        Some(name) => {
            crate::dev_log!(
                "[AUDIO/WORKER] PipeWire source: {}, setting as default...",
                name
            );

            if !is_valid_device_name(name) {
                return Err(format!("Invalid device name: '{}'", name));
            }

            // Bluetooth: switch card to headset profile (HFP) for mic access
            if name.starts_with("bluez_input.") {
                let mac = name
                    .strip_prefix("bluez_input.")
                    .and_then(|m| m.rsplitn(2, '.').last())
                    .unwrap_or("");
                let card_name = format!("bluez_card.{}", mac);
                crate::dev_log!(
                    "[AUDIO/BT] Switching {} to headset-head-unit for mic access",
                    card_name
                );
                match std::process::Command::new("pactl")
                    .args(["set-card-profile", &card_name, "headset-head-unit"])
                    .output()
                {
                    Ok(o) if !o.status.success() => crate::dev_log!(
                        "[AUDIO/BT] pactl set-card-profile failed: {}",
                        String::from_utf8_lossy(&o.stderr)
                    ),
                    Err(e) => crate::dev_log!("[AUDIO/BT] Failed to run pactl: {}", e),
                    _ => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            match std::process::Command::new("pactl")
                .args(["set-default-source", name])
                .output()
            {
                Ok(o) if !o.status.success() => crate::dev_log!(
                    "[AUDIO/WORKER] pactl set-default-source failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => crate::dev_log!("[AUDIO/WORKER] Failed to run pactl: {}", e),
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            crate::dev_log!("[AUDIO/WORKER] Opening CPAL default (routed to PipeWire source)");
            host.default_input_device()
                .ok_or("No default input device available".to_string())
        }
    }
}

// ── Config negotiation (extracted for worker thread) ─────────────────────────

fn negotiate_config(
    device: &cpal::Device,
    target_sample_rate: u32,
) -> Result<(cpal::StreamConfig, u32), String> {
    let desired = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(target_sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    if let Ok(supported_ranges) = device.supported_input_configs() {
        for range in supported_ranges {
            if range.channels() == 1
                && range.min_sample_rate().0 <= target_sample_rate
                && range.max_sample_rate().0 >= target_sample_rate
            {
                return Ok((desired, target_sample_rate));
            }
        }
    }

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
        "[AUDIO/WORKER] Device doesn't support {}Hz, using {}Hz with resampling",
        target_sample_rate,
        native_rate
    );

    Ok((config, native_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A bare `run_worker` on its own thread, fed by hand.
    struct WorkerUnderTest {
        /// Stand-in for cpal's data callback. Must stay alive even when unused:
        /// dropping it disconnects the channel and the worker exits at once.
        sample_tx: mpsc::SyncSender<Vec<f32>>,
        cmd_tx: mpsc::Sender<Cmd>,
        handle: std::thread::JoinHandle<()>,
    }

    impl WorkerUnderTest {
        fn spawn() -> Self {
            let (sample_tx, sample_rx) = mpsc::sync_channel::<Vec<f32>>(SAMPLE_QUEUE_BUFFERS);
            let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
            let vad = Arc::new(Mutex::new(None));
            let handle = std::thread::spawn(move || {
                run_worker(vad, sample_rx, cmd_rx, 16_000, 16_000);
            });
            Self {
                sample_tx,
                cmd_tx,
                handle,
            }
        }

        fn feed_silence(&self) {
            self.sample_tx.send(vec![0.0; 160]).expect("worker alive");
        }

        fn exits_within(&self, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if self.handle.is_finished() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        }

        fn stop(&self) -> Vec<f32> {
            let (reply_tx, reply_rx) = mpsc::channel();
            self.cmd_tx.send(Cmd::Stop(reply_tx)).expect("send Stop");
            reply_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("Stop must still answer")
        }
    }

    const SLACK: Duration = Duration::from_secs(2);

    #[test]
    fn stream_error_hides_an_open_stream() {
        let capture = AudioCapture::new(16_000);
        assert!(!capture.is_stream_open(), "never opened");
        capture.is_open.store(true, Ordering::SeqCst);
        assert!(capture.is_stream_open());
        capture.stream_error.store(true, Ordering::SeqCst);
        assert!(
            !capture.is_stream_open(),
            "a failed stream must not look open, or the next PTT records silence"
        );
    }

    #[test]
    fn close_stream_clears_stream_error() {
        let mut capture = AudioCapture::new(16_000);
        capture.stream_error.store(true, Ordering::SeqCst);
        capture.close_stream(); // no worker: only the state reset runs
        assert!(!capture.stream_error.load(Ordering::SeqCst));
    }

    #[test]
    fn idle_worker_releases_a_stalled_stream() {
        let worker = WorkerUnderTest::spawn();
        worker.feed_silence();
        assert!(
            !worker.exits_within(STALLED_STREAM_TIMEOUT / 2),
            "a short gap must not drop the stream"
        );
        assert!(
            worker.exits_within(STALLED_STREAM_TIMEOUT / 2 + SLACK),
            "worker must exit once no audio arrived for {:?}",
            STALLED_STREAM_TIMEOUT
        );
        worker.handle.join().expect("worker panicked");
    }

    #[test]
    fn recording_worker_stays_up_while_stalled() {
        let worker = WorkerUnderTest::spawn();
        worker.feed_silence();
        worker.cmd_tx.send(Cmd::Start).expect("send Start");
        assert!(
            !worker.exits_within(STALLED_STREAM_TIMEOUT + Duration::from_millis(600)),
            "a recording worker must wait for Cmd::Stop, not drop the captured audio"
        );
        let samples = worker.stop();
        assert_eq!(
            samples.len(),
            160,
            "Stop returns what was fed before the stall"
        );
        assert!(
            worker.exits_within(SLACK),
            "once idle again the worker must release the dead stream"
        );
        worker.handle.join().expect("worker panicked");
    }

    #[test]
    fn start_queued_during_the_last_wait_is_honoured() {
        let worker = WorkerUnderTest::spawn();
        worker.feed_silence();
        // Land the Start inside the final 500 ms wait before the timeout expires.
        std::thread::sleep(STALLED_STREAM_TIMEOUT - Duration::from_millis(300));
        worker.cmd_tx.send(Cmd::Start).expect("send Start");
        assert!(
            !worker.exits_within(Duration::from_millis(800)),
            "a Start queued during the release tick must start a recording, not be lost"
        );
        let _ = worker.stop();
        worker.handle.join().expect("worker panicked");
    }
}
