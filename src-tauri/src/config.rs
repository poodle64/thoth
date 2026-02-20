//! Configuration management for Thoth
//!
//! Provides persistent settings storage with schema versioning and migrations.
//! Configuration is stored in `~/.thoth/config.json` and is accessible from
//! both the Rust backend and the Svelte frontend via IPC commands.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Current config schema version
const CURRENT_VERSION: u32 = 1;

/// Global config instance for caching
static CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Schema version for migrations
    pub version: u32,
    /// Audio recording settings
    pub audio: AudioConfig,
    /// Transcription settings
    pub transcription: TranscriptionConfig,
    /// Keyboard shortcut settings
    pub shortcuts: ShortcutConfig,
    /// AI enhancement settings
    pub enhancement: EnhancementConfig,
    /// General application settings
    pub general: GeneralConfig,
    /// Recorder window settings
    pub recorder: RecorderConfig,
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
            recorder: RecorderConfig::default(),
        }
    }
}

/// Audio recording configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Selected audio input device ID (None for system default)
    pub device_id: Option<String>,
    /// Sample rate in Hz (default: 16000 for transcription)
    pub sample_rate: u32,
    /// Whether to play audio feedback sounds
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

/// Transcription engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    /// Selected model ID (e.g., "ggml-large-v3-turbo", "parakeet-tdt-0.6b-v2-int8")
    /// If None, uses the recommended model from the manifest
    pub model_id: Option<String>,
    /// Transcription language code (e.g., "en", "auto")
    pub language: String,
    /// Whether to automatically copy transcription to clipboard
    pub auto_copy: bool,
    /// Whether to automatically paste transcription at cursor
    pub auto_paste: bool,
    /// Whether to add space before pasted text
    pub add_leading_space: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            model_id: None,
            language: "en".to_string(),
            auto_copy: false,
            auto_paste: true,
            add_leading_space: false,
        }
    }
}

/// Recording mode options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    /// Toggle mode: press to start, press again to stop
    #[default]
    Toggle,
    /// Push-to-talk mode: hold to record, release to stop
    PushToTalk,
}

/// Keyboard shortcut configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    /// Toggle recording shortcut (e.g., "F13")
    pub toggle_recording: String,
    /// Alternative toggle recording shortcut
    pub toggle_recording_alt: Option<String>,
    /// Copy last transcription shortcut
    pub copy_last: Option<String>,
    /// Recording mode: toggle or push-to-talk
    pub recording_mode: RecordingMode,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            toggle_recording: "F13".to_string(),
            toggle_recording_alt: Some("CommandOrControl+Shift+Space".to_string()),
            copy_last: Some("F14".to_string()),
            recording_mode: RecordingMode::default(),
        }
    }
}

/// AI enhancement configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhancementConfig {
    /// Whether AI enhancement is enabled
    pub enabled: bool,
    /// Ollama model to use for enhancement
    pub model: String,
    /// Selected prompt template ID
    pub prompt_id: String,
    /// Ollama server URL
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

/// General application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Launch application on system startup
    pub launch_at_login: bool,
    /// Show menu bar icon
    pub show_in_menu_bar: bool,
    /// Show dock icon (macOS)
    pub show_in_dock: bool,
    /// Automatically check for updates on launch
    pub check_for_updates: bool,
    /// Show the floating recording indicator during recording
    pub show_recording_indicator: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            show_in_menu_bar: true,
            show_in_dock: false,
            check_for_updates: true,
            show_recording_indicator: true,
        }
    }
}

/// Recorder window position options
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RecorderPosition {
    /// Position near the cursor when recording starts
    Cursor,
    /// Position near the tray icon
    TrayIcon,
    /// Top-left corner of the screen
    TopLeft,
    /// Top-right corner of the screen
    TopRight,
    /// Bottom-left corner of the screen
    BottomLeft,
    /// Bottom-right corner of the screen
    BottomRight,
    /// Centre of the screen
    Centre,
}

impl Default for RecorderPosition {
    fn default() -> Self {
        Self::TopRight
    }
}

/// Recorder window configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecorderConfig {
    /// Window position preference
    pub position: RecorderPosition,
    /// Horizontal offset from position anchor (in pixels)
    pub offset_x: i32,
    /// Vertical offset from position anchor (in pixels)
    pub offset_y: i32,
    /// Auto-hide delay in milliseconds after transcription completes (0 = no auto-hide)
    pub auto_hide_delay: u32,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            position: RecorderPosition::default(),
            offset_x: -20,
            offset_y: 20,
            auto_hide_delay: 3000,
        }
    }
}

/// Get the path to the config file (~/.thoth/config.json)
pub fn get_config_path() -> PathBuf {
    home_dir_or_fallback().join(".thoth").join("config.json")
}

/// Get the path to the config directory (~/.thoth)
fn get_config_dir() -> PathBuf {
    home_dir_or_fallback().join(".thoth")
}

/// Get the home directory, falling back to /tmp if unavailable
fn home_dir_or_fallback() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        tracing::error!("Could not determine home directory, using /tmp");
        PathBuf::from("/tmp")
    })
}

/// Ensure the config directory exists
fn ensure_config_dir() -> Result<(), String> {
    let dir = get_config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    Ok(())
}

/// Load configuration from disk
fn load_from_disk() -> Result<Config, String> {
    let path = get_config_path();

    if !path.exists() {
        tracing::info!("Config file not found, using defaults");
        return Ok(Config::default());
    }

    let contents =
        fs::read_to_string(&path).map_err(|e| format!("Failed to read config file: {}", e))?;

    let config: Config =
        serde_json::from_str(&contents).map_err(|e| format!("Failed to parse config: {}", e))?;

    // Run migrations if needed
    let migrated = migrate_config(config)?;

    Ok(migrated)
}

/// Save configuration to disk
fn save_to_disk(config: &Config) -> Result<(), String> {
    ensure_config_dir()?;

    let path = get_config_path();
    let contents = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialise config: {}", e))?;

    fs::write(&path, contents).map_err(|e| format!("Failed to write config file: {}", e))?;

    tracing::info!(
        "Config saved to disk: device_id={:?}",
        config.audio.device_id
    );
    Ok(())
}

/// Migrate configuration from older schema versions
fn migrate_config(mut config: Config) -> Result<Config, String> {
    let original_version = config.version;

    // Apply migrations sequentially
    while config.version < CURRENT_VERSION {
        config = apply_migration(config)?;
    }

    if config.version != original_version {
        tracing::info!(
            "Migrated config from version {} to {}",
            original_version,
            config.version
        );
        // Save the migrated config
        save_to_disk(&config)?;
    }

    Ok(config)
}

/// Apply a single migration step
fn apply_migration(config: Config) -> Result<Config, String> {
    match config.version {
        // Version 0 -> 1: Initial migration (add any new fields)
        0 => {
            let mut migrated = config;
            migrated.version = 1;
            // Future migrations would add field transformations here
            Ok(migrated)
        }
        v => Err(format!("Unknown config version: {}", v)),
    }
}

/// Get the global config instance
fn get_config_instance() -> &'static RwLock<Config> {
    CONFIG.get_or_init(|| {
        let config = load_from_disk().unwrap_or_else(|e| {
            tracing::error!("Failed to load config, using defaults: {}", e);
            Config::default()
        });
        tracing::info!(
            "Config loaded from disk: device_id={:?}",
            config.audio.device_id
        );
        RwLock::new(config)
    })
}

// --- IPC Commands ---

/// Get the current configuration
///
/// Returns the current configuration state. The config is cached in memory
/// and loaded from disk on first access.
#[tauri::command]
pub fn get_config() -> Result<Config, String> {
    let config = get_config_instance().read().clone();
    Ok(config)
}

/// Update the configuration
///
/// Replaces the current configuration with the provided config and persists
/// it to disk. The version field is automatically updated to the current schema.
#[tauri::command]
pub fn set_config(mut config: Config) -> Result<(), String> {
    // Ensure version is current
    config.version = CURRENT_VERSION;

    // Preserve device_id if the incoming config has None but the current config
    // has a device selected. This prevents other config saves (shortcuts, AI
    // settings, etc.) from accidentally clearing the user's device preference.
    // The dedicated set_audio_device command handles intentional device changes.
    //
    // Similarly, preserve prompt_id if the incoming value is the default but the
    // cached value differs. This prevents the frontend's generic config save from
    // overwriting a tray-initiated prompt change. The dedicated set_prompt_config
    // function handles intentional prompt changes.
    {
        let current = get_config_instance().read();
        if config.audio.device_id.is_none() && current.audio.device_id.is_some() {
            tracing::debug!(
                "Preserving device_id={:?} (incoming config had None)",
                current.audio.device_id
            );
            config.audio.device_id = current.audio.device_id.clone();
        }

        let default_prompt_id = EnhancementConfig::default().prompt_id;
        if config.enhancement.prompt_id == default_prompt_id
            && current.enhancement.prompt_id != default_prompt_id
        {
            tracing::debug!(
                "Preserving prompt_id={:?} (incoming config had default)",
                current.enhancement.prompt_id
            );
            config.enhancement.prompt_id = current.enhancement.prompt_id.clone();
        }
    }

    // Save to disk first
    save_to_disk(&config)?;

    // Update cached config
    let mut cached = get_config_instance().write();
    *cached = config;

    tracing::info!(
        "Configuration updated (device_id: {:?})",
        cached.audio.device_id
    );
    Ok(())
}

/// Set the audio device_id directly, bypassing set_config's preservation logic.
///
/// This is the only correct way to change device_id (including clearing it to
/// None for "System Default"). The preservation logic in `set_config` is designed
/// to protect against accidental clears from frontend config saves, but would
/// also block intentional clears if used for device changes.
pub fn set_audio_device_config(device_id: Option<String>) -> Result<(), String> {
    let mut cached = get_config_instance().write();
    cached.audio.device_id = device_id;
    save_to_disk(&cached)?;
    tracing::info!(
        "Audio device config updated (device_id: {:?})",
        cached.audio.device_id
    );
    Ok(())
}

/// Set the prompt_id directly, bypassing set_config's preservation logic.
///
/// This is the correct way to change prompt_id from the tray menu. The
/// preservation logic in `set_config` prevents the frontend's generic config
/// save from overwriting a tray-initiated prompt change.
pub fn set_prompt_config(prompt_id: String) -> Result<(), String> {
    let mut cached = get_config_instance().write();
    cached.enhancement.prompt_id = prompt_id;
    save_to_disk(&cached)?;
    tracing::info!(
        "Prompt config updated (prompt_id: {:?})",
        cached.enhancement.prompt_id
    );
    Ok(())
}

/// Reset configuration to defaults
///
/// Resets all settings to their default values and persists to disk.
#[tauri::command]
pub fn reset_config() -> Result<Config, String> {
    let default_config = Config::default();

    // Save to disk
    save_to_disk(&default_config)?;

    // Update cached config
    let mut cached = get_config_instance().write();
    *cached = default_config.clone();

    tracing::info!("Configuration reset to defaults");
    Ok(default_config)
}

/// Get the configuration file path
///
/// Returns the path to the config file for debugging or user information.
#[tauri::command]
pub fn get_config_path_cmd() -> String {
    get_config_path().to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_current_version() {
        let config = Config::default();
        assert_eq!(config.version, CURRENT_VERSION);
    }

    #[test]
    fn test_config_serialisation_roundtrip() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialised: Config = serde_json::from_str(&json).unwrap();

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
        assert!(!transcription.auto_copy);
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
        assert_eq!(shortcuts.recording_mode, RecordingMode::Toggle);
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

    #[test]
    fn test_recorder_config_defaults() {
        let recorder = RecorderConfig::default();
        assert_eq!(recorder.position, RecorderPosition::TopRight);
        assert_eq!(recorder.offset_x, -20);
        assert_eq!(recorder.offset_y, 20);
    }

    #[test]
    fn test_recorder_position_serialisation() {
        let positions = vec![
            (RecorderPosition::Cursor, "\"cursor\""),
            (RecorderPosition::TrayIcon, "\"tray-icon\""),
            (RecorderPosition::TopLeft, "\"top-left\""),
            (RecorderPosition::TopRight, "\"top-right\""),
            (RecorderPosition::BottomLeft, "\"bottom-left\""),
            (RecorderPosition::BottomRight, "\"bottom-right\""),
            (RecorderPosition::Centre, "\"centre\""),
        ];

        for (position, expected_json) in positions {
            let json = serde_json::to_string(&position).unwrap();
            assert_eq!(json, expected_json);

            let parsed: RecorderPosition = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, position);
        }
    }

    #[test]
    fn test_partial_config_deserialisation() {
        // Config should use defaults for missing fields
        let json = r#"{"version": 1, "audio": {"sample_rate": 48000}}"#;
        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.version, 1);
        assert_eq!(config.audio.sample_rate, 48000);
        assert_eq!(config.audio.device_id, None); // Default
        assert_eq!(config.transcription.language, "en"); // Default
    }

    #[test]
    fn test_migration_from_version_0() {
        let old_config = Config {
            version: 0,
            ..Default::default()
        };

        let migrated = migrate_config(old_config).unwrap();
        assert_eq!(migrated.version, CURRENT_VERSION);
    }

    // =========================================================================
    // Additional config tests
    // =========================================================================

    #[test]
    fn test_recording_mode_serialisation() {
        assert_eq!(
            serde_json::to_string(&RecordingMode::Toggle).unwrap(),
            "\"toggle\""
        );
        assert_eq!(
            serde_json::to_string(&RecordingMode::PushToTalk).unwrap(),
            "\"push_to_talk\""
        );
    }

    #[test]
    fn test_recording_mode_deserialisation() {
        assert_eq!(
            serde_json::from_str::<RecordingMode>("\"toggle\"").unwrap(),
            RecordingMode::Toggle
        );
        assert_eq!(
            serde_json::from_str::<RecordingMode>("\"push_to_talk\"").unwrap(),
            RecordingMode::PushToTalk
        );
    }

    #[test]
    fn test_config_path_format() {
        let path = get_config_path();
        let path_str = path.to_string_lossy();

        // Should be in .thoth directory
        assert!(path_str.contains(".thoth"));
        // Should be named config.json
        assert!(path_str.ends_with("config.json"));
    }

    #[test]
    fn test_full_config_serialisation_roundtrip() {
        let config = Config {
            version: CURRENT_VERSION,
            audio: AudioConfig {
                device_id: Some("test-device".to_string()),
                sample_rate: 44100,
                play_sounds: false,
            },
            transcription: TranscriptionConfig {
                model_id: Some("test-model".to_string()),
                language: "de".to_string(),
                auto_copy: false,
                auto_paste: false,
                add_leading_space: true,
            },
            shortcuts: ShortcutConfig {
                toggle_recording: "F12".to_string(),
                toggle_recording_alt: None,
                copy_last: None,
                recording_mode: RecordingMode::PushToTalk,
            },
            enhancement: EnhancementConfig {
                enabled: true,
                model: "mistral".to_string(),
                prompt_id: "custom".to_string(),
                ollama_url: "http://custom:8080".to_string(),
            },
            general: GeneralConfig {
                launch_at_login: true,
                show_in_menu_bar: false,
                show_in_dock: true,
                check_for_updates: true,
                show_recording_indicator: true,
            },
            recorder: RecorderConfig {
                position: RecorderPosition::Centre,
                offset_x: 10,
                offset_y: 20,
                auto_hide_delay: 5000,
            },
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();

        // Verify all fields were preserved
        assert_eq!(restored.audio.device_id, Some("test-device".to_string()));
        assert_eq!(restored.audio.sample_rate, 44100);
        assert!(!restored.audio.play_sounds);

        assert_eq!(restored.transcription.language, "de");
        assert!(!restored.transcription.auto_copy);
        assert!(restored.transcription.add_leading_space);

        assert_eq!(restored.shortcuts.toggle_recording, "F12");
        assert!(restored.shortcuts.toggle_recording_alt.is_none());
        assert_eq!(restored.shortcuts.recording_mode, RecordingMode::PushToTalk);

        assert!(restored.enhancement.enabled);
        assert_eq!(restored.enhancement.model, "mistral");

        assert!(restored.general.launch_at_login);
        assert!(!restored.general.show_in_menu_bar);

        assert_eq!(restored.recorder.position, RecorderPosition::Centre);
    }

    #[test]
    fn test_config_unknown_fields_ignored() {
        // JSON with extra unknown fields should still parse
        let json = r#"{
            "version": 1,
            "unknown_field": "should be ignored",
            "audio": {"sample_rate": 16000, "extra": true}
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.audio.sample_rate, 16000);
    }

    #[test]
    fn test_apply_migration_unknown_version() {
        let future_config = Config {
            version: 999,
            ..Default::default()
        };

        let result = apply_migration(future_config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown config version"));
    }

    #[test]
    fn test_audio_config_custom_values() {
        let audio = AudioConfig {
            device_id: Some("custom-mic".to_string()),
            sample_rate: 48000,
            play_sounds: false,
        };

        assert_eq!(audio.device_id, Some("custom-mic".to_string()));
        assert_eq!(audio.sample_rate, 48000);
        assert!(!audio.play_sounds);
    }

    #[test]
    fn test_enhancement_config_custom_ollama_url() {
        let enhancement = EnhancementConfig {
            enabled: true,
            model: "custom-model".to_string(),
            prompt_id: "summarise".to_string(),
            ollama_url: "http://192.168.1.100:11434".to_string(),
        };

        assert!(enhancement.enabled);
        assert_eq!(enhancement.ollama_url, "http://192.168.1.100:11434");
    }
}
