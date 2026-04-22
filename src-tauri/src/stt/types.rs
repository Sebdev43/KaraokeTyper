use serde::{Deserialize, Serialize};
use specta::Type;

/// Self-hosted STT backends. No cloud APIs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum EngineType {
    #[default]
    Whisper,
    /// NVIDIA Parakeet-TDT-0.6B v3, int8 ONNX quantization.
    /// Supports 25 EU languages + RU/UK natively (language is auto-detected
    /// by the model — we don't pass a hint at inference time, unlike Whisper).
    Parakeet,
    SenseVoice,
    Moonshine,
}

impl EngineType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::Parakeet => "parakeet",
            Self::SenseVoice => "sense-voice",
            Self::Moonshine => "moonshine",
        }
    }

    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Whisper => "Whisper",
            Self::Parakeet => "Parakeet V3 (FR + UE)",
            Self::SenseVoice => "SenseVoice",
            Self::Moonshine => "Moonshine (EN)",
        }
    }

    /// Languages supported by this engine (lowercase ISO 639-1 codes).
    /// Used by the UI to restrict the language picker when the engine is
    /// selected. Empty Vec means "auto-detection only, no user pick".
    #[must_use]
    pub fn supported_languages(&self) -> Vec<&'static str> {
        match self {
            // Whisper supports 99 languages — we let the user pick anything.
            Self::Whisper => vec![],
            // Parakeet V3: 25 EU languages + RU/UK (27 total). See NVIDIA's
            // model card; this list mirrors Handy's `parakeet_v3_languages`.
            Self::Parakeet => vec![
                "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt",
                "lv", "mt", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
            ],
            // Moonshine is English-only.
            Self::Moonshine => vec!["en"],
            // SenseVoice supports EN/ZH/JA/KO/YUE/AUTO; we keep auto-detect.
            Self::SenseVoice => vec!["auto", "en", "zh", "ja", "ko", "yue"],
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
        ] {
            let encoded = serde_json::to_string(&variant).expect("encode");
            let stripped = encoded.trim_matches('"');
            assert_eq!(stripped, variant.as_str());
        }
    }
}
