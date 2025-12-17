use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub shortcut: String,
    pub audio_device_index: usize,
    #[serde(default)]
    pub audio_device_name: Option<String>,  // Device name for robust matching across OS/versions
    pub model: String,
    pub key_delay_ms: u64,
    pub language: String,  // Whisper language code (en, fr, es, de, etc.)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: "CommandOrControl+Space".to_string(),
            audio_device_index: 0,
            audio_device_name: None,  // Will use default device
            model: "base".to_string(),
            key_delay_ms: 8,
            language: "fr".to_string(),  // Default to French
        }
    }
}

impl AppConfig {
    pub fn config_path() -> Result<PathBuf, String> {
        let config_dir = dirs::config_dir()
            .ok_or("Failed to get config directory")?;

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

        let config: AppConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        Ok(config)
    }

    pub fn save(&self) -> Result<(), String> {
        let config_path = Self::config_path()?;

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        fs::write(&config_path, json)
            .map_err(|e| format!("Failed to write config file: {}", e))?;

        Ok(())
    }
}
