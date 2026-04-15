use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum EngineType {
    #[default]
    Whisper,
    SenseVoice,
    Moonshine,
    MistralApi,
}

impl EngineType {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Whisper => "whisper",
            Self::SenseVoice => "sense-voice",
            Self::Moonshine => "moonshine",
            Self::MistralApi => "mistral-api",
        }
    }

    #[must_use]
    pub fn is_local(&self) -> bool {
        !matches!(self, Self::MistralApi)
    }

    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Whisper => "Whisper (local)",
            Self::SenseVoice => "SenseVoice (local)",
            Self::Moonshine => "Moonshine (local, EN)",
            Self::MistralApi => "Mistral Voxtral (cloud)",
        }
    }
}
