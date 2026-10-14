use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::path::PathBuf;

use crate::i18n::UiLanguage;
use crate::stt::{EngineType, HybridPolicy};

/// How the shortcut's press and release drive a recording.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
pub enum ShortcutActivation {
    /// Hold to record, release to stop (push-to-talk).
    #[default]
    Hold,
    /// Press to start, press again to stop.
    Toggle,
    /// Hold to record and release to stop, or tap to keep recording until the next
    /// press. Which one it was is decided by how long the key was held
    /// (`hold_threshold_ms`).
    HoldOrToggle,
}

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

/// The container-level `serde(default)` is what keeps a partial or slightly-wrong
/// settings file loadable: any missing field — including every field added in the
/// future — falls back to its `Default` value instead of failing the whole parse.
/// Field-level defaults below still take precedence where present.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(default)]
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
    /// Selected speech-to-text backend (Whisper by default, all backends are self-hosted).
    #[serde(default)]
    pub stt_backend: EngineType,
    /// Route short dictations to Parakeet (when installed) while Whisper stays the
    /// primary engine. Parakeet is far faster; Whisper keeps the long dictations.
    #[serde(default)]
    pub hybrid_engine_enabled: bool,
    /// Longest dictation (seconds) Parakeet handles in hybrid mode; longer ones go to Whisper.
    #[serde(default = "default_hybrid_threshold_secs")]
    pub hybrid_threshold_secs: u32,
    /// Hold (the historical push-to-talk), toggle, or hold-or-toggle.
    #[serde(default)]
    pub shortcut_activation: ShortcutActivation,
    /// Hold-or-toggle only: a press held at least this long is push-to-talk, anything
    /// shorter is a tap that locks recording on.
    #[serde(default = "default_hold_threshold_ms")]
    pub hold_threshold_ms: u32,
    /// Language of the interface — the windows, the tray and the messages the
    /// backend writes. Independent of `language`, which is what the speech
    /// engine transcribes: dictating in French from an English interface is a
    /// normal combination, not a mistake to prevent.
    ///
    /// Absent from an older settings file, so the `Default` (the system locale)
    /// decides on first run after the upgrade.
    #[serde(default)]
    pub ui_language: UiLanguage,
}

/// Handy's value: comfortably longer than a tap, shorter than the shortest dictation.
const DEFAULT_HOLD_THRESHOLD_MS: u32 = 300;

/// What the settings UI offers; a hand-edited value outside it is clamped when used.
const HOLD_THRESHOLD_MS: std::ops::RangeInclusive<u32> = 100..=1000;

fn default_hold_threshold_ms() -> u32 {
    DEFAULT_HOLD_THRESHOLD_MS
}

impl AppConfig {
    /// `hold_threshold_ms` as a `Duration`, clamped to the range the UI offers.
    pub fn hold_threshold(&self) -> std::time::Duration {
        let ms = self
            .hold_threshold_ms
            .clamp(*HOLD_THRESHOLD_MS.start(), *HOLD_THRESHOLD_MS.end());
        std::time::Duration::from_millis(u64::from(ms))
    }
}

/// Wait between writing the clipboard and sending the paste shortcut.
/// 60 ms is the value the paste pipeline actually used for as long as this
/// setting was bypassed, so it is the timing every existing install is tuned to.
const DEFAULT_KEY_DELAY_MS: u32 = 60;

/// The `key_delay_ms` default shipped while the setting was never read — the paste
/// pipeline passed a literal 60 instead. A stored 8 is therefore dead data, not a
/// preference (there is no UI control for this field), and `load()` rewrites it.
const LEGACY_UNUSED_KEY_DELAY_MS: u32 = 8;

/// A sentence or two; long enough that a quick note stays on Parakeet, short enough
/// that a real paragraph still gets the Whisper pass.
const DEFAULT_HYBRID_THRESHOLD_SECS: u32 = 20;

fn default_true() -> bool {
    true
}
fn default_hybrid_threshold_secs() -> u32 {
    DEFAULT_HYBRID_THRESHOLD_SECS
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
        crate::dev_log!(
            "[GPU] Auto-enabling GPU (compiled with GPU support, first launch or new field)"
        );
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
            key_delay_ms: DEFAULT_KEY_DELAY_MS,
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
            hybrid_engine_enabled: false,
            hybrid_threshold_secs: DEFAULT_HYBRID_THRESHOLD_SECS,
            shortcut_activation: ShortcutActivation::default(),
            hold_threshold_ms: default_hold_threshold_ms(),
            ui_language: UiLanguage::default(),
        }
    }
}

impl AppConfig {
    /// The hybrid-routing settings, as a `Copy` snapshot that outlives any lock.
    #[must_use]
    pub fn hybrid_policy(&self) -> HybridPolicy {
        HybridPolicy::from(self)
    }
}

impl From<&AppConfig> for HybridPolicy {
    fn from(config: &AppConfig) -> Self {
        Self {
            primary: config.stt_backend,
            enabled: config.hybrid_engine_enabled,
            threshold_secs: config.hybrid_threshold_secs,
        }
    }
}

/// Rebuild a config from a settings file that failed to deserialize as a whole.
///
/// Every stored field that is individually valid is kept; only the broken ones fall
/// back to their default. Without this, a single unreadable value — an enum variant
/// written by a newer build, a hand-edited typo — discards every other setting the
/// user ever chose, because `load()` fails and the caller falls back to `default()`.
///
/// Works by overwriting the defaults one field at a time and re-deserializing after
/// each: whichever assignment breaks the parse is the bad one, and only that field is
/// reverted. That is one parse per known field — a bounded ~20 — regardless of what
/// the file contains, so a corrupt or hostile settings.json cannot turn startup into
/// a quadratic grind. Keys the struct does not know are skipped outright; the final
/// deserialization would discard them anyway.
fn salvage_config(stored: &serde_json::Value) -> AppConfig {
    use serde::Deserialize;

    let Some(stored_map) = stored.as_object() else {
        crate::dev_log!("[CONFIG] Stored config is not a JSON object; using defaults");
        return AppConfig::default();
    };

    let Ok(serde_json::Value::Object(mut merged)) = serde_json::to_value(AppConfig::default())
    else {
        return AppConfig::default();
    };

    for key in merged.keys().cloned().collect::<Vec<_>>() {
        let Some(value) = stored_map.get(&key) else {
            continue;
        };

        let Some(previous) = merged.insert(key.clone(), value.clone()) else {
            continue;
        };

        if AppConfig::deserialize(&serde_json::Value::Object(merged.clone())).is_err() {
            // Log the key only — values carry the Ollama URL and the custom word list.
            crate::dev_log!(
                "[CONFIG] Dropping invalid field '{}', keeping its default",
                key
            );
            merged.insert(key, previous);
        }
    }

    AppConfig::deserialize(&serde_json::Value::Object(merged)).unwrap_or_else(|_| {
        crate::dev_log!("[CONFIG] Could not reassemble salvaged config; using defaults");
        AppConfig::default()
    })
}

/// Whisper catalogue names before the move to GGUF, and what they are called now.
///
/// The files themselves are not interchangeable — the GGML `.bin` a previous
/// version downloaded cannot be read by the GGUF engine — so this only keeps the
/// settings pointing at a model that still exists in the catalogue. The Models
/// screen then shows it as not installed, and the engine says why if it is asked
/// to load the old file.
const RENAMED_WHISPER_MODELS: &[(&str, &str)] = &[
    ("ggml-tiny", "whisper-tiny"),
    ("ggml-base", "whisper-base"),
    ("ggml-small", "whisper-small"),
    ("ggml-medium", "whisper-medium"),
    ("ggml-large-v3", "whisper-large-v3"),
];

/// One-time fix-ups applied on load. Returns true when the config changed and should
/// be written back.
#[must_use = "true means the config must be written back to disk"]
fn apply_migrations(config: &mut AppConfig) -> bool {
    let mut changed = false;

    if let Some((_, new_name)) = RENAMED_WHISPER_MODELS
        .iter()
        .find(|(old, _)| *old == config.model)
    {
        crate::dev_log!(
            "[CONFIG] Migration: Whisper model {} is now {} (GGUF)",
            config.model,
            new_name
        );
        config.model = (*new_name).to_string();
        // The stored path points at the old `.bin`; dropping it sends the loader
        // back to the catalogue instead of failing on a file the engine refuses.
        config.model_path = None;
        changed = true;
    }

    // Auto-enable GPU if this build has GPU support but the stored config predates it.
    // NOTE: this is pre-existing behaviour and it is NOT idempotent — `use_gpu` has a
    // toggle in the settings window, so a user who deliberately turns the GPU off gets
    // it turned back on at every launch. `#[serde(default = "auto_detect_gpu")]` on the
    // field already covers the pre-GPU-config case this was written for, which makes
    // the migration redundant as well as destructive. Left alone here because changing
    // GPU behaviour is outside this change; see the findings log.
    let gpu_compiled = cfg!(feature = "gpu-vulkan") || cfg!(feature = "gpu-metal");
    if gpu_compiled && !config.use_gpu {
        crate::dev_log!("[CONFIG] Migration: enabling GPU (compiled with GPU support)");
        config.use_gpu = true;
        changed = true;
    }

    // `key_delay_ms` used to be ignored, so a stored 8 is the old dead default rather
    // than a choice. Now that the paste pipeline reads it, leaving it would cut the
    // pre-paste wait from the 60 ms every install actually runs on down to 8 ms.
    if config.key_delay_ms == LEGACY_UNUSED_KEY_DELAY_MS {
        crate::dev_log!("[CONFIG] Migration: key_delay_ms 8 -> 60 (was never in effect)");
        config.key_delay_ms = DEFAULT_KEY_DELAY_MS;
        changed = true;
    }

    changed
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

        let (config, needs_save) = Self::from_content(&content)?;

        if needs_save {
            if let Err(e) = config.save() {
                // Not fatal — the config is usable in memory. But it means the salvage
                // or migration will run again on every launch, so say so.
                crate::dev_log!("[CONFIG] Could not persist repaired config: {}", e);
            }
        }

        Ok(config)
    }

    /// Parse settings-file contents into a config, repairing what can be repaired.
    ///
    /// Split out from `load()` so the interesting half — salvage, then migrations,
    /// then "does this need writing back" — is testable without touching the real
    /// settings.json. Returns the config and whether it differs from what was read.
    fn from_content(content: &str) -> Result<(Self, bool), String> {
        let mut needs_save = false;

        let mut config: AppConfig = match serde_json::from_str(content) {
            Ok(config) => config,
            Err(e) => {
                crate::dev_log!("[CONFIG] Parse failed ({}); salvaging valid fields", e);
                let stored: serde_json::Value = serde_json::from_str(content)
                    .map_err(|e| format!("Failed to parse config file: {}", e))?;
                needs_save = true;
                salvage_config(&stored)
            }
        };

        if apply_migrations(&mut config) {
            needs_save = true;
        }

        Ok((config, needs_save))
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
        assert_eq!(config.key_delay_ms, DEFAULT_KEY_DELAY_MS);
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
            stt_backend: EngineType::default(),
            hybrid_engine_enabled: true,
            hybrid_threshold_secs: 45,
            shortcut_activation: ShortcutActivation::default(),
            hold_threshold_ms: default_hold_threshold_ms(),
            ui_language: UiLanguage::En,
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
        assert!(restored.hybrid_engine_enabled);
        assert_eq!(restored.hybrid_threshold_secs, 45);
    }

    #[test]
    fn hybrid_defaults_are_off_and_20s() {
        let config = AppConfig::default();
        assert!(!config.hybrid_engine_enabled);
        assert_eq!(config.hybrid_threshold_secs, 20);
    }

    #[test]
    fn legacy_config_without_hybrid_fields_uses_defaults() {
        let json = r#"{
            "shortcut": "Ctrl+Space",
            "audio_device_index": 0,
            "model": "ggml-large-v3",
            "key_delay_ms": 60,
            "language": "fr",
            "stt_backend": "whisper"
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!config.hybrid_engine_enabled);
        assert_eq!(config.hybrid_threshold_secs, DEFAULT_HYBRID_THRESHOLD_SECS);
        assert!(!config.hybrid_policy().is_active());
    }

    #[test]
    fn hybrid_policy_mirrors_config() {
        let config = AppConfig {
            stt_backend: EngineType::Whisper,
            hybrid_engine_enabled: true,
            hybrid_threshold_secs: 12,
            ..AppConfig::default()
        };
        let policy = config.hybrid_policy();
        assert_eq!(
            policy,
            HybridPolicy {
                primary: EngineType::Whisper,
                enabled: true,
                threshold_secs: 12,
            }
        );
        assert!(policy.is_active());
    }

    #[test]
    fn a_stored_ggml_model_name_is_migrated_to_its_gguf_entry() {
        let mut config = AppConfig {
            model: "ggml-large-v3".to_string(),
            model_path: Some("/home/u/.local/share/karaoketyper/models/ggml-large-v3.bin".into()),
            ..AppConfig::default()
        };
        assert!(
            apply_migrations(&mut config),
            "the config must be rewritten"
        );
        assert_eq!(
            config.model, "whisper-large-v3",
            "the old name points at nothing in the catalogue"
        );
        assert_eq!(
            config.model_path, None,
            "the stored path is the GGML file the engine refuses; the loader must \
             fall back to the catalogue"
        );

        // Every renamed entry must exist in the shipped catalogue, or the settings
        // would point at a model the Models screen cannot show.
        for (_, new_name) in RENAMED_WHISPER_MODELS {
            assert!(
                crate::catalog::models().iter().any(|m| m.name == *new_name),
                "{new_name} is missing from the catalogue"
            );
        }
    }

    #[test]
    fn a_model_name_the_catalogue_still_knows_is_left_alone() {
        let mut config = AppConfig {
            model: "whisper-large-v3".to_string(),
            model_path: None,
            ..AppConfig::default()
        };
        let before = config.model.clone();
        let _ = apply_migrations(&mut config);
        assert_eq!(config.model, before);
    }

    #[test]
    fn legacy_config_without_stt_backend_defaults_to_whisper() {
        let json = r#"{
            "shortcut": "Ctrl+Space",
            "audio_device_index": 0,
            "model": "ggml-small",
            "key_delay_ms": 8,
            "language": "fr"
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.stt_backend, EngineType::Whisper);
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
        // Existing installs keep push-to-talk exactly as it was.
        assert_eq!(config.shortcut_activation, ShortcutActivation::Hold);
        assert_eq!(config.hold_threshold_ms, DEFAULT_HOLD_THRESHOLD_MS);
    }

    #[test]
    fn shortcut_activation_round_trips_every_mode() {
        for mode in [
            ShortcutActivation::Hold,
            ShortcutActivation::Toggle,
            ShortcutActivation::HoldOrToggle,
        ] {
            let json = serde_json::to_string(&mode).expect("serialize");
            let back: ShortcutActivation = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, mode, "{json}");
        }
    }

    fn default_config_json() -> serde_json::Value {
        serde_json::to_value(AppConfig::default()).unwrap()
    }

    /// Every field must survive a partial file: a missing key must never fail the
    /// whole parse. `{}` is the extreme case, and it is what guards future fields.
    #[test]
    fn empty_object_parses_with_every_default() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({}))
            .expect("every AppConfig field needs a serde default");
        assert_eq!(config.shortcut, "CommandOrControl+Space");
        assert_eq!(config.model, "base");
        assert_eq!(config.language, "fr");
        assert_eq!(config.key_delay_ms, DEFAULT_KEY_DELAY_MS);
    }

    #[test]
    fn salvage_keeps_valid_fields_when_one_value_is_invalid() {
        let mut stored = default_config_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("shortcut".into(), serde_json::json!("Alt+X"));
        map.insert("language".into(), serde_json::json!("en"));
        map.insert("custom_words".into(), serde_json::json!(["ChargeBee"]));
        // An enum variant this build does not know — e.g. written by a newer version.
        map.insert("paste_method".into(), serde_json::json!("MiddleClick"));

        // Precondition: this is the whole-file parse failure that used to wipe everything.
        assert!(serde_json::from_value::<AppConfig>(stored.clone()).is_err());

        let salvaged = salvage_config(&stored);
        assert_eq!(salvaged.shortcut, "Alt+X");
        assert_eq!(salvaged.language, "en");
        assert_eq!(salvaged.custom_words, vec!["ChargeBee".to_string()]);
        assert_eq!(salvaged.paste_method, PasteMethod::default());
    }

    #[test]
    fn salvage_drops_only_the_wrong_typed_fields() {
        let mut stored = default_config_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("key_delay_ms".into(), serde_json::json!("soixante"));
        map.insert("audio_feedback_volume".into(), serde_json::json!("loud"));
        map.insert("model".into(), serde_json::json!("large-v3"));

        assert!(serde_json::from_value::<AppConfig>(stored.clone()).is_err());

        let salvaged = salvage_config(&stored);
        assert_eq!(salvaged.key_delay_ms, DEFAULT_KEY_DELAY_MS);
        assert_eq!(salvaged.audio_feedback_volume, 0.7);
        assert_eq!(salvaged.model, "large-v3");
    }

    #[test]
    fn salvage_tolerates_unknown_keys() {
        let mut stored = default_config_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("field_from_a_later_version".into(), serde_json::json!(true));
        map.insert("model".into(), serde_json::json!("kept"));

        let salvaged = salvage_config(&stored);
        assert_eq!(salvaged.model, "kept");
    }

    #[test]
    fn salvage_of_non_object_falls_back_to_defaults() {
        for stored in [
            serde_json::json!("corrupt"),
            serde_json::json!(null),
            serde_json::json!([1, 2, 3]),
        ] {
            let salvaged = salvage_config(&stored);
            assert_eq!(
                serde_json::to_value(&salvaged).unwrap(),
                default_config_json()
            );
        }
    }

    /// The stored 8 was never in effect, so rewriting it is not overriding a choice.
    #[test]
    fn migration_rewrites_the_dead_key_delay_default() {
        let mut config = AppConfig {
            key_delay_ms: LEGACY_UNUSED_KEY_DELAY_MS,
            ..AppConfig::default()
        };
        assert!(apply_migrations(&mut config));
        assert_eq!(config.key_delay_ms, DEFAULT_KEY_DELAY_MS);
    }

    #[test]
    fn migration_preserves_a_deliberate_key_delay() {
        let mut config = AppConfig {
            key_delay_ms: 120,
            ..AppConfig::default()
        };
        let _ = apply_migrations(&mut config);
        assert_eq!(config.key_delay_ms, 120);
    }

    /// A current-format config must not be rewritten on every read.
    ///
    /// Scoped to `key_delay_ms` on purpose: the GPU migration in the same function is
    /// genuinely non-idempotent when the build has GPU support and the user turned the
    /// GPU off, so this cannot claim idempotency for `apply_migrations` as a whole.
    #[test]
    fn key_delay_migration_is_idempotent() {
        let mut config = AppConfig {
            key_delay_ms: LEGACY_UNUSED_KEY_DELAY_MS,
            ..AppConfig::default()
        };
        assert!(apply_migrations(&mut config));
        assert!(!apply_migrations(&mut config));
    }

    /// Documents the pre-existing GPU-migration defect rather than pretending it away:
    /// on a GPU build, an explicit `use_gpu: false` is overwritten on every load.
    #[cfg(any(feature = "gpu-vulkan", feature = "gpu-metal"))]
    #[test]
    fn gpu_migration_overrides_an_explicit_opt_out() {
        let mut config = AppConfig {
            use_gpu: false,
            ..AppConfig::default()
        };
        assert!(
            apply_migrations(&mut config),
            "known defect: the GPU migration is not idempotent"
        );
        assert!(config.use_gpu);
    }

    #[test]
    fn from_content_reports_no_rewrite_for_a_current_config() {
        let content = serde_json::to_string(&AppConfig::default()).unwrap();
        let (config, needs_save) = AppConfig::from_content(&content).unwrap();
        assert!(!needs_save);
        assert_eq!(config.key_delay_ms, DEFAULT_KEY_DELAY_MS);
    }

    #[test]
    fn from_content_salvages_and_asks_to_be_rewritten() {
        let mut stored = default_config_json();
        let map = stored.as_object_mut().unwrap();
        map.insert("model".into(), serde_json::json!("large-v3"));
        map.insert("paste_method".into(), serde_json::json!("MiddleClick"));

        let (config, needs_save) =
            AppConfig::from_content(&serde_json::to_string(&stored).unwrap()).unwrap();

        assert!(needs_save, "a repaired config must be written back");
        assert_eq!(config.model, "large-v3");
        assert_eq!(config.paste_method, PasteMethod::default());
    }

    #[test]
    fn from_content_rejects_content_that_is_not_json() {
        assert!(AppConfig::from_content("this is not json at all").is_err());
    }

    /// A settings file stuffed with unknown keys must not make startup quadratic.
    #[test]
    fn salvage_ignores_unknown_keys_without_reparsing_for_each() {
        let mut stored = default_config_json();
        let map = stored.as_object_mut().unwrap();
        for i in 0..5_000 {
            map.insert(format!("junk_{}", i), serde_json::json!("x".repeat(64)));
        }
        map.insert("model".into(), serde_json::json!("kept"));

        let salvaged = salvage_config(&stored);
        assert_eq!(salvaged.model, "kept");
    }
}
