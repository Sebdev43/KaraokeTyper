//! How the recording shortcut's presses and releases drive a recording.
//!
//! A pure state machine, ported from Handy (`transcription_coordinator.rs`, commits
//! c62a5fc and c6fa60d) without its multi-binding, release-grace and debounce layers —
//! KaraokeTyper has one shortcut and its listeners already deduplicate repeats. The
//! listeners feed timestamped key events in and execute the [`Effect`]s that come out;
//! nothing here touches audio, Tauri or a clock, so every transition is unit-testable.
//!
//! The three modes are documented on [`ShortcutActivation`]; a release before the
//! hold threshold is a tap, at or after it a hold.
//!
//! Between `StopRecording` and [`ShortcutMachine::on_processing_finished`] the pipeline
//! (transcription, paste) is busy and cannot start a session. A press landing in that
//! window is remembered — at most one — and starts the next recording when the pipeline
//! drains; it evolves under exactly the rules of a live recording, so a second press
//! forgets it the way a press ends a locked session (toggle parity, Handy c6fa60d) and
//! a hold released before the drain is dropped: the user let go, nothing is left to start.

use crate::config::{AppConfig, ShortcutActivation};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A shortcut edge as the listener saw it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Pressed,
    Released,
}

/// What the listener must do in response to an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    StartRecording,
    StopRecording,
}

/// The settings that decide transitions, read afresh for every event so a change in
/// the settings window applies at the next key without restarting anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationRules {
    pub mode: ShortcutActivation,
    pub hold_threshold: Duration,
}

impl From<&AppConfig> for ActivationRules {
    fn from(config: &AppConfig) -> Self {
        Self {
            mode: config.shortcut_activation,
            hold_threshold: config.hold_threshold(),
        }
    }
}

impl ActivationRules {
    /// A release this soon after its press is a tap. Only hold-or-toggle has one; in
    /// `Hold` any release stops.
    fn tap_window(self) -> Duration {
        match self.mode {
            ShortcutActivation::HoldOrToggle => self.hold_threshold,
            ShortcutActivation::Hold | ShortcutActivation::Toggle => Duration::ZERO,
        }
    }
}

/// One press of the shortcut and what it has become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Press {
    /// When the key physically went down. A press remembered while the pipeline was
    /// busy keeps it across the drain, so a hold straddling the drain is measured from
    /// the key-down, not from the moment recording actually began.
    at: Instant,
    /// The session ends on the next press rather than on a release: toggle mode, or a
    /// tap.
    locked: bool,
}

impl Press {
    fn new(at: Instant, rules: ActivationRules) -> Self {
        Self {
            at,
            locked: rules.mode == ShortcutActivation::Toggle,
        }
    }

    /// How this press reacts to the next edge. `None`: its session is over. The same
    /// rules apply whether it is recording or waiting for the pipeline to drain — that
    /// is what keeps toggle parity through a busy window.
    fn step(self, event: KeyEvent, now: Instant, rules: ActivationRules) -> Option<Press> {
        // In toggle mode every press ends the session, even one begun under a hold mode
        // (the setting changed meanwhile) — otherwise nothing could stop it.
        let ends_on_press = self.locked || rules.mode == ShortcutActivation::Toggle;
        match event {
            KeyEvent::Pressed if ends_on_press => None,
            // The key is still held (its release will end this); a repeat means nothing.
            KeyEvent::Pressed => Some(self),
            KeyEvent::Released if ends_on_press => Some(self),
            // A hold: the release ends it.
            KeyEvent::Released if now.saturating_duration_since(self.at) >= rules.tap_window() => {
                None
            }
            // A tap: keep going until the next press.
            KeyEvent::Released => Some(Press {
                locked: true,
                ..self
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Idle,
    Recording(Press),
    /// Between `StopRecording` and `on_processing_finished`; `pending` is the one press
    /// remembered meanwhile.
    Processing {
        pending: Option<Press>,
    },
}

#[derive(Debug, Default)]
pub struct ShortcutMachine {
    stage: Stage,
}

impl ShortcutMachine {
    fn is_recording(&self) -> bool {
        matches!(self.stage, Stage::Recording(_))
    }

    /// Feed one shortcut edge. `now` is when the listener saw it.
    #[must_use = "the listener must carry out the effect"]
    pub fn on_key(
        &mut self,
        event: KeyEvent,
        now: Instant,
        rules: ActivationRules,
    ) -> Option<Effect> {
        match (self.stage, event) {
            (Stage::Idle, KeyEvent::Pressed) => {
                self.stage = Stage::Recording(Press::new(now, rules));
                Some(Effect::StartRecording)
            }
            (Stage::Idle, KeyEvent::Released) => None,
            (Stage::Recording(press), _) => match press.step(event, now, rules) {
                Some(press) => {
                    self.stage = Stage::Recording(press);
                    None
                }
                None => {
                    self.stage = Stage::Processing { pending: None };
                    Some(Effect::StopRecording)
                }
            },
            (Stage::Processing { pending: None }, KeyEvent::Pressed) => {
                self.stage = Stage::Processing {
                    pending: Some(Press::new(now, rules)),
                };
                None
            }
            (Stage::Processing { pending: None }, KeyEvent::Released) => None,
            (
                Stage::Processing {
                    pending: Some(press),
                },
                _,
            ) => {
                self.stage = Stage::Processing {
                    pending: press.step(event, now, rules),
                };
                None
            }
        }
    }

    /// The pipeline started by the last `StopRecording` has drained (text pasted, or
    /// it failed). Starts the press remembered meanwhile, if any.
    #[must_use = "the listener must carry out the effect"]
    pub fn on_processing_finished(&mut self) -> Option<Effect> {
        let Stage::Processing { pending } = self.stage else {
            return None;
        };
        self.stage = pending.map_or(Stage::Idle, Stage::Recording);
        pending.map(|_| Effect::StartRecording)
    }

    /// `StartRecording` could not be carried out: roll the optimistic transition back.
    pub fn on_start_failed(&mut self) {
        if self.is_recording() {
            self.stage = Stage::Idle;
        }
    }
}

/// The machine as both listeners hold it: the input thread feeds it, the effect
/// executor tells it when the pipeline drained.
#[derive(Debug, Clone, Default)]
pub struct SharedMachine(Arc<Mutex<ShortcutMachine>>);

impl SharedMachine {
    fn lock(&self) -> std::sync::MutexGuard<'_, ShortcutMachine> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Decide on an edge seen right now and queue the effect before letting go of the
    /// lock: with one input thread per device, two edges decided back to back must
    /// reach the executor in the order the machine decided them.
    pub fn on_key_into(
        &self,
        event: KeyEvent,
        rules: ActivationRules,
        effects: &tokio::sync::mpsc::UnboundedSender<Effect>,
    ) {
        let mut machine = self.lock();
        let effect = machine.on_key(event, Instant::now(), rules);
        crate::dev_log!("[SHORTCUT] {:?} ({:?}) → {:?}", event, rules.mode, effect);
        if let Some(effect) = effect {
            if effects.send(effect).is_err() {
                crate::dev_log!("[SHORTCUT] Effect executor gone — {:?} dropped", effect);
            }
        }
    }

    #[must_use = "the listener must carry out the effect"]
    pub fn on_processing_finished(&self) -> Option<Effect> {
        self.lock().on_processing_finished()
    }

    pub fn on_start_failed(&self) {
        self.lock().on_start_failed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const THRESHOLD: Duration = Duration::from_millis(300);

    fn rules(mode: ShortcutActivation) -> ActivationRules {
        ActivationRules {
            mode,
            hold_threshold: THRESHOLD,
        }
    }

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn hold_press_starts_and_release_stops_however_short() {
        let r = rules(ShortcutActivation::Hold);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, r),
            Some(Effect::StartRecording)
        );
        assert!(m.is_recording());
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 50), r),
            None,
            "repeat while held"
        );
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 60), r),
            Some(Effect::StopRecording),
            "push-to-talk stops on release, even a 60 ms one"
        );
        assert!(!m.is_recording());
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 70), r),
            None,
            "stray release"
        );
    }

    #[test]
    fn toggle_press_starts_next_press_stops_and_releases_are_ignored() {
        let r = rules(ShortcutActivation::Toggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, r),
            Some(Effect::StartRecording)
        );
        assert_eq!(m.on_key(KeyEvent::Released, at(t0, 40), r), None);
        assert!(m.is_recording(), "still recording after the release");
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 5_000), r),
            Some(Effect::StopRecording)
        );
        assert_eq!(m.on_key(KeyEvent::Released, at(t0, 5_040), r), None);
    }

    #[test]
    fn hold_or_toggle_tap_locks_recording_until_the_next_press() {
        let r = rules(ShortcutActivation::HoldOrToggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, r),
            Some(Effect::StartRecording)
        );
        assert_eq!(m.on_key(KeyEvent::Released, at(t0, 120), r), None, "a tap");
        assert!(m.is_recording(), "locked on");
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 4_000), r),
            Some(Effect::StopRecording)
        );
        assert_eq!(m.on_key(KeyEvent::Released, at(t0, 4_100), r), None);
    }

    #[test]
    fn hold_or_toggle_hold_stops_on_release() {
        let r = rules(ShortcutActivation::HoldOrToggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, r),
            Some(Effect::StartRecording)
        );
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 300), r),
            Some(Effect::StopRecording),
            "held exactly the threshold: a hold"
        );
    }

    #[test]
    fn switching_to_toggle_mid_hold_still_lets_a_press_stop_it() {
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, rules(ShortcutActivation::Hold)),
            Some(Effect::StartRecording)
        );
        assert_eq!(
            m.on_key(
                KeyEvent::Pressed,
                at(t0, 1_000),
                rules(ShortcutActivation::Toggle)
            ),
            Some(Effect::StopRecording)
        );
    }

    #[test]
    fn press_while_processing_is_remembered_once_and_started_on_drain() {
        let r = rules(ShortcutActivation::Toggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, r),
            Some(Effect::StartRecording)
        );
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 2_000), r),
            Some(Effect::StopRecording)
        );
        // The pipeline is transcribing: the next press must not be lost…
        assert_eq!(m.on_key(KeyEvent::Pressed, at(t0, 2_500), r), None);
        assert_eq!(m.on_key(KeyEvent::Released, at(t0, 2_550), r), None);
        // …and starts the next session as soon as it drains.
        assert_eq!(m.on_processing_finished(), Some(Effect::StartRecording));
        assert!(m.is_recording());
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 6_000), r),
            Some(Effect::StopRecording),
            "the remembered toggle session is locked: one press ends it"
        );
    }

    #[test]
    fn second_press_while_processing_cancels_the_remembered_one() {
        let r = rules(ShortcutActivation::Toggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 2_000), r);
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 2_500), r),
            None,
            "remembered"
        );
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 2_800), r),
            None,
            "forgotten"
        );
        assert_eq!(
            m.on_processing_finished(),
            None,
            "parity: two presses net to nothing"
        );
        assert!(!m.is_recording());
    }

    #[test]
    fn hold_completed_while_processing_is_forgotten_but_a_tap_starts_locked() {
        let r = rules(ShortcutActivation::HoldOrToggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 1_000), r),
            Some(Effect::StopRecording)
        );
        // Held through the whole busy window and let go: nothing left to start.
        assert_eq!(m.on_key(KeyEvent::Pressed, at(t0, 1_200), r), None);
        assert_eq!(m.on_key(KeyEvent::Released, at(t0, 1_900), r), None);
        assert_eq!(m.on_processing_finished(), None);

        // A tap while busy starts a locked session on the drain.
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 3_000), r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 4_000), r);
        assert_eq!(m.on_key(KeyEvent::Pressed, at(t0, 4_100), r), None);
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 4_200), r),
            None,
            "a tap"
        );
        assert_eq!(m.on_processing_finished(), Some(Effect::StartRecording));
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 5_000), r),
            None,
            "locked"
        );
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 6_000), r),
            Some(Effect::StopRecording)
        );
    }

    #[test]
    fn two_taps_while_processing_net_to_nothing() {
        let r = rules(ShortcutActivation::HoldOrToggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 1_000), r);
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 1_100), r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 1_150), r); // tap: pending, locked
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 1_300), r); // ends the pending session
        assert_eq!(m.on_processing_finished(), None);
        assert!(!m.is_recording());
    }

    #[test]
    fn hold_tap_while_processing_is_dropped_too() {
        let r = rules(ShortcutActivation::Hold);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 1_000), r);
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 1_200), r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 1_250), r); // push-to-talk: any release ends it
        assert_eq!(m.on_processing_finished(), None);
    }

    #[test]
    fn hold_still_held_when_the_pipeline_drains_starts_and_stops_on_release() {
        let r = rules(ShortcutActivation::Hold);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 1_000), r);
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 1_200), r),
            None,
            "remembered"
        );
        assert_eq!(m.on_processing_finished(), Some(Effect::StartRecording));
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 2_000), r),
            Some(Effect::StopRecording)
        );
    }

    #[test]
    fn remembered_hold_is_measured_from_the_physical_press_across_the_drain() {
        let r = rules(ShortcutActivation::HoldOrToggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        let _ = m.on_key(KeyEvent::Released, at(t0, 500), r);
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 1_000), r); // remembered while busy
        assert_eq!(m.on_processing_finished(), Some(Effect::StartRecording)); // drain at 2.8 s
        assert_eq!(
            m.on_key(KeyEvent::Released, at(t0, 2_900), r),
            Some(Effect::StopRecording),
            "1.9 s down in total: a hold, not a 100 ms tap"
        );
    }

    #[test]
    fn start_failure_returns_to_idle_and_drain_when_idle_is_a_no_op() {
        let r = rules(ShortcutActivation::Hold);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        assert_eq!(
            m.on_key(KeyEvent::Pressed, t0, r),
            Some(Effect::StartRecording)
        );
        m.on_start_failed();
        assert!(!m.is_recording());
        assert_eq!(m.on_processing_finished(), None);
        assert_eq!(
            m.on_key(KeyEvent::Pressed, at(t0, 500), r),
            Some(Effect::StartRecording),
            "usable again"
        );
    }

    #[test]
    fn start_failure_while_processing_keeps_the_remembered_press() {
        let r = rules(ShortcutActivation::Toggle);
        let t0 = Instant::now();
        let mut m = ShortcutMachine::default();
        let _ = m.on_key(KeyEvent::Pressed, t0, r);
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 1_000), r);
        let _ = m.on_key(KeyEvent::Pressed, at(t0, 1_500), r); // remembered
        m.on_start_failed(); // a late failure report: nothing to roll back
        assert_eq!(m.on_processing_finished(), Some(Effect::StartRecording));
    }

    #[test]
    fn rules_from_config_clamp_the_threshold() {
        let rules_for = |hold_threshold_ms: u32| {
            ActivationRules::from(&AppConfig {
                shortcut_activation: ShortcutActivation::HoldOrToggle,
                hold_threshold_ms,
                ..AppConfig::default()
            })
        };
        assert_eq!(rules_for(5).hold_threshold, Duration::from_millis(100));
        assert_eq!(
            rules_for(99_999).hold_threshold,
            Duration::from_millis(1_000)
        );
        assert_eq!(rules_for(450).hold_threshold, Duration::from_millis(450));
    }
}
