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

use crate::enhancement;

/// Current config schema version
const CURRENT_VERSION: u32 = 1;

/// Global config instance for caching
static CONFIG: OnceLock<RwLock<Config>> = OnceLock::new();

/// Integrations configuration (Local Control API, MCP server)
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IntegrationsConfig {
    /// Whether the Local Control API HTTP server is enabled
    #[serde(default)]
    pub api_enabled: bool,
    /// Port for the Local Control API (default 8765)
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    /// Whether the MCP server is enabled
    #[serde(default)]
    pub mcp_enabled: bool,
    /// Bearer token for authenticating API requests (None = not yet generated)
    #[serde(default)]
    pub api_token: Option<String>,
}

fn default_api_port() -> u16 {
    8765
}

impl std::fmt::Debug for IntegrationsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntegrationsConfig")
            .field("api_enabled", &self.api_enabled)
            .field("api_port", &self.api_port)
            .field("mcp_enabled", &self.mcp_enabled)
            .field(
                "api_token",
                &self.api_token.as_ref().map(|_| "***redacted***"),
            )
            .finish()
    }
}

impl Default for IntegrationsConfig {
    fn default() -> Self {
        Self {
            // Control API and MCP server default ON. They bind 127.0.0.1 only and
            // require the bearer token, so they are not network-exposed; defaulting
            // on means MCP-capable assistants work out of the box. The token is
            // auto-generated on first run when missing (see lib.rs startup).
            api_enabled: true,
            api_port: default_api_port(),
            mcp_enabled: true,
            api_token: None,
        }
    }
}

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
    /// Integrations settings (Local Control API, MCP)
    pub integrations: IntegrationsConfig,
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
            integrations: IntegrationsConfig::default(),
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
    /// Keep the cpal input stream open between recordings ("warm stream").
    ///
    /// When true (default), the device is opened once and kept playing with an
    /// armed flag gating writes to the recording buffer. Start latency drops
    /// from ~150ms to near-zero. The stream is torn down after 45s of inactivity.
    /// When false, the device is opened and closed on every recording (original
    /// behaviour); the mic indicator only shows levels during active recording.
    #[serde(default = "default_true")]
    pub warm_stream: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            sample_rate: 16000,
            play_sounds: true,
            warm_stream: true,
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
    /// Whether to remove hesitation sounds (um, uh, er, ah) from transcription
    #[serde(default = "default_true")]
    pub remove_fillers: bool,
    /// Whether to convert US spellings to Australian/British equivalents.
    /// Defaults on — the operator dictates Australian English.
    #[serde(default = "default_true")]
    pub australian_spelling: bool,
    /// Whether to convert spoken number words to digits.
    /// Defaults OFF: rule-based ITN is inherently ambiguous (lone "one" as a
    /// pronoun, counting sequences vs sums), so it stays opt-in for when the
    /// user is dictating numeric content rather than prose.
    #[serde(default)]
    pub spoken_numbers_to_digits: bool,
    /// Whether to collapse runs of whitespace and trim leading/trailing spaces
    #[serde(default = "default_true")]
    pub normalise_whitespace: bool,
    /// Whether to fix spacing around punctuation marks
    #[serde(default = "default_true")]
    pub cleanup_punctuation: bool,
    /// Whether to capitalise the first word of each sentence
    #[serde(default)]
    pub sentence_case: bool,
}

fn default_true() -> bool {
    true
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            model_id: None,
            language: "en".to_string(),
            auto_copy: false,
            auto_paste: true,
            add_leading_space: false,
            remove_fillers: true,
            australian_spelling: true,
            spoken_numbers_to_digits: false,
            normalise_whitespace: true,
            cleanup_punctuation: true,
            sentence_case: false,
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
    /// Toggle AI enhancement on/off shortcut (unbound by default)
    pub toggle_enhancement: Option<String>,
    /// Recording mode: toggle or push-to-talk
    pub recording_mode: RecordingMode,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            toggle_recording: "F13".to_string(),
            toggle_recording_alt: Some("ShiftRight".to_string()),
            copy_last: Some("F14".to_string()),
            toggle_enhancement: None,
            recording_mode: RecordingMode::default(),
        }
    }
}

/// AI enhancement configuration
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhancementConfig {
    /// Whether AI enhancement is enabled
    pub enabled: bool,
    /// Model name (used by whichever backend is active)
    pub model: String,
    /// Selected prompt template ID
    pub prompt_id: String,
    /// Ollama server URL (unchanged from pre-existing config)
    pub ollama_url: String,
    /// Active backend: "ollama" (default) or "openai_compat"
    #[serde(default = "default_backend")]
    pub backend: String,
    /// OpenAI-compatible server base URL
    #[serde(default = "default_openai_compat_url")]
    pub openai_compat_url: String,
    /// Optional API key for the OpenAI-compatible endpoint
    #[serde(default)]
    pub api_key: Option<String>,
}

impl std::fmt::Debug for EnhancementConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnhancementConfig")
            .field("enabled", &self.enabled)
            .field("model", &self.model)
            .field("prompt_id", &self.prompt_id)
            .field("ollama_url", &self.ollama_url)
            .field("backend", &self.backend)
            .field("openai_compat_url", &self.openai_compat_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "***redacted***"))
            .finish()
    }
}

fn default_backend() -> String {
    "ollama".to_string()
}

fn default_openai_compat_url() -> String {
    "http://localhost:1234".to_string()
}

impl Default for EnhancementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "llama3.2".to_string(),
            prompt_id: "fix-grammar".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            backend: default_backend(),
            openai_compat_url: default_openai_compat_url(),
            api_key: None,
        }
    }
}

/// Recording indicator visual style
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IndicatorStyle {
    /// Small dot/square that follows the mouse cursor (default)
    #[default]
    CursorDot,
    /// Small stationary window at a fixed screen position
    FixedFloat,
    /// Elongated horizontal bar with waveform visualisation
    Pill,
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
    /// Visual style for the recording indicator
    pub indicator_style: IndicatorStyle,
    /// App version recorded on the most recent run.
    ///
    /// Used to detect that an update has been applied: when this differs from
    /// the running binary's version, macOS TCC permission grants are likely
    /// stale (TCC keys grants to the code-signing identity, which changes on
    /// each build), so the app resets them once and prompts a re-grant.
    /// `None` on a genuinely fresh install — no reset is triggered then.
    #[serde(default)]
    pub last_run_version: Option<String>,
    /// Show system window decorations (title bar). Linux only.
    #[serde(default = "default_true")]
    pub window_decorations: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            show_in_menu_bar: true,
            show_in_dock: false,
            check_for_updates: true,
            show_recording_indicator: true,
            indicator_style: IndicatorStyle::default(),
            last_run_version: None,
            window_decorations: true,
        }
    }
}

/// Recorder window position options
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RecorderPosition {
    /// Position near the cursor when recording starts
    Cursor,
    /// Position near the tray icon
    TrayIcon,
    /// Top-left corner of the screen
    TopLeft,
    /// Top-right corner of the screen
    #[default]
    TopRight,
    /// Bottom-left corner of the screen
    BottomLeft,
    /// Bottom-right corner of the screen
    BottomRight,
    /// Centre of the screen
    Centre,
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

    fs::write(&path, &contents).map_err(|e| format!("Failed to write config file: {}", e))?;

    // Restrict config file permissions to owner-only (rw-------) because it
    // may contain an API key in plaintext.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        if let Err(e) = fs::set_permissions(&path, permissions) {
            tracing::warn!("Failed to set config file permissions to 0o600: {}", e);
        }
    }

    tracing::info!(
        "Config saved to disk: device_id={:?}, toggle_recording_alt={:?}",
        config.audio.device_id,
        config.shortcuts.toggle_recording_alt
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

/// Apply the enhancement config to the global backend singleton.
///
/// Called on startup and after every `set_config` that touches enhancement
/// settings, so the in-process backend always reflects persisted config.
pub fn apply_enhancement_backend(enh: &EnhancementConfig) {
    enhancement::configure_backend(
        &enh.backend,
        &enh.ollama_url,
        &enh.openai_compat_url,
        enh.api_key.as_deref(),
    );
}

/// Get the global config instance
fn get_config_instance() -> &'static RwLock<Config> {
    CONFIG.get_or_init(|| {
        let config = load_from_disk().unwrap_or_else(|e| {
            tracing::error!("Failed to load config, using defaults: {}", e);
            Config::default()
        });
        tracing::info!(
            "Config loaded from disk: device_id={:?}, toggle_recording_alt={:?}",
            config.audio.device_id,
            config.shortcuts.toggle_recording_alt
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

        if config.transcription.model_id.is_none() && current.transcription.model_id.is_some() {
            tracing::debug!(
                "Preserving model_id={:?} (incoming config had None)",
                current.transcription.model_id
            );
            config.transcription.model_id = current.transcription.model_id.clone();
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

        // Preserve toggle_recording_alt if the incoming config has the default but
        // the cached config has a user-chosen value (e.g. "ShiftRight"). This prevents
        // unrelated config saves from overwriting the user's shortcut preference.
        let default_shortcuts = ShortcutConfig::default();
        if config.shortcuts.toggle_recording_alt == default_shortcuts.toggle_recording_alt
            && current.shortcuts.toggle_recording_alt != default_shortcuts.toggle_recording_alt
        {
            tracing::debug!(
                "Preserving toggle_recording_alt={:?} (incoming config had default)",
                current.shortcuts.toggle_recording_alt
            );
            config.shortcuts.toggle_recording_alt = current.shortcuts.toggle_recording_alt.clone();
        }

        // Preserve toggle_enhancement if incoming is None but cached has a user-set value.
        if config.shortcuts.toggle_enhancement.is_none()
            && current.shortcuts.toggle_enhancement.is_some()
        {
            tracing::debug!(
                "Preserving toggle_enhancement={:?} (incoming config had None)",
                current.shortcuts.toggle_enhancement
            );
            config.shortcuts.toggle_enhancement = current.shortcuts.toggle_enhancement.clone();
        }

        // Preserve copy_last if incoming is None but cached has a user-set value.
        if config.shortcuts.copy_last.is_none() && current.shortcuts.copy_last.is_some() {
            tracing::debug!(
                "Preserving copy_last={:?} (incoming config had None)",
                current.shortcuts.copy_last
            );
            config.shortcuts.copy_last = current.shortcuts.copy_last.clone();
        }

        // Preserve api_token if the incoming config has None but the cached config has
        // a token. The frontend settings panel does not echo the token back (it only
        // shows a masked representation), so a generic config save must not wipe it.
        // Use rotate_api_token or set_api_enabled (which generates one) for intentional
        // token changes.
        if config.integrations.api_token.is_none() && current.integrations.api_token.is_some() {
            tracing::debug!("Preserving api_token (incoming config had None)");
            config.integrations.api_token = current.integrations.api_token.clone();
        }
    }

    // Save to disk first
    save_to_disk(&config)?;

    // Update cached config
    {
        let mut cached = get_config_instance().write();
        *cached = config.clone();
        tracing::info!(
            "Configuration updated (device_id: {:?}, toggle_recording_alt: {:?})",
            cached.audio.device_id,
            cached.shortcuts.toggle_recording_alt
        );
    }

    // Reconfigure the enhancement backend to reflect any provider changes.
    apply_enhancement_backend(&config.enhancement);

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

/// Set enhancement enabled directly, bypassing set_config's preservation logic.
///
/// This is the correct way to toggle enhancement from the shortcut handler. The
/// preservation logic in `set_config` has a prompt_id guard that would interfere
/// with a full-config round-trip; this bypass touches only the `enabled` flag.
pub fn set_enhancement_enabled(enabled: bool) -> Result<(), String> {
    let mut cached = get_config_instance().write();
    cached.enhancement.enabled = enabled;
    save_to_disk(&cached)?;
    tracing::info!("Enhancement enabled updated to: {}", enabled);
    Ok(())
}

/// Set shortcut config directly, bypassing set_config's preservation logic.
///
/// Used by the Settings UI when intentionally changing shortcuts. The
/// preservation logic in `set_config` prevents unrelated config saves from
/// overwriting shortcuts, but would also block intentional changes (e.g.
/// resetting a shortcut back to its default value).
#[tauri::command]
pub fn set_shortcut_config(shortcuts: ShortcutConfig) -> Result<(), String> {
    let mut cached = get_config_instance().write();
    cached.shortcuts = shortcuts;
    save_to_disk(&cached)?;
    tracing::info!(
        "Shortcut config updated directly (toggle_recording_alt: {:?})",
        cached.shortcuts.toggle_recording_alt
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
            Some("ShiftRight".to_string())
        );
        assert_eq!(shortcuts.copy_last, Some("F14".to_string()));
        assert_eq!(shortcuts.toggle_enhancement, None);
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
    }

    #[test]
    fn test_recording_mode_deserialisation() {
        assert_eq!(
            serde_json::from_str::<RecordingMode>("\"toggle\"").unwrap(),
            RecordingMode::Toggle
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
                warm_stream: true,
            },
            transcription: TranscriptionConfig {
                model_id: Some("test-model".to_string()),
                language: "de".to_string(),
                auto_copy: false,
                auto_paste: true,
                add_leading_space: true,
                remove_fillers: false,
                australian_spelling: false,
                spoken_numbers_to_digits: false,
                normalise_whitespace: true,
                cleanup_punctuation: true,
                sentence_case: false,
            },
            shortcuts: ShortcutConfig {
                toggle_recording: "F12".to_string(),
                toggle_recording_alt: None,
                copy_last: None,
                toggle_enhancement: None,
                recording_mode: RecordingMode::Toggle,
            },
            enhancement: EnhancementConfig {
                enabled: true,
                model: "mistral".to_string(),
                prompt_id: "custom".to_string(),
                ollama_url: "http://custom:8080".to_string(),
                backend: "openai_compat".to_string(),
                openai_compat_url: "http://localhost:1234".to_string(),
                api_key: Some("sk-test".to_string()),
            },
            general: GeneralConfig {
                launch_at_login: true,
                show_in_menu_bar: false,
                show_in_dock: true,
                check_for_updates: true,
                show_recording_indicator: true,
                indicator_style: IndicatorStyle::CursorDot,
                last_run_version: None,
                window_decorations: true,
            },
            recorder: RecorderConfig {
                position: RecorderPosition::Centre,
                offset_x: 10,
                offset_y: 20,
                auto_hide_delay: 5000,
            },
            integrations: IntegrationsConfig::default(),
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
        assert_eq!(restored.shortcuts.recording_mode, RecordingMode::Toggle);

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
            warm_stream: false,
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
            ..Default::default()
        };

        assert!(enhancement.enabled);
        assert_eq!(enhancement.ollama_url, "http://192.168.1.100:11434");
    }

    // =========================================================================
    // OpenAI-compat provider field tests
    // =========================================================================

    #[test]
    fn test_enhancement_config_new_fields_roundtrip() {
        let enh = EnhancementConfig {
            enabled: true,
            model: "mistral".to_string(),
            prompt_id: "fix-grammar".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            backend: "openai_compat".to_string(),
            openai_compat_url: "http://localhost:1234".to_string(),
            api_key: Some("test-key".to_string()),
        };

        let json = serde_json::to_string(&enh).unwrap();
        let restored: EnhancementConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.backend, "openai_compat");
        assert_eq!(restored.openai_compat_url, "http://localhost:1234");
        assert_eq!(restored.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_enhancement_config_new_fields_snake_case_deserialise() {
        // The JSON uses snake_case (as serialised by the Rust backend; no camelCase mapping)
        let json = r#"{
            "enabled": false,
            "model": "llama3.2",
            "prompt_id": "fix-grammar",
            "ollama_url": "http://localhost:11434",
            "backend": "openai_compat",
            "openai_compat_url": "http://lm-studio:1234",
            "api_key": "sk-test"
        }"#;

        let enh: EnhancementConfig = serde_json::from_str(json).unwrap();
        assert_eq!(enh.backend, "openai_compat");
        assert_eq!(enh.openai_compat_url, "http://lm-studio:1234");
        assert_eq!(enh.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn test_old_config_without_new_fields_uses_defaults() {
        // A config JSON that predates the new fields should parse cleanly,
        // with the new fields taking their defaults.
        let json = r#"{
            "version": 1,
            "enhancement": {
                "enabled": false,
                "model": "llama3.2",
                "prompt_id": "fix-grammar",
                "ollama_url": "http://localhost:11434"
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.enhancement.backend, "ollama");
        assert_eq!(
            config.enhancement.openai_compat_url,
            "http://localhost:1234"
        );
        assert_eq!(config.enhancement.api_key, None);
        // Old field preserved
        assert_eq!(config.enhancement.ollama_url, "http://localhost:11434");
    }

    #[test]
    fn test_enhancement_config_api_key_cleared_when_null() {
        // If the user clears the API key, the clear must take effect (not preserved)
        let json = r#"{
            "enabled": false,
            "model": "llama3.2",
            "prompt_id": "fix-grammar",
            "ollama_url": "http://localhost:11434",
            "api_key": null
        }"#;

        let enh: EnhancementConfig = serde_json::from_str(json).unwrap();
        assert_eq!(enh.api_key, None);
    }
}
