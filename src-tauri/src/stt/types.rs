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
}

impl EngineType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::SenseVoice => "sense-voice",
            Self::Moonshine => "moonshine",
        }
    }

    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Whisper => "Whisper",
            Self::SenseVoice => "SenseVoice",
            Self::Moonshine => "Moonshine (EN)",
        }
    }
}
