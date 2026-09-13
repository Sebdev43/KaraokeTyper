//! From a shortcut edge to a recording session: the shared half of both listeners.
//!
//! The platform input layer (evdev on Linux, rdev elsewhere) turns key events into
//! [`KeyEvent`]s and hands them to [`feed_key`] on its own thread, so every edge is
//! timestamped as it happened. The [`Effect`]s the machine decides go through one
//! sequential executor ([`spawn_effect_executor`]): a stop pipeline (transcribe, paste)
//! always runs to completion before the next session starts, and the machine is told
//! when it drained — which is what lets a press made meanwhile start that session.

use crate::shortcut_activation::{ActivationRules, Effect, KeyEvent, SharedMachine};
use crate::AppState;
use futures::FutureExt;
use std::panic::AssertUnwindSafe;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

/// Where the executor's effects are queued.
pub(crate) type EffectQueue = mpsc::UnboundedSender<Effect>;

/// The activation settings, read for every event so a change applies at the next key.
fn activation_rules(app_handle: &AppHandle) -> ActivationRules {
    let state = app_handle.state::<AppState>();
    let config = state.config.lock().unwrap_or_else(|e| e.into_inner());
    ActivationRules::from(&*config)
}

/// Hand one shortcut edge to the machine, right now, and queue what it decides.
/// Called from the input thread: the timestamp is the edge's own.
pub(crate) fn feed_key(
    app_handle: &AppHandle,
    machine: &SharedMachine,
    effects: &EffectQueue,
    event: KeyEvent,
) {
    let rules = activation_rules(app_handle);
    machine.on_key_into(event, rules, effects);
}

/// Run the machine's effects one after the other, each shielded from panics so the
/// shortcut cannot die with one session. Returns the queue to feed effects into.
pub(crate) fn spawn_effect_executor(app_handle: AppHandle, machine: SharedMachine) -> EffectQueue {
    let (tx, mut rx) = mpsc::unbounded_channel::<Effect>();
    let requeue = tx.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(effect) = rx.recv().await {
            let ran = AssertUnwindSafe(run_effect(&app_handle, &machine, effect))
                .catch_unwind()
                .await;
            if ran.is_err() {
                crate::dev_log!("[SHORTCUT] {:?} panicked — session abandoned", effect);
                machine.on_start_failed();
                crate::hide_overlay_animated(&app_handle);
            }
            // The pipeline drained: a press remembered meanwhile starts the next session,
            // through the same guarded path as any other effect.
            if effect == Effect::StopRecording {
                if let Some(next) = machine.on_processing_finished() {
                    let _ = requeue.send(next);
                }
            }
        }
    });
    tx
}

async fn run_effect(app_handle: &AppHandle, machine: &SharedMachine, effect: Effect) {
    match effect {
        Effect::StartRecording => begin_recording(app_handle, machine).await,
        Effect::StopRecording => run_stop_pipeline(app_handle).await,
    }
}

async fn begin_recording(app_handle: &AppHandle, machine: &SharedMachine) {
    let state = app_handle.state::<AppState>();
    let t0 = state.mark_session_start();
    crate::dev_log!("[TIMING] ========== RECORDING SESSION START ==========");

    // Show overlay centered on cursor's monitor
    crate::show_overlay_with_state(app_handle, "recording");
    crate::dev_log!("[TIMING] T+{}ms: overlay shown", t0.elapsed().as_millis());

    match crate::do_start_recording(app_handle.clone(), &state).await {
        Ok(_) => crate::dev_log!(
            "[TIMING] T+{}ms: do_start_recording() returned OK",
            t0.elapsed().as_millis()
        ),
        Err(e) => {
            crate::dev_log!(
                "[TIMING] T+{}ms: do_start_recording() FAILED: {}",
                t0.elapsed().as_millis(),
                e
            );
            machine.on_start_failed();
            crate::hide_overlay_animated(app_handle);
        }
    }
}

/// Stop the capture, transcribe, paste. Overlay handling included.
async fn run_stop_pipeline(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();
    let t0 = state.session_start();
    crate::dev_log!(
        "[TIMING] T+{}ms: recording STOP — calling do_stop_recording()",
        t0.elapsed().as_millis()
    );
    crate::show_overlay_with_state(app_handle, "processing");

    let transcription = match crate::do_stop_recording(app_handle.clone(), &state).await {
        Ok(text) if !text.is_empty() => text,
        Ok(_) => {
            crate::dev_log!(
                "[TIMING] T+{}ms: no speech detected",
                t0.elapsed().as_millis()
            );
            crate::hide_overlay_animated(app_handle);
            return;
        }
        Err(e) => {
            crate::dev_log!(
                "[TIMING] T+{}ms: stop_recording FAILED: {}",
                t0.elapsed().as_millis(),
                e
            );
            crate::hide_overlay_animated(app_handle);
            return;
        }
    };
    crate::dev_log!(
        "[TIMING] T+{}ms: transcription complete: '{}'",
        t0.elapsed().as_millis(),
        transcription
    );

    // Linux hides the overlay before pasting: RustDesk and other remote desktops need
    // unambiguous focus. Elsewhere the target keeps focus while the overlay is up, so
    // pasting first loses no characters (as Handy does).
    let hide_before_paste = cfg!(target_os = "linux");
    if hide_before_paste {
        crate::hide_overlay_now(app_handle);
    }

    // Wait 300ms for physical hotkey keys (Ctrl+Space) to fully release before sending
    // the paste keystroke. Prevents modifier interference: without this, xdotool
    // --clearmodifiers sees Ctrl as held → first char typed as Ctrl+L (eaten by terminal).
    tokio::time::sleep(Duration::from_millis(300)).await;

    crate::dev_log!("[TIMING] T+{}ms: pasting text", t0.elapsed().as_millis());
    let pasted = crate::do_type_text(transcription, app_handle, state.paste_settings()).await;
    if !hide_before_paste {
        crate::hide_overlay_animated(app_handle);
    }
    match pasted {
        Ok(()) => crate::dev_log!(
            "[TIMING] T+{}ms: ========== SESSION COMPLETE ==========",
            t0.elapsed().as_millis()
        ),
        Err(e) => crate::dev_log!(
            "[TIMING] T+{}ms: paste FAILED: {}",
            t0.elapsed().as_millis(),
            e
        ),
    }
}
