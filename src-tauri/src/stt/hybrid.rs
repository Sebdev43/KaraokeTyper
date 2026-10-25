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

/// Longest dictation Parakeet is trusted with once the user has named a language.
///
/// Parakeet V3 detects the language itself and offers no way to be told: NVIDIA's own
/// model card says it "automatically detects the language of the audio and transcribes
/// it without requiring additional prompting", and `transcribe-rs` carries a
/// `ParakeetParams::language` field its code never reads. On a short phrase that
/// detection lands; on a longer one it drifts mid-sentence and returns the two
/// languages braided together — "In parallel to the recherche and tutorial in the
/// detail", from a dictation entirely in French.
///
/// So a named language caps the fast lane here instead of being silently discarded:
/// short dictations keep Parakeet's instant answer, longer ones go to Whisper, which
/// is told the language and holds it. Only a dictation longer than this is affected,
/// and only when a language was actually chosen.
pub const PARAKEET_LANGUAGE_SAFE_SECS: u32 = 20;

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
    /// Whether the user named a language rather than leaving detection to the engine.
    ///
    /// Parakeet cannot be told a language, so this caps its share of the work — see
    /// [`PARAKEET_LANGUAGE_SAFE_SECS`].
    pub language_pinned: bool,
}

impl HybridPolicy {
    /// True when this policy can ever route a dictation to Parakeet — so a zero
    /// threshold, which `lane_for` never satisfies, also keeps Parakeet unloaded.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled && self.primary == EngineType::Whisper && self.threshold_secs > 0
    }

    /// The threshold actually applied, which is lower than the configured one when a
    /// language is pinned and the configured value exceeds what Parakeet can be
    /// trusted with.
    #[must_use]
    pub fn effective_threshold_secs(&self) -> u32 {
        if self.language_pinned {
            self.threshold_secs.min(PARAKEET_LANGUAGE_SAFE_SECS)
        } else {
            self.threshold_secs
        }
    }

    /// True when a pinned language is holding the threshold below the configured one,
    /// which the UI states rather than leaving the user to wonder.
    #[must_use]
    pub fn threshold_capped_by_language(&self) -> bool {
        self.language_pinned && self.threshold_secs > PARAKEET_LANGUAGE_SAFE_SECS
    }

    /// Lane for a recording of `duration_secs` (raw capture, before padding).
    ///
    /// `parakeet_installed` is resolved by the caller so this stays a pure decision.
    /// A missing Parakeet silently falls back to the primary engine: the user asked
    /// for the fast lane "if it is installed", not for an error when it is not.
    #[must_use]
    pub fn lane_for(&self, duration_secs: f32, parakeet_installed: bool) -> SttLane {
        if self.is_active()
            && parakeet_installed
            && duration_secs <= self.effective_threshold_secs() as f32
        {
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
            language_pinned: false,
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

    /// The reported defect, as a test. A 90 s threshold with French pinned used to
    /// send a 23.7 s dictation to Parakeet, which answered in braided
    /// French-and-English; it now goes to Whisper, which is told the language.
    #[test]
    fn a_pinned_language_keeps_long_dictations_off_parakeet() {
        let p = HybridPolicy {
            language_pinned: true,
            ..whisper_hybrid(90)
        };

        // Short: Parakeet's own detection is reliable here, and the speed is the point.
        assert_eq!(p.lane_for(9.2, true), SttLane::FastParakeet);

        // Long: this is the case that came back as franglais.
        assert_eq!(p.lane_for(23.7, true), SttLane::Primary);

        assert_eq!(p.effective_threshold_secs(), PARAKEET_LANGUAGE_SAFE_SECS);
        assert!(p.threshold_capped_by_language());
    }

    /// Without a pinned language the user asked for detection, so the configured
    /// threshold is left exactly as it is.
    #[test]
    fn detection_mode_keeps_the_configured_threshold() {
        let p = whisper_hybrid(90);
        assert_eq!(p.effective_threshold_secs(), 90);
        assert!(!p.threshold_capped_by_language());
        assert_eq!(p.lane_for(23.7, true), SttLane::FastParakeet);
        assert_eq!(p.lane_for(89.0, true), SttLane::FastParakeet);
    }

    /// A threshold already below the cap is the user's own, stricter choice and is
    /// never raised to meet it.
    #[test]
    fn a_threshold_below_the_cap_is_left_alone() {
        let p = HybridPolicy {
            language_pinned: true,
            ..whisper_hybrid(5)
        };
        assert_eq!(p.effective_threshold_secs(), 5);
        assert!(!p.threshold_capped_by_language());
        assert_eq!(p.lane_for(9.2, true), SttLane::Primary);
    }
}
