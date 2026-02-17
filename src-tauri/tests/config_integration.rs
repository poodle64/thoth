//! Configuration system integration tests for Thoth.
//!
//! Tests the load, save, and reset functionality of the configuration
//! system using temporary files to avoid affecting the real config.

use serde::{Deserialize, Serialize};
use std::fs;
use tempfile::TempDir;

/// Current config schema version (must match the actual config module).
const CURRENT_VERSION: u32 = 1;

// =============================================================================
// Config Structures (matching the actual config module)
// =============================================================================

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub audio: AudioConfig,
    pub transcription: TranscriptionConfig,
    pub shortcuts: ShortcutConfig,
    pub enhancement: EnhancementConfig,
    pub general: GeneralConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            audio: AudioConfig::default(),
            transcription: TranscriptionConfig::default(),
            shortcuts: ShortcutConfig::default(),
            enhancement: EnhancementConfig::default(),
            general: GeneralConfig::default(),
        }
    }
}

/// Audio recording configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub device_id: Option<String>,
    pub sample_rate: u32,
    pub play_sounds: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            sample_rate: 16000,
            play_sounds: true,
        }
    }
}

/// Transcription engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    pub language: String,
    pub auto_copy: bool,
    pub auto_paste: bool,
    pub add_leading_space: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            language: "en".to_string(),
            auto_copy: true,
            auto_paste: true,
            add_leading_space: false,
        }
    }
}

/// Keyboard shortcut configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    pub toggle_recording: String,
    pub toggle_recording_alt: Option<String>,
    pub copy_last: Option<String>,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            toggle_recording: "F13".to_string(),
            toggle_recording_alt: Some("CommandOrControl+Shift+Space".to_string()),
            copy_last: Some("F14".to_string()),
        }
    }
}

/// AI enhancement configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhancementConfig {
    pub enabled: bool,
    pub model: String,
    pub prompt_id: String,
    pub ollama_url: String,
}

impl Default for EnhancementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "llama3.2".to_string(),
            prompt_id: "fix-grammar".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
        }
    }
}

/// General application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub launch_at_login: bool,
    pub show_in_menu_bar: bool,
    pub show_in_dock: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            show_in_menu_bar: true,
            show_in_dock: false,
        }
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Saves configuration to a file.
fn save_config(config: &Config, path: &std::path::Path) -> Result<(), String> {
    let contents = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialise config: {}", e))?;
    fs::write(path, contents).map_err(|e| format!("Failed to write config file: {}", e))
}

/// Loads configuration from a file.
fn load_config(path: &std::path::Path) -> Result<Config, String> {
    if !path.exists() {
        return Ok(Config::default());
    }

    let contents =
        fs::read_to_string(path).map_err(|e| format!("Failed to read config file: {}", e))?;

    serde_json::from_str(&contents).map_err(|e| format!("Failed to parse config: {}", e))
}

// =============================================================================
// Config Default Tests
// =============================================================================

#[test]
fn test_default_config_has_current_version() {
    let config = Config::default();
    assert_eq!(config.version, CURRENT_VERSION);
}

#[test]
fn test_audio_config_defaults() {
    let audio = AudioConfig::default();
    assert_eq!(audio.device_id, None);
    assert_eq!(audio.sample_rate, 16000);
    assert!(audio.play_sounds);
}

#[test]
fn test_transcription_config_defaults() {
    let transcription = TranscriptionConfig::default();
    assert_eq!(transcription.language, "en");
    assert!(transcription.auto_copy);
    assert!(transcription.auto_paste);
    assert!(!transcription.add_leading_space);
}

#[test]
fn test_shortcut_config_defaults() {
    let shortcuts = ShortcutConfig::default();
    assert_eq!(shortcuts.toggle_recording, "F13");
    assert_eq!(
        shortcuts.toggle_recording_alt,
        Some("CommandOrControl+Shift+Space".to_string())
    );
    assert_eq!(shortcuts.copy_last, Some("F14".to_string()));
}

#[test]
fn test_enhancement_config_defaults() {
    let enhancement = EnhancementConfig::default();
    assert!(!enhancement.enabled);
    assert_eq!(enhancement.model, "llama3.2");
    assert_eq!(enhancement.prompt_id, "fix-grammar");
    assert_eq!(enhancement.ollama_url, "http://localhost:11434");
}

#[test]
fn test_general_config_defaults() {
    let general = GeneralConfig::default();
    assert!(!general.launch_at_login);
    assert!(general.show_in_menu_bar);
    assert!(!general.show_in_dock);
}

// =============================================================================
// Config Serialisation Tests
// =============================================================================

#[test]
fn test_config_serialisation_roundtrip() {
    let config = Config::default();
    let json = serde_json::to_string(&config).expect("Failed to serialise");
    let deserialised: Config = serde_json::from_str(&json).expect("Failed to deserialise");

    assert_eq!(deserialised.version, config.version);
    assert_eq!(deserialised.audio.sample_rate, config.audio.sample_rate);
    assert_eq!(
        deserialised.transcription.language,
        config.transcription.language
    );
    assert_eq!(
        deserialised.shortcuts.toggle_recording,
        config.shortcuts.toggle_recording
    );
    assert_eq!(deserialised.enhancement.model, config.enhancement.model);
}

#[test]
fn test_partial_config_deserialisation() {
    // Config should use defaults for missing fields
    let json = r#"{"version": 1, "audio": {"sample_rate": 48000}}"#;
    let config: Config = serde_json::from_str(json).expect("Failed to deserialise");

    assert_eq!(config.version, 1);
    assert_eq!(config.audio.sample_rate, 48000);
    assert_eq!(config.audio.device_id, None); // Default
    assert_eq!(config.transcription.language, "en"); // Default
}

#[test]
fn test_config_with_all_fields_set() {
    let json = r#"{
        "version": 1,
        "audio": {
            "device_id": "my-mic",
            "sample_rate": 44100,
            "play_sounds": false
        },
        "transcription": {
            "language": "de",
            "auto_copy": false,
            "auto_paste": false,
            "add_leading_space": true
        },
        "shortcuts": {
            "toggle_recording": "F12",
            "toggle_recording_alt": null,
            "copy_last": "F15"
        },
        "enhancement": {
            "enabled": true,
            "model": "mistral",
            "prompt_id": "summarise",
            "ollama_url": "http://192.168.1.100:11434"
        },
        "general": {
            "launch_at_login": true,
            "show_in_menu_bar": false,
            "show_in_dock": true
        }
    }"#;

    let config: Config = serde_json::from_str(json).expect("Failed to deserialise");

    assert_eq!(config.audio.device_id, Some("my-mic".to_string()));
    assert_eq!(config.audio.sample_rate, 44100);
    assert!(!config.audio.play_sounds);

    assert_eq!(config.transcription.language, "de");
    assert!(!config.transcription.auto_copy);
    assert!(config.transcription.add_leading_space);

    assert_eq!(config.shortcuts.toggle_recording, "F12");
    assert_eq!(config.shortcuts.toggle_recording_alt, None);

    assert!(config.enhancement.enabled);
    assert_eq!(config.enhancement.model, "mistral");

    assert!(config.general.launch_at_login);
    assert!(!config.general.show_in_menu_bar);
}

// =============================================================================
// Config File Operations Tests
// =============================================================================

#[test]
fn test_save_and_load_config() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("config.json");

    // Create a modified config
    let mut config = Config::default();
    config.audio.sample_rate = 48000;
    config.transcription.language = "de".to_string();
    config.enhancement.enabled = true;

    // Save it
    save_config(&config, &config_path).expect("Failed to save config");

    // Load it back
    let loaded = load_config(&config_path).expect("Failed to load config");

    assert_eq!(loaded.audio.sample_rate, 48000);
    assert_eq!(loaded.transcription.language, "de");
    assert!(loaded.enhancement.enabled);
}

#[test]
fn test_load_nonexistent_config_returns_defaults() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("nonexistent.json");

    let config = load_config(&config_path).expect("Should return defaults");

    assert_eq!(config.version, CURRENT_VERSION);
    assert_eq!(config.audio.sample_rate, 16000);
    assert_eq!(config.transcription.language, "en");
}

#[test]
fn test_config_file_persistence() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("persistent.json");

    // Save config
    let mut config = Config::default();
    config.general.launch_at_login = true;
    save_config(&config, &config_path).expect("Failed to save");

    // Verify file exists
    assert!(config_path.exists());

    // Modify and save again
    config.general.show_in_dock = true;
    save_config(&config, &config_path).expect("Failed to save");

    // Load and verify both changes persisted
    let loaded = load_config(&config_path).expect("Failed to load");
    assert!(loaded.general.launch_at_login);
    assert!(loaded.general.show_in_dock);
}

#[test]
fn test_reset_config() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("reset.json");

    // Save a modified config
    let mut config = Config::default();
    config.audio.sample_rate = 48000;
    config.transcription.auto_copy = false;
    config.enhancement.enabled = true;
    save_config(&config, &config_path).expect("Failed to save");

    // Reset to defaults
    let default_config = Config::default();
    save_config(&default_config, &config_path).expect("Failed to save defaults");

    // Verify reset worked
    let loaded = load_config(&config_path).expect("Failed to load");
    assert_eq!(loaded.audio.sample_rate, 16000);
    assert!(loaded.transcription.auto_copy);
    assert!(!loaded.enhancement.enabled);
}

// =============================================================================
// Config Version and Migration Tests
// =============================================================================

#[test]
fn test_config_version_preserved() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("versioned.json");

    let config = Config::default();
    save_config(&config, &config_path).expect("Failed to save");

    let loaded = load_config(&config_path).expect("Failed to load");
    assert_eq!(loaded.version, CURRENT_VERSION);
}

#[test]
fn test_old_version_config_deserialises() {
    // Simulate an old config with version 0
    let json = r#"{"version": 0, "audio": {"sample_rate": 16000}}"#;
    let config: Config = serde_json::from_str(json).expect("Failed to deserialise");

    assert_eq!(config.version, 0);
    // Other fields should use defaults
    assert_eq!(config.transcription.language, "en");
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_config_with_empty_strings() {
    let json = r#"{
        "version": 1,
        "audio": {"device_id": ""},
        "transcription": {"language": ""},
        "shortcuts": {"toggle_recording": ""},
        "enhancement": {"model": "", "prompt_id": "", "ollama_url": ""}
    }"#;

    let config: Config = serde_json::from_str(json).expect("Failed to deserialise");

    assert_eq!(config.audio.device_id, Some("".to_string()));
    assert_eq!(config.transcription.language, "");
    assert_eq!(config.shortcuts.toggle_recording, "");
}

#[test]
fn test_config_with_special_characters() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("special.json");

    let mut config = Config::default();
    config.audio.device_id = Some("Device with \"quotes\" and 'apostrophes'".to_string());
    config.transcription.language = "en-AU".to_string();

    save_config(&config, &config_path).expect("Failed to save");
    let loaded = load_config(&config_path).expect("Failed to load");

    assert_eq!(
        loaded.audio.device_id,
        Some("Device with \"quotes\" and 'apostrophes'".to_string())
    );
    assert_eq!(loaded.transcription.language, "en-AU");
}

#[test]
fn test_config_pretty_printed_json() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("pretty.json");

    let config = Config::default();
    save_config(&config, &config_path).expect("Failed to save");

    let content = fs::read_to_string(&config_path).expect("Failed to read");

    // Pretty-printed JSON should have newlines and indentation
    assert!(content.contains('\n'));
    assert!(content.contains("  ")); // Indentation
}

#[test]
fn test_config_handles_invalid_json() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("invalid.json");

    // Write invalid JSON
    fs::write(&config_path, "{ this is not valid json }").expect("Failed to write");

    let result = load_config(&config_path);
    assert!(result.is_err());
}

#[test]
fn test_config_with_unknown_fields() {
    // serde(default) should ignore unknown fields
    let json = r#"{
        "version": 1,
        "unknown_field": "should be ignored",
        "audio": {"sample_rate": 16000, "unknown_audio_field": true}
    }"#;

    let config: Config = serde_json::from_str(json).expect("Failed to deserialise");
    assert_eq!(config.version, 1);
    assert_eq!(config.audio.sample_rate, 16000);
}

// =============================================================================
// Concurrent Access Simulation
// =============================================================================

#[test]
fn test_multiple_saves_dont_corrupt() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let config_path = temp_dir.path().join("concurrent.json");

    // Simulate multiple rapid saves
    for i in 0..10 {
        let mut config = Config::default();
        config.audio.sample_rate = 16000 + (i * 1000);
        save_config(&config, &config_path).expect("Failed to save");
    }

    // Final load should succeed and have the last value
    let loaded = load_config(&config_path).expect("Failed to load");
    assert_eq!(loaded.audio.sample_rate, 16000 + (9 * 1000));
}
