//! Audio capture module - CPAL (cross-platform)
//!
//! Uses CPAL for audio capture on all platforms:
//! - Linux: ALSA backend (transparent PipeWire support via pipewire-alsa)
//! - Windows: WASAPI backend
//! - macOS: CoreAudio backend
//!
//! Architecture (worker thread — inspired by Handy):
//! - `open()` spawns a worker thread that builds the CPAL stream and starts it playing.
//!   Audio samples flow continuously through a wait-free SPSC ring (`rtrb`) into the
//!   worker; commands go through an mpsc channel.
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
use rtrb::{Consumer, RingBuffer};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

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
const STALLED_STREAM_TIMEOUT: Duration = Duration::from_secs(3);

/// Longest a dictation can plausibly hold the capture. Past this, a claim left
/// behind by a caller that never stopped is taken over rather than blocking
/// every later dictation.
const MAX_DICTATION: Duration = Duration::from_secs(600);

/// Same, before the first sample ever arrived: some devices (Bluetooth HFP) take a few
/// seconds to start delivering, and giving up early would waste the warm-up open.
const STARTUP_STREAM_TIMEOUT: Duration = Duration::from_secs(10);

/// Delay before auto-closing the stream after recording stops (releases mic device)
const STREAM_AUTO_CLOSE_SECS: u64 = 10;

/// Seconds of device-rate audio the ring between the cpal callback and the worker can
/// hold before the callback starts dropping samples: allocated once, under a megabyte
/// at 48 kHz, and more than any worker hiccup should need.
///
/// Dropping is the right failure: the alternative in a real-time callback is blocking,
/// which does not make the audio arrive, it makes the *device* glitch as well.
const AUDIO_RING_SECONDS: usize = 4;

/// Device-rate samples the ring holds.
fn ring_capacity(sample_rate: u32) -> usize {
    sample_rate as usize * AUDIO_RING_SECONDS
}

/// Whether a dictation about to start needs a fresh worker.
///
/// Callers must hold a session claim before asking: reopening tears the current
/// worker down, and doing that under a live dictation throws its buffer away.
/// A device that changed while a dictation was running is picked up here, which
/// is why the settings window is allowed to defer the switch instead of forcing it.
#[must_use]
pub fn needs_reopen(
    worker_alive: bool,
    stream_failed: bool,
    opened_device: Option<&str>,
    wanted_device: Option<&str>,
) -> bool {
    !worker_alive || stream_failed || opened_device != wanted_device
}

/// How long the worker waits for a command before draining the ring again — the upper
/// bound on how long fresh audio waits before the worker sees it.
const RING_POLL_INTERVAL: Duration = Duration::from_millis(10);

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
    /// since. Lets `stream_failed()` report the loss at once, before the worker
    /// notices the gap itself.
    stream_error: Arc<AtomicBool>,
    /// When a dictation was claimed, `None` when none is in flight. Without the
    /// claim, a second start walks the reopen path in `do_start_recording` and
    /// `close_stream()` kills the worker mid-dictation, buffer and all — the
    /// recording silently loses everything said so far. Handy guards the same
    /// way, one layer up: any start outside `RecordingState::Idle` is refused.
    ///
    /// The timestamp is what keeps a caller that dies before stopping (a reloaded
    /// webview, a panic between start and stop) from refusing every later
    /// dictation: past `MAX_DICTATION`, the next start takes the claim over.
    session_claimed_at: Mutex<Option<Instant>>,
    /// The device the live worker was opened on, so a device change that had to
    /// wait for a dictation to end is picked up at the next one.
    opened_device: Mutex<Option<String>>,
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
            session_claimed_at: Mutex::new(None),
            opened_device: Mutex::new(None),
            is_bluetooth: AtomicBool::new(false),
            stop_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Set the Voice Activity Detection engine.
    pub fn set_vad(&self, vad: SmoothedVad) {
        *self.vad.lock().unwrap() = Some(vad);
    }

    /// Whether a worker thread exists and is still running.
    ///
    /// This is the honest test for "someone may be capturing", and `is_open` is
    /// not: the worker only raises `is_open` once `stream.play()` returns, after
    /// device enumeration (1-2 s on ALSA). For that whole window a worker exists
    /// with a `Cmd::Start` already queued while `is_open` is still false.
    #[must_use]
    pub fn worker_alive(&self) -> bool {
        self.worker_handle
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    /// Claim the capture for one dictation, before anything touches the stream.
    ///
    /// `Err` when a dictation is already in flight: the caller must then leave the
    /// stream alone rather than reopen it under a live capture. A claim is taken
    /// over when no worker can be holding it any more, or when it is older than
    /// any plausible dictation — a caller that died before stopping must not
    /// refuse recording for the rest of the session.
    pub fn try_begin_session(&mut self) -> Result<(), String> {
        let mut claimed = self
            .session_claimed_at
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *claimed {
            Some(_) if !self.worker_alive() => {
                crate::dev_log!("[AUDIO] Stale session claim taken over: no worker is running");
            }
            Some(since) if since.elapsed() > MAX_DICTATION => {
                crate::dev_log!(
                    "[AUDIO] Stale session claim taken over: {}s old, past the {}s ceiling",
                    since.elapsed().as_secs(),
                    MAX_DICTATION.as_secs()
                );
            }
            Some(_) => return Err("a recording is already in flight".to_string()),
            None => {}
        }
        *claimed = Some(Instant::now());
        Ok(())
    }

    /// Whether a dictation currently holds the capture.
    #[must_use]
    pub fn session_in_flight(&self) -> bool {
        self.session_claimed_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// The device the current worker was opened on.
    #[must_use]
    pub fn opened_device(&self) -> Option<String> {
        self.opened_device
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Release the claim taken by `try_begin_session`. Idempotent.
    pub fn end_session(&self) {
        *self
            .session_claimed_at
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Whether cpal reported an error and no audio has arrived since.
    ///
    /// Deliberately not "is the stream open": a worker that has not reached
    /// `stream.play()` yet is still the owner of the queued dictation, and tearing
    /// it down to open another one costs the first seconds of speech. Only a
    /// reported failure justifies a respawn before the worker gives up on its own.
    #[must_use]
    pub fn stream_failed(&self) -> bool {
        self.stream_error.load(Ordering::SeqCst)
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
        *self.opened_device.lock().unwrap_or_else(|e| e.into_inner()) =
            device_name.map(str::to_owned);

        // Track if this is a Bluetooth device (for auto-close logic)
        let is_bt = device_name
            .map(|n| n.starts_with("bluez_input."))
            .unwrap_or(false);
        self.is_bluetooth.store(is_bt, Ordering::Relaxed);

        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();

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

            // Rate conversion happens on the worker, where it can take as long as it
            // needs and keep state across callbacks. The callback only copies samples
            // into a wait-free ring and bumps a counter: no allocation, no lock, no
            // syscall, no log on the audio thread — the worker reports the overruns.
            let (mut producer, consumer) = RingBuffer::<f32>::new(ring_capacity(actual_rate));
            let overrun = Arc::new(AtomicU64::new(0));
            let overrun_data = Arc::clone(&overrun);
            let stream_error_data = Arc::clone(&stream_error);
            let stream = match device.build_input_stream(
                &config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Audio is flowing, so whatever cpal reported was transient (ALSA
                    // reports an overrun it hits mid-read, then recovers by itself).
                    stream_error_data.store(false, Ordering::SeqCst);
                    // Never block here: waiting for a slow worker would add a device-level
                    // glitch on top of the backlog it was waiting on. Whatever does not
                    // fit is dropped and counted.
                    let (_, dropped) = producer.push_partial_slice(data);
                    if !dropped.is_empty() {
                        overrun_data.fetch_add(dropped.len() as u64, Ordering::Relaxed);
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
            run_worker(vad, consumer, overrun, cmd_rx, actual_rate, target_rate);

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
        // The worker that owned the dictation is gone with its buffer.
        self.end_session();
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
        // Released first: the dictation is over whatever happens below, and a
        // claim kept after a failed send would refuse every later start.
        self.end_session();
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
                std::thread::sleep(Duration::from_secs(STREAM_AUTO_CLOSE_SECS));
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
            .recv_timeout(Duration::from_secs(5))
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

/// Pull everything the callback has written through the resampler into `converted`
/// (cleared first). Returns the device-rate sample count taken from the ring.
///
/// The ring's two halves go straight into the resampler: it is streaming, so
/// `push(a); push(b)` is `push(a ++ b)` without copying 4 s of audio first.
fn drain_ring(
    consumer: &mut Consumer<f32>,
    resampler: &mut AudioResampler,
    converted: &mut Vec<f32>,
) -> usize {
    converted.clear();
    let available = consumer.slots();
    let Ok(chunk) = consumer.read_chunk(available) else {
        return 0;
    };
    let (first, second) = chunk.as_slices();
    resampler.push(first, converted);
    resampler.push(second, converted);
    chunk.commit_all();
    available
}

fn run_worker(
    vad: Arc<Mutex<Option<SmoothedVad>>>,
    mut consumer: Consumer<f32>,
    overrun: Arc<AtomicU64>,
    cmd_rx: mpsc::Receiver<Cmd>,
    actual_rate: u32,
    target_rate: u32,
) {
    let mut audio_buffer: Vec<f32> = Vec::new();
    let mut frame_accumulator: Vec<f32> = Vec::new();
    let mut recording = false;
    // Whether the device has delivered anything since this dictation started. The
    // pre-roll is prepended at `Cmd::Start`, so "buffer is empty" stopped meaning
    // "nothing was captured" long ago: on a cold stream a Stop would then return
    // the pre-roll alone — audio from *before* the press — and call it a dictation.
    let mut captured_this_session = false;
    let mut first_callback_logged = false;
    let mut last_audio = Instant::now();
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
        // Take whatever the callback has written since the last pass.
        let taken = drain_ring(&mut consumer, &mut resampler, &mut converted);
        // The callback only counts; a stalled worker sees the whole burst here, once.
        let lost = overrun.swap(0, Ordering::Relaxed);
        if lost > 0 {
            crate::dev_log!(
                "[AUDIO/WORKER] Ring overran: {} samples dropped while the worker was behind",
                lost
            );
        }
        if taken > 0 {
            last_audio = Instant::now();
            ever_received = true;
            let samples = &converted[..];
            if recording {
                // Usable output, not merely "the ring had bytes": the resampler
                // needs a full FFT block before it emits anything, and a Stop in
                // that window must still wait for the device (see `Cmd::Stop`).
                captured_this_session |= !samples.is_empty();
                if !first_callback_logged {
                    crate::dev_log!(
                        "[AUDIO/WORKER] First audio chunk while recording: {} samples ({} captured)",
                        samples.len(),
                        taken
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

        // Commands. The wait doubles as the ring poll interval: a quiet device still
        // lets Shutdown through, and a command never waits for audio to arrive.
        let first = match cmd_rx.recv_timeout(RING_POLL_INTERVAL) {
            Ok(cmd) => Some(cmd),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return, // capture dropped
        };
        for cmd in first.into_iter().chain(cmd_rx.try_iter()) {
            match cmd {
                Cmd::Start => {
                    audio_buffer.clear();
                    frame_accumulator.clear();
                    recording = true;
                    captured_this_session = false;
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
                    let was_recording = std::mem::replace(&mut recording, false);

                    // First-PTT race fix: if buffer is empty, wait for audio to arrive.
                    // On first PTT, ALSA can take 2s+ to open. Both Start and Stop
                    // queue before the stream produces any samples, resulting in 0 audio.
                    // Wait up to 500ms for the first chunk — instant on subsequent PTTs.
                    // An unpaired Stop (a second one, or one for a session another
                    // path already ended) must not spend the grace window pulling
                    // idle-time audio into a buffer and returning it as a dictation.
                    if was_recording && !captured_this_session {
                        crate::dev_log!(
                            "[AUDIO/WORKER] Nothing captured yet on Stop — waiting up to 500ms for audio..."
                        );
                        let deadline = Instant::now() + Duration::from_millis(500);
                        let mut data_received = false;
                        while Instant::now() < deadline {
                            if drain_ring(&mut consumer, &mut resampler, &mut converted) == 0 {
                                std::thread::sleep(RING_POLL_INTERVAL);
                                continue;
                            }
                            process_audio(
                                &converted,
                                &vad,
                                &mut frame_accumulator,
                                &mut audio_buffer,
                            );
                            // Audio arriving no longer implies usable output: the resampler
                            // needs a full FFT block before it emits anything, and the
                            // first pass after a reset always emits zero. Breaking on
                            // "audio arrived" would end this wait having captured nothing.
                            if !converted.is_empty() {
                                data_received = true;
                                captured_this_session = true;
                                break;
                            }
                        }
                        if data_received {
                            crate::dev_log!(
                                "[AUDIO/WORKER] Audio arrived, buffer: {} samples",
                                audio_buffer.len()
                            );
                        }
                    }

                    // Drain whatever the callback wrote since the last pass
                    drain_ring(&mut consumer, &mut resampler, &mut converted);
                    process_audio(&converted, &vad, &mut frame_accumulator, &mut audio_buffer);

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
                std::thread::sleep(Duration::from_millis(500));
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
            std::thread::sleep(Duration::from_millis(100));
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

    /// A bare `run_worker` on its own thread, fed by hand through the same ring the
    /// cpal callback would write to.
    struct WorkerUnderTest {
        producer: rtrb::Producer<f32>,
        cmd_tx: mpsc::Sender<Cmd>,
        handle: std::thread::JoinHandle<()>,
    }

    impl WorkerUnderTest {
        fn spawn() -> Self {
            let (producer, consumer) = RingBuffer::<f32>::new(ring_capacity(16_000));
            let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
            let vad = Arc::new(Mutex::new(None));
            let overrun = Arc::new(AtomicU64::new(0));
            let handle = std::thread::spawn(move || {
                run_worker(vad, consumer, overrun, cmd_rx, 16_000, 16_000);
            });
            Self {
                producer,
                cmd_tx,
                handle,
            }
        }

        fn feed_silence(&mut self) {
            self.feed(&[0.0; 160]);
        }

        fn feed(&mut self, samples: &[f32]) {
            let (_, rest) = self.producer.push_partial_slice(samples);
            assert!(
                rest.is_empty(),
                "test ring must take the whole feed — worker not draining?"
            );
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

    /// A stand-in for a worker thread that is alive until the test lets it go.
    fn live_worker() -> (std::thread::JoinHandle<()>, mpsc::Sender<()>) {
        let (tx, rx) = mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let _ = rx.recv();
        });
        (handle, tx)
    }

    #[test]
    fn reopen_is_decided_on_the_worker_and_the_device_it_holds() {
        // Nothing running: open.
        assert!(needs_reopen(false, false, None, None));
        // Live worker on the wanted device, nothing reported: leave it alone. This
        // is the case the claim protects — reopening here kills the dictation.
        assert!(!needs_reopen(true, false, Some("mic-a"), Some("mic-a")));
        // Enumerating (worker alive, nothing reported) on the wanted device: the
        // Start is already queued on it, do not respawn.
        assert!(!needs_reopen(true, false, Some("mic-a"), Some("mic-a")));
        // A reported failure does justify a respawn.
        assert!(needs_reopen(true, true, Some("mic-a"), Some("mic-a")));
        // A device change the settings window deferred is picked up here.
        assert!(needs_reopen(true, false, Some("mic-a"), Some("mic-b")));
        assert!(needs_reopen(true, false, None, Some("mic-b")));
        // A worker that died must be replaced whatever the device says.
        assert!(needs_reopen(false, false, Some("mic-a"), Some("mic-a")));
    }

    #[test]
    fn a_second_session_is_refused_while_a_worker_holds_one() {
        let mut capture = AudioCapture::new(16_000);
        let (handle, release) = live_worker();
        capture.worker_handle = Some(handle);

        capture.try_begin_session().expect("first claim");
        assert!(
            capture.try_begin_session().is_err(),
            "a start under a live dictation must be refused, not reopen the stream \
             and take the worker (and its buffer) down with it"
        );

        capture.end_session();
        capture
            .try_begin_session()
            .expect("the next dictation must be allowed once the session ended");

        drop(release);
        capture.worker_handle.take().expect("handle").join().ok();
    }

    #[test]
    fn a_claim_survives_the_window_where_the_stream_is_not_open_yet() {
        let mut capture = AudioCapture::new(16_000);
        let (handle, release) = live_worker();
        capture.worker_handle = Some(handle);
        // The worker is enumerating the device: it exists and already holds the
        // queued Start, but has not reached `stream.play()` yet.
        assert!(
            !capture.is_open.load(Ordering::SeqCst),
            "precondition: the worker has not reached stream.play() yet"
        );
        assert!(!capture.stream_failed(), "precondition: nothing reported");

        capture.try_begin_session().expect("first claim");
        assert!(
            capture.try_begin_session().is_err(),
            "the claim must hold during device enumeration — that is the window \
             where a reopen would kill the worker that owns the dictation"
        );

        drop(release);
        capture.worker_handle.take().expect("handle").join().ok();
    }

    #[test]
    fn a_claim_left_behind_by_a_dead_caller_does_not_wedge_recording() {
        let mut capture = AudioCapture::new(16_000);
        capture.try_begin_session().expect("claim");
        // No worker can be holding it: the caller died before stopping.
        capture
            .try_begin_session()
            .expect("no worker means no session to protect");
    }

    #[test]
    fn stop_waits_for_the_device_even_when_a_pre_roll_filled_the_buffer() {
        let mut worker = WorkerUnderTest::spawn();
        // Pre-roll: captured before the press, so it lands in the buffer at Start.
        worker.feed(&[0.5; 480]);
        std::thread::sleep(Duration::from_millis(50));
        worker.cmd_tx.send(Cmd::Start).expect("send Start");
        std::thread::sleep(Duration::from_millis(50));

        // The device delivers the dictation itself only after the Stop is queued —
        // a cold stream. The buffer is not empty (pre-roll), which is exactly the
        // case the old `audio_buffer.is_empty()` grace refused to wait for.
        let (reply_tx, reply_rx) = mpsc::channel();
        worker.cmd_tx.send(Cmd::Stop(reply_tx)).expect("send Stop");
        std::thread::sleep(Duration::from_millis(150));
        worker.feed(&[0.25; 320]);

        let captured = reply_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Stop must answer");
        assert_eq!(
            captured.len(),
            800,
            "pre-roll (480) + the dictation that arrived during the grace window (320)"
        );
        assert!(
            captured[..480].iter().all(|s| *s == 0.5),
            "precondition: the pre-roll really was in the buffer at Stop, which is \
             what made the old `audio_buffer.is_empty()` grace refuse to wait"
        );
        assert!(
            captured[480..].iter().all(|s| *s == 0.25),
            "the dictation arrived during the grace window and must be in the result"
        );

        worker.cmd_tx.send(Cmd::Shutdown).expect("send Shutdown");
        worker.handle.join().expect("worker panicked");
    }

    #[test]
    fn a_reported_stream_error_is_visible_to_the_next_start() {
        let capture = AudioCapture::new(16_000);
        assert!(!capture.stream_failed(), "nothing has failed yet");
        capture.is_open.store(true, Ordering::SeqCst);
        assert!(
            !capture.stream_failed(),
            "an open stream with no error must not ask for a respawn"
        );
        capture.stream_error.store(true, Ordering::SeqCst);
        assert!(
            capture.stream_failed(),
            "a failed stream must be replaced, or the next PTT records silence"
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
        let mut worker = WorkerUnderTest::spawn();
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
        let mut worker = WorkerUnderTest::spawn();
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
    fn drain_ring_feeds_both_halves_in_order() {
        let (mut producer, mut consumer) = RingBuffer::<f32>::new(100);
        let mut resampler = AudioResampler::new(16_000, 16_000); // passthrough
        let mut converted = Vec::new();
        // Fill, drain, then write past the end so the next read wraps around.
        assert!(producer.push_partial_slice(&[0.0; 60]).1.is_empty());
        assert_eq!(
            drain_ring(&mut consumer, &mut resampler, &mut converted),
            60
        );
        let wrapped: Vec<f32> = (0..80).map(|i| i as f32).collect();
        assert!(producer.push_partial_slice(&wrapped).1.is_empty());
        assert_eq!(
            drain_ring(&mut consumer, &mut resampler, &mut converted),
            80
        );
        assert_eq!(
            converted, wrapped,
            "both ring halves, in order, nothing duplicated"
        );
        assert_eq!(
            drain_ring(&mut consumer, &mut resampler, &mut converted),
            0,
            "nothing left"
        );
        assert!(converted.is_empty(), "drain clears the output first");
    }

    #[test]
    fn shutdown_is_processed_without_audio() {
        let worker = WorkerUnderTest::spawn();
        worker.cmd_tx.send(Cmd::Shutdown).expect("send Shutdown");
        assert!(
            worker.exits_within(Duration::from_secs(1)),
            "Shutdown must not wait for a sample to arrive"
        );
        worker.handle.join().expect("worker panicked");
    }

    #[test]
    fn stop_returns_pre_roll_and_everything_written_before_it() {
        let mut worker = WorkerUnderTest::spawn();
        // Idle: more than the pre-roll ring holds, so only its tail can survive —
        // which is how this test tells the pre-roll path from a recording.
        let pre_roll_capacity = 16_000 * PRE_ROLL_MS / 1000;
        worker.feed(&vec![0.1; pre_roll_capacity + 2_000]);
        std::thread::sleep(RING_POLL_INTERVAL * 5);
        worker.cmd_tx.send(Cmd::Start).expect("send Start");
        std::thread::sleep(RING_POLL_INTERVAL * 5);
        worker.feed(&[0.2; 1_600]); // recording: captured
        let samples = worker.stop(); // drains the ring before answering
        assert_eq!(
            samples.len(),
            pre_roll_capacity + 1_600,
            "pre-roll tail + recording, nothing lost"
        );
        assert!(samples[..pre_roll_capacity].iter().all(|&s| s == 0.1));
        assert!(samples[pre_roll_capacity..].iter().all(|&s| s == 0.2));
        worker.cmd_tx.send(Cmd::Shutdown).expect("send Shutdown");
        worker.handle.join().expect("worker panicked");
    }

    #[test]
    fn start_queued_during_the_last_wait_is_honoured() {
        let mut worker = WorkerUnderTest::spawn();
        worker.feed_silence();
        // Land the Start shortly before the release check; it must be drained and
        // honoured, not dropped with the channel.
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
