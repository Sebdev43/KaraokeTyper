use serde::{Deserialize, Serialize};
use specta::Type;

/// Self-hosted STT backends. No cloud APIs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum EngineType {
    #[default]
    Whisper,
    SenseVoice,
    Moonshine,
    /// Mistral Voxtral Mini 4B Realtime (auto-hébergé via voxtral-mini-realtime-rs).
    /// Requires `--features voxtral` at compile time.
    Voxtral,
}

impl EngineType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::SenseVoice => "sense-voice",
            Self::Moonshine => "moonshine",
            Self::Voxtral => "voxtral",
        }
    }

    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Whisper => "Whisper",
            Self::SenseVoice => "SenseVoice",
            Self::Moonshine => "Moonshine (EN)",
            Self::Voxtral => "Voxtral Mini (self-hosted)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_whisper() {
        assert_eq!(EngineType::default(), EngineType::Whisper);
    }

    #[test]
    fn serde_kebab_case_roundtrip() {
        for (variant, tag) in [
            (EngineType::Whisper, "\"whisper\""),
            (EngineType::SenseVoice, "\"sense-voice\""),
            (EngineType::Moonshine, "\"moonshine\""),
            (EngineType::Voxtral, "\"voxtral\""),
        ] {
            let encoded = serde_json::to_string(&variant).expect("encode");
            assert_eq!(encoded, tag);
            let decoded: EngineType = serde_json::from_str(tag).expect("decode");
            assert_eq!(decoded, variant);
        }
    }

    #[test]
    fn as_str_matches_serialized_tag() {
        for variant in [
            EngineType::Whisper,
            EngineType::SenseVoice,
            EngineType::Moonshine,
            EngineType::Voxtral,
        ] {
            let encoded = serde_json::to_string(&variant).expect("encode");
            let stripped = encoded.trim_matches('"');
            assert_eq!(stripped, variant.as_str());
        }
    }
}
