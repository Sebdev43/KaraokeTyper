#![warn(dead_code, unused_imports)]

use super::types::EngineType;

/// Which of the two engine slots a recording is transcribed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttLane {
    /// The engine the user configured (`stt_backend` + `model`).
    Primary,
    /// Parakeet, kept loaded next to the primary engine so a short dictation
    /// skips the (much slower) Whisper pass.
    FastParakeet,
}

/// The hybrid-routing settings, snapshotted from the config when a recording stops.
///
/// The fast lane only exists when the primary engine is Whisper: `config.model` is a
/// single field, so with Parakeet as primary there is no Whisper model to hand long
/// dictations to — and every dictation already runs on Parakeet anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HybridPolicy {
    pub primary: EngineType,
    pub enabled: bool,
    /// Longest dictation (seconds) Parakeet handles; anything longer goes to Whisper.
    pub threshold_secs: u32,
}

impl HybridPolicy {
    /// True when this policy can ever route a dictation to Parakeet — so a zero
    /// threshold, which `lane_for` never satisfies, also keeps Parakeet unloaded.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.primary == EngineType::Whisper && self.threshold_secs > 0
    }

    /// Lane for a recording of `duration_secs` (raw capture, before padding).
    ///
    /// `parakeet_installed` is resolved by the caller so this stays a pure decision.
    /// A missing Parakeet silently falls back to the primary engine: the user asked
    /// for the fast lane "if it is installed", not for an error when it is not.
    #[must_use]
    pub fn lane_for(&self, duration_secs: f32, parakeet_installed: bool) -> SttLane {
        if self.is_active() && parakeet_installed && duration_secs <= self.threshold_secs as f32 {
            SttLane::FastParakeet
        } else {
            SttLane::Primary
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The common case: Whisper primary, hybrid mode on.
    fn whisper_hybrid(threshold_secs: u32) -> HybridPolicy {
        HybridPolicy {
            primary: EngineType::Whisper,
            enabled: true,
            threshold_secs,
        }
    }

    #[test]
    fn disabled_routes_to_primary() {
        let p = HybridPolicy {
            enabled: false,
            ..whisper_hybrid(20)
        };
        assert!(!p.is_active());
        assert_eq!(p.lane_for(2.0, true), SttLane::Primary);
    }

    #[test]
    fn primary_not_whisper_is_inactive() {
        for primary in [
            EngineType::Parakeet,
            EngineType::SenseVoice,
            EngineType::Moonshine,
        ] {
            let p = HybridPolicy {
                primary,
                ..whisper_hybrid(20)
            };
            assert!(
                !p.is_active(),
                "{primary:?} should not activate the fast lane"
            );
            assert_eq!(p.lane_for(2.0, true), SttLane::Primary);
        }
    }

    #[test]
    fn parakeet_missing_routes_to_primary() {
        let p = whisper_hybrid(20);
        assert!(p.is_active());
        assert_eq!(p.lane_for(2.0, false), SttLane::Primary);
    }

    #[test]
    fn short_dictation_routes_to_parakeet() {
        let p = whisper_hybrid(20);
        assert_eq!(p.lane_for(3.5, true), SttLane::FastParakeet);
    }

    #[test]
    fn long_dictation_routes_to_primary() {
        let p = whisper_hybrid(20);
        assert_eq!(p.lane_for(20.01, true), SttLane::Primary);
        assert_eq!(p.lane_for(95.0, true), SttLane::Primary);
    }

    #[test]
    fn threshold_is_inclusive() {
        let p = whisper_hybrid(20);
        assert_eq!(p.lane_for(20.0, true), SttLane::FastParakeet);
    }

    #[test]
    fn zero_threshold_never_routes_to_parakeet() {
        let p = whisper_hybrid(0);
        assert!(
            !p.is_active(),
            "nothing to warm up when no dictation can qualify"
        );
        assert_eq!(p.lane_for(0.1, true), SttLane::Primary);
    }
}
