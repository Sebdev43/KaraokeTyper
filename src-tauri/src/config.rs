use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::PathBuf;

use crate::stt::EngineType;

/// How text is pasted into the focused application.
/// Inspired by Handy's approach: user picks the method, no auto-detection.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub enum PasteMethod {
    /// Type text directly via wtype/xdotool/dotool (no clipboard).
    /// Works universally — terminals, editors, remote desktop apps.
    Direct,
    /// Clipboard + Ctrl+V (standard apps).
    CtrlV,
    /// Clipboard + Ctrl+Shift+V (terminals).
    CtrlShiftV,
    /// Clipboard + Shift+Insert (some terminals, tmux).
    ShiftInsert,
    /// Do nothing (disable paste).
    None,
}

impl Default for PasteMethod {
    fn default() -> Self {
        #[cfg(target_os = "linux")]
        return PasteMethod::Direct;
        #[cfg(not(target_os = "linux"))]
        return PasteMethod::CtrlV;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
pub enum ModelUnloadTimeout {
    #[default]
    Never,
    Min5,
    Min15,
    Hour1,
}

impl ModelUnloadTimeout {
    pub fn to_seconds(&self) -> Option<u64> {
        match self {
            Self::Never => None,
            Self::Min5 => Some(300),
            Self::Min15 => Some(900),
            Self::Hour1 => Some(3600),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppConfig {
    pub shortcut: String,
    pub audio_device_index: u32,
    #[serde(default)]
    pub audio_device_name: Option<String>,
    pub model: String,
    pub key_delay_ms: u32,
    pub language: String,
    #[serde(default)]
    pub model_path: Option<String>,
    #[serde(default = "auto_detect_gpu")]
    pub use_gpu: bool,
    #[serde(default)]
    pub gpu_device_index: Option<u32>,
    #[serde(default = "default_true")]
    pub audio_feedback_enabled: bool,
    #[serde(default = "default_volume")]
    pub audio_feedback_volume: f32,
    #[serde(default)]
    pub model_unload_timeout: ModelUnloadTimeout,
    #[serde(default)]
    pub ollama_url: Option<String>,
    #[serde(default)]
    pub ollama_model: Option<String>,
    #[serde(default)]
    pub ollama_enabled: bool,
    #[serde(default)]
    pub paste_method: PasteMethod,
    /// Custom words for fuzzy correction after transcription.
    /// Words that Whisper frequently misrecognizes (names, technical terms, etc.)
    #[serde(default)]
    pub custom_words: Vec<String>,
    /// Fuzzy matching threshold (0.0 = exact, 1.0 = anything matches).
    /// Default: 0.3 — good balance for French speech correction.
    #[serde(default = "default_custom_words_threshold")]
    pub custom_words_threshold: f32,
    /// Enable filler word removal ("euh", "um", "ah", etc.)
    #[serde(default = "default_true")]
    pub filler_removal_enabled: bool,
    /// Enable stutter collapse ("je je je veux" → "je veux")
    #[serde(default = "default_true")]
    pub stutter_collapse_enabled: bool,
    /// Selected speech-to-text backend (Whisper by default for backwards compat).
    #[serde(default)]
    pub stt_backend: EngineType,
    /// Optional Mistral API key (only used when `stt_backend == MistralApi`).
    #[serde(default)]
    pub mistral_api_key: Option<String>,
    /// Override base URL for the Mistral-compatible API. Defaults to
    /// `https://api.mistral.ai/v1` when None.
    #[serde(default)]
    pub mistral_base_url: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_volume() -> f32 {
    0.7
}
fn default_custom_words_threshold() -> f32 {
    0.3
}

/// Auto-detect GPU: enable if compiled with GPU support
fn auto_detect_gpu() -> bool {
    let gpu_compiled = cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal");
    if gpu_compiled {
        println!("[GPU] Auto-enabling GPU (compiled with GPU support, first launch or new field)");
    }
    gpu_compiled
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: "CommandOrControl+Space".to_string(),
            audio_device_index: 0,
            audio_device_name: None,
            model: "base".to_string(),
            key_delay_ms: 8,
            language: "fr".to_string(),
            model_path: None,
            use_gpu: auto_detect_gpu(),
            gpu_device_index: None,
            audio_feedback_enabled: true,
            audio_feedback_volume: 0.7,
            model_unload_timeout: ModelUnloadTimeout::Never,
            ollama_url: None,
            ollama_model: None,
            ollama_enabled: false,
            paste_method: PasteMethod::default(),
            custom_words: Vec::new(),
            custom_words_threshold: default_custom_words_threshold(),
            filler_removal_enabled: true,
            stutter_collapse_enabled: true,
            stt_backend: EngineType::default(),
            mistral_api_key: None,
            mistral_base_url: None,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Result<PathBuf, String> {
        let config_dir = dirs::config_dir().ok_or("Failed to get config directory")?;

        let app_config_dir = config_dir.join("karaoketyper");

        if !app_config_dir.exists() {
            fs::create_dir_all(&app_config_dir)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }

        Ok(app_config_dir.join("settings.json"))
    }

    pub fn load() -> Result<Self, String> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            let default_config = Self::default();
            default_config.save()?;
            return Ok(default_config);
        }

        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let mut config: AppConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        // Migration: auto-enable GPU if compiled with support but config has it disabled
        // This handles upgrading from builds without GPU to builds with GPU
        let gpu_compiled = cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal");
        if gpu_compiled && !config.use_gpu {
            println!("[CONFIG] Migration: enabling GPU (compiled with GPU support)");
            config.use_gpu = true;
            let _ = config.save();
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<(), String> {
        let config_path = Self::config_path()?;

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&config_path, json).map_err(|e| format!("Failed to write config file: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let config = AppConfig::default();
        assert_eq!(config.shortcut, "CommandOrControl+Space");
        assert_eq!(config.audio_device_index, 0);
        assert_eq!(config.model, "base");
        assert_eq!(config.key_delay_ms, 8);
        assert_eq!(config.language, "fr");
        // use_gpu depends on compile-time features (auto-detect)
        assert_eq!(
            config.use_gpu,
            cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal")
        );
        assert!(config.gpu_device_index.is_none());
        assert!(config.model_path.is_none());
        assert!(config.audio_device_name.is_none());
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let config = AppConfig {
            shortcut: "Alt+S".to_string(),
            audio_device_index: 2,
            audio_device_name: Some("USB Mic".to_string()),
            model: "large".to_string(),
            key_delay_ms: 15,
            language: "en".to_string(),
            model_path: Some("/path/to/model".to_string()),
            use_gpu: true,
            gpu_device_index: Some(1),
            audio_feedback_enabled: true,
            audio_feedback_volume: 0.5,
            model_unload_timeout: ModelUnloadTimeout::Min5,
            ollama_url: Some("http://localhost:11434".to_string()),
            ollama_model: Some("mistral".to_string()),
            ollama_enabled: false,
            paste_method: PasteMethod::CtrlShiftV,
            custom_words: vec!["Claude".to_string(), "Tauri".to_string()],
            custom_words_threshold: 0.25,
            filler_removal_enabled: true,
            stutter_collapse_enabled: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.shortcut, "Alt+S");
        assert_eq!(restored.audio_device_index, 2);
        assert_eq!(restored.audio_device_name.as_deref(), Some("USB Mic"));
        assert_eq!(restored.model, "large");
        assert_eq!(restored.language, "en");
        assert!(restored.use_gpu);
        assert_eq!(restored.gpu_device_index, Some(1));
    }

    #[test]
    fn test_deserialize_with_missing_optional_fields() {
        let json = r#"{
            "shortcut": "Ctrl+Space",
            "audio_device_index": 0,
            "model": "base",
            "key_delay_ms": 8,
            "language": "fr"
        }"#;

        let config: AppConfig = serde_json::from_str(json).unwrap();
        // use_gpu depends on compile-time features (auto-detect)
        assert_eq!(
            config.use_gpu,
            cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal")
        );
        assert!(config.gpu_device_index.is_none());
        assert!(config.model_path.is_none());
        assert!(config.audio_device_name.is_none());
    }
}
