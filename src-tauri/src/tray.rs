//! System tray setup and event handling
//!
//! Provides a dynamic system tray with recording state awareness and quick actions:
//! - Start/Stop Recording toggle
//! - Copy Last Transcription
//! - History window
//! - Settings window
//! - Quit

use parking_lot::RwLock;
use std::sync::OnceLock;
use tauri::{
    image::Image,
    menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

use crate::audio;
use crate::config;
use crate::database;
use crate::enhancement;
use crate::pipeline::PipelineState;
use crate::platform;
use crate::transcription;

// =============================================================================
// Tray State Management
// =============================================================================

/// Global tray state
static TRAY_STATE: OnceLock<RwLock<TrayState>> = OnceLock::new();

/// Current state of the tray
#[derive(Debug, Clone, Default)]
struct TrayState {
    /// Whether recording is in progress
    is_recording: bool,
    /// Last transcription text (truncated for display)
    last_transcription: Option<String>,
    /// Current pipeline state
    pipeline_state: PipelineState,
}


/// Get the global tray state instance
fn get_tray_state() -> &'static RwLock<TrayState> {
    TRAY_STATE.get_or_init(|| RwLock::new(TrayState::default()))
}

/// Menu item IDs
mod menu_ids {
    pub const STATUS: &str = "status";
    pub const TOGGLE_RECORDING: &str = "toggle_recording";
    pub const COPY_LAST: &str = "copy_last";
    pub const TRANSCRIBE: &str = "transcribe";
    pub const HISTORY: &str = "history";
    pub const SETTINGS: &str = "settings";
    pub const QUIT: &str = "quit";
    /// Prefix for audio device menu items (double-colon avoids ambiguity
    /// with cpal DeviceId format which contains single colons)
    pub const INPUT_SOURCE_PREFIX: &str = "audio_device::";
    pub const INPUT_SOURCE_DEFAULT: &str = "audio_device::__default__";
    /// Prefix for prompt template menu items
    pub const PROMPT_PREFIX: &str = "prompt::";
    /// Toggle AI enhancement on/off
    pub const AI_ENHANCEMENT_TOGGLE: &str = "ai_enhancement_toggle";
}

// =============================================================================
// Tray Setup
// =============================================================================

/// Set up the system tray with menu
pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Load the most recent transcription from the database (if any)
    let last_transcription = database::transcription::list_transcriptions(Some(1), Some(0))
        .ok()
        .and_then(|v| v.into_iter().next())
        .map(|t| t.text);

    // Store it in tray state so rebuild_tray_menu picks it up later
    if let Some(ref text) = last_transcription {
        let mut state = get_tray_state().write();
        state.last_transcription = Some(text.clone());
    }

    // Enumerate audio devices and read config
    let devices = audio::device::list_input_devices();
    let cfg = config::get_config().ok();
    let selected_device_id = cfg.as_ref().and_then(|c| c.audio.device_id.clone());
    let enhancement_enabled = cfg.as_ref().is_some_and(|c| c.enhancement.enabled);
    let active_prompt_id = cfg
        .as_ref()
        .map(|c| c.enhancement.prompt_id.clone())
        .unwrap_or_else(|| "fix-grammar".to_string());

    // Build initial menu
    let menu = build_tray_menu(
        app,
        false,
        last_transcription.as_deref(),
        &devices,
        selected_device_id.as_deref(),
        enhancement_enabled,
        &active_prompt_id,
    )?;

    // Create tray icon
    let icon = create_idle_icon();

    // Get shortcut hint for tooltip
    let shortcut_hint = get_shortcut_hint();

    // Build tray (template icon = macOS auto-tints for light/dark mode)
    let _tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .tooltip(format!("Thoth - Voice Transcription\n{}", shortcut_hint))
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, &event.id().0);
        })
        .on_tray_icon_event(|_tray, event| {
            handle_tray_event(event);
        })
        .build(app)?;

    tracing::info!("System tray initialised");

    Ok(())
}

/// Build the tray menu with current state
fn build_tray_menu(
    app: &impl Manager<tauri::Wry>,
    is_recording: bool,
    last_transcription: Option<&str>,
    devices: &[audio::device::AudioDevice],
    selected_device_id: Option<&str>,
    enhancement_enabled: bool,
    active_prompt_id: &str,
) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    // Status item (non-interactive, coloured dot as visual indicator)
    let status_text = if is_recording {
        "🔴 Recording..."
    } else {
        // Check for issues that would prevent recording
        let mic_ok = platform::check_microphone_permission() == "granted";
        let accessibility_ok = platform::check_accessibility();
        let model_downloaded = transcription::download::check_model_downloaded(None);

        if !mic_ok {
            "🟡 Microphone Permission Required"
        } else if !accessibility_ok {
            "🟡 Accessibility Permission Required"
        } else if !model_downloaded {
            "🟡 No Model Downloaded"
        } else {
            "🟢 Ready"
        }
    };
    let status = MenuItemBuilder::with_id(menu_ids::STATUS, status_text)
        .enabled(false)
        .build(app)?;

    let separator1 = PredefinedMenuItem::separator(app)?;

    // Input Source submenu with checkable device items
    let input_source_submenu = build_input_source_submenu(app, devices, selected_device_id)?;

    // AI Enhancement submenu (always visible)
    let ai_submenu = build_ai_submenu(app, enhancement_enabled, active_prompt_id)?;

    let separator_input = PredefinedMenuItem::separator(app)?;

    // Toggle recording item
    let recording_text = if is_recording {
        "Stop Recording"
    } else {
        "Start Recording"
    };
    let toggle_recording =
        MenuItemBuilder::with_id(menu_ids::TOGGLE_RECORDING, recording_text).build(app)?;

    let separator2 = PredefinedMenuItem::separator(app)?;

    // Copy last transcription
    let copy_last = MenuItemBuilder::with_id(menu_ids::COPY_LAST, "Copy Last Transcription")
        .enabled(last_transcription.is_some())
        .build(app)?;

    // Transcribe
    let transcribe =
        MenuItemBuilder::with_id(menu_ids::TRANSCRIBE, "Transcribe...").build(app)?;

    // History
    let history = MenuItemBuilder::with_id(menu_ids::HISTORY, "History...").build(app)?;

    // Settings
    let settings = MenuItemBuilder::with_id(menu_ids::SETTINGS, "Settings...")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;

    let separator3 = PredefinedMenuItem::separator(app)?;

    // Quit
    let quit = MenuItemBuilder::with_id(menu_ids::QUIT, "Quit Thoth")
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;

    // Build menu
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &separator1,
            &input_source_submenu,
            &ai_submenu,
            &separator_input,
            &toggle_recording,
            &separator2,
            &copy_last,
            &transcribe,
            &history,
            &settings,
            &separator3,
            &quit,
        ],
    )?;

    Ok(menu)
}

/// Tick prefix for the selected device in the submenu
const SELECTED_PREFIX: &str = "✓ ";
/// Padding to align unselected items with the tick
const UNSELECTED_PREFIX: &str = "   ";

/// Build the "Input Source" submenu listing audio input devices.
///
/// Uses regular `MenuItem`s with a text bullet prefix instead of `CheckMenuItem`
/// to avoid macOS auto-toggling checkmarks on click (which causes two items to
/// appear checked until the menu is rebuilt).
fn build_input_source_submenu(
    app: &impl Manager<tauri::Wry>,
    devices: &[audio::device::AudioDevice],
    selected_device_id: Option<&str>,
) -> Result<tauri::menu::Submenu<tauri::Wry>, Box<dyn std::error::Error>> {
    let is_default_selected = selected_device_id.is_none();

    // "System Default" label includes the actual default device name
    let default_device_name = devices
        .iter()
        .find(|d| d.is_default)
        .map(|d| d.name.as_str())
        .unwrap_or("Unknown");
    let prefix = if is_default_selected { SELECTED_PREFIX } else { UNSELECTED_PREFIX };
    let default_label = format!("{}System Default ({})", prefix, default_device_name);

    let default_item = MenuItemBuilder::with_id(
        menu_ids::INPUT_SOURCE_DEFAULT,
        &default_label,
    )
    .build(app)?;

    let mut submenu = SubmenuBuilder::new(app, "Input Source").item(&default_item);

    if !devices.is_empty() {
        submenu = submenu.separator();
    }

    // Build items for each device
    let device_items: Vec<tauri::menu::MenuItem<tauri::Wry>> = devices
        .iter()
        .map(|device| {
            let menu_id = format!("{}{}", menu_ids::INPUT_SOURCE_PREFIX, device.id);
            let is_selected = selected_device_id == Some(device.id.as_str());
            let prefix = if is_selected { SELECTED_PREFIX } else { UNSELECTED_PREFIX };
            let suffix = if device.is_default { " (Default)" } else { "" };
            let label = format!("{}{}{}", prefix, device.name, suffix);
            MenuItemBuilder::with_id(menu_id, &label).build(app)
        })
        .collect::<Result<Vec<_>, _>>()?;

    for item in &device_items {
        submenu = submenu.item(item);
    }

    Ok(submenu.build()?)
}

/// Build the "AI Enhancement" submenu with enable toggle, model info, and prompt selection.
///
/// Always visible in the tray. Layout:
/// - Enabled/Disabled toggle
/// - Model: <name> (disabled info item)
/// - separator
/// - Prompt list (selectable when enabled, tick on active)
fn build_ai_submenu(
    app: &impl Manager<tauri::Wry>,
    enhancement_enabled: bool,
    active_prompt_id: &str,
) -> Result<tauri::menu::Submenu<tauri::Wry>, Box<dyn std::error::Error>> {
    let toggle_label = if enhancement_enabled {
        "Enabled"
    } else {
        "Disabled"
    };
    let toggle_item =
        MenuItemBuilder::with_id(menu_ids::AI_ENHANCEMENT_TOGGLE, toggle_label).build(app)?;

    // Show configured model (read from config cache)
    let model_name = config::get_config()
        .map(|c| c.enhancement.model.clone())
        .unwrap_or_default();
    let model_label = if model_name.is_empty() {
        "Model: Not Set".to_string()
    } else {
        format!("Model: {}", model_name)
    };
    let model_item = MenuItemBuilder::with_id("ai_model_info", &model_label)
        .enabled(false)
        .build(app)?;

    let mut submenu = SubmenuBuilder::new(app, "AI Enhancement")
        .item(&toggle_item)
        .item(&model_item)
        .separator();

    let prompts = enhancement::get_all_prompts();

    // If the active prompt doesn't exist, fall back to fix-grammar and update config
    let active_id = if prompts.iter().any(|p| p.id == active_prompt_id) {
        active_prompt_id.to_string()
    } else {
        let fallback = "fix-grammar".to_string();
        let _ = config::set_prompt_config(fallback.clone());
        fallback
    };

    for prompt in &prompts {
        let menu_id = format!("{}{}", menu_ids::PROMPT_PREFIX, prompt.id);
        let is_selected = enhancement_enabled && prompt.id == active_id;
        let prefix = if is_selected {
            SELECTED_PREFIX
        } else {
            UNSELECTED_PREFIX
        };
        let suffix = if prompt.is_builtin { "" } else { " (Custom)" };

        // Truncate long names to 40 characters
        let name = if prompt.name.len() > 40 {
            format!("{}…", &prompt.name[..39])
        } else {
            prompt.name.clone()
        };

        let label = format!("{}{}{}", prefix, name, suffix);
        let item = MenuItemBuilder::with_id(menu_id, &label)
            .enabled(enhancement_enabled)
            .build(app)?;
        submenu = submenu.item(&item);
    }

    Ok(submenu.build()?)
}

/// Get shortcut hint for tooltip
fn get_shortcut_hint() -> String {
    match config::get_config() {
        Ok(cfg) => {
            let shortcut = &cfg.shortcuts.toggle_recording;
            format!("Press {} to record", shortcut)
        }
        Err(_) => "Press F13 to record".to_string(),
    }
}

// =============================================================================
// Tray Icon Creation
// =============================================================================

/// Pre-rendered 𓅝 ibis hieroglyph tray icons (Noto Sans Egyptian Hieroglyphs)
static TRAY_IDLE_PNG: &[u8] = include_bytes!("../icons/tray-idle-44.png");
static TRAY_IDLE_LIGHT_PNG: &[u8] = include_bytes!("../icons/tray-idle-light-44.png");
static TRAY_RECORDING_PNG: &[u8] = include_bytes!("../icons/tray-recording-44.png");

/// Detect if the system is using a dark theme (Linux only)
#[cfg(target_os = "linux")]
fn is_dark_theme() -> bool {
    // Check GNOME/GTK theme preference
    // Try gsettings first (most reliable for GNOME)
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("'prefer-dark'") || stdout.contains("'dark'") {
            return true;
        }
        if stdout.contains("'prefer-light'") || stdout.contains("'light'") {
            return false;
        }
    }

    // Fallback: check GTK theme name
    if let Ok(output) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        if stdout.contains("dark") || stdout.contains("adwaita-dark") {
            return true;
        }
    }

    // Check environment variable
    if let Ok(theme) = std::env::var("GTK_THEME") {
        let theme_lower = theme.to_lowercase();
        if theme_lower.contains("dark") {
            return true;
        }
    }

    // Default to light theme assumption
    false
}

#[cfg(not(target_os = "linux"))]
fn is_dark_theme() -> bool {
    // On macOS, template mode handles this automatically
    false
}

/// Create the idle tray icon (theme-aware on Linux)
fn create_idle_icon() -> Image<'static> {
    #[cfg(target_os = "linux")]
    {
        // On Linux, use white icon for dark themes, black for light themes
        if is_dark_theme() {
            return Image::from_bytes(TRAY_IDLE_LIGHT_PNG)
                .expect("embedded light idle tray icon is valid PNG");
        }
    }

    Image::from_bytes(TRAY_IDLE_PNG).expect("embedded idle tray icon is valid PNG")
}

/// Create the recording tray icon (Scribe's Amber #D08B3E)
fn create_recording_icon() -> Image<'static> {
    Image::from_bytes(TRAY_RECORDING_PNG).expect("embedded recording tray icon is valid PNG")
}

// =============================================================================
// State Updates
// =============================================================================

/// Update the tray to reflect recording state
pub fn set_recording_state(app: &AppHandle, is_recording: bool) {
    // Update internal state
    {
        let mut state = get_tray_state().write();
        state.is_recording = is_recording;
        state.pipeline_state = if is_recording {
            PipelineState::Recording
        } else {
            PipelineState::Idle
        };
    }

    // Update tray icon
    if let Some(tray) = app.tray_by_id("main") {
        let icon = if is_recording {
            create_recording_icon()
        } else {
            create_idle_icon()
        };
        if let Err(e) = tray.set_icon(Some(icon)) {
            tracing::warn!("Failed to update tray icon: {}", e);
        }
        // Template mode: idle = template (macOS auto-tints), recording = coloured
        if let Err(e) = tray.set_icon_as_template(!is_recording) {
            tracing::warn!("Failed to set icon template mode: {}", e);
        }
    }

    // Update menu
    rebuild_tray_menu(app);
}

/// Update the tray with the latest transcription
pub fn set_last_transcription(app: &AppHandle, text: Option<String>) {
    // Update internal state
    {
        let mut state = get_tray_state().write();
        state.last_transcription = text;
    }

    // Update menu
    rebuild_tray_menu(app);
}

/// Rebuild the tray menu with current state
fn rebuild_tray_menu(app: &AppHandle) {
    // Enumerate devices and read config outside of any lock to avoid
    // blocking tray state updates during CoreAudio enumeration
    let devices = audio::device::list_input_devices();
    let cfg = config::get_config().ok();
    let selected_device_id = cfg.as_ref().and_then(|c| c.audio.device_id.clone());
    let enhancement_enabled = cfg.as_ref().is_some_and(|c| c.enhancement.enabled);
    let active_prompt_id = cfg
        .as_ref()
        .map(|c| c.enhancement.prompt_id.clone())
        .unwrap_or_else(|| "fix-grammar".to_string());

    let state = get_tray_state().read();

    // Try to get last transcription from database if not in state
    let last_text = state.last_transcription.clone().or_else(|| {
        database::transcription::list_transcriptions(Some(1), Some(0))
            .ok()
            .and_then(|v| v.into_iter().next())
            .map(|t| t.text)
    });

    match build_tray_menu(
        app,
        state.is_recording,
        last_text.as_deref(),
        &devices,
        selected_device_id.as_deref(),
        enhancement_enabled,
        &active_prompt_id,
    ) {
        Ok(menu) => {
            drop(state); // Release lock before menu operations
            if let Some(tray) = app.tray_by_id("main") {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    tracing::warn!("Failed to update tray menu: {}", e);
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to build tray menu: {}", e);
        }
    }
}

// =============================================================================
// Event Handlers
// =============================================================================

/// Handle menu item clicks
fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        menu_ids::TOGGLE_RECORDING => {
            tracing::info!("Toggle recording clicked from tray menu");

            // Show recording indicator immediately if not already recording
            // This is especially important on Wayland where keyboard shortcuts may not work
            if !crate::pipeline::is_pipeline_running() {
                if let Err(e) = crate::recording_indicator::show_indicator_instant(app) {
                    tracing::warn!("Failed to show recording indicator from tray: {}", e);
                }
            }

            // Emit the same event as keyboard shortcut would
            if let Err(e) = app.emit("shortcut-triggered", "toggle_recording") {
                tracing::error!("Failed to emit shortcut-triggered event from tray: {}", e);
            }
        }
        menu_ids::COPY_LAST => {
            tracing::info!("Copy last transcription clicked");
            handle_copy_last(app);
        }
        menu_ids::TRANSCRIBE => {
            tracing::info!("Transcribe clicked");
            handle_open_transcribe(app);
        }
        menu_ids::HISTORY => {
            tracing::info!("History clicked");
            handle_open_history(app);
        }
        menu_ids::SETTINGS => {
            tracing::info!("Settings clicked");
            handle_open_settings(app);
        }
        menu_ids::QUIT => {
            tracing::info!("Quit clicked");
            app.exit(0);
        }
        menu_ids::AI_ENHANCEMENT_TOGGLE => {
            tracing::info!("AI enhancement toggle clicked");
            handle_toggle_enhancement(app);
        }
        _ if id.starts_with(menu_ids::PROMPT_PREFIX) => {
            let prompt_id = &id[menu_ids::PROMPT_PREFIX.len()..];
            tracing::info!("Prompt selected from tray: {:?}", prompt_id);
            handle_select_prompt(app, prompt_id.to_string());
        }
        _ if id.starts_with(menu_ids::INPUT_SOURCE_PREFIX) => {
            let suffix = &id[menu_ids::INPUT_SOURCE_PREFIX.len()..];
            let device_id = if suffix == "__default__" {
                None
            } else {
                Some(suffix.to_string())
            };
            tracing::info!("Input source selected from tray: {:?}", device_id);
            handle_select_audio_device(app, device_id);
        }
        _ => {
            tracing::debug!("Unknown menu item: {}", id);
        }
    }
}

/// Handle tray icon events (click, double-click, etc.)
fn handle_tray_event(event: TrayIconEvent) {
    match event {
        TrayIconEvent::Click { .. } => {
            tracing::debug!("Tray icon clicked");
        }
        TrayIconEvent::DoubleClick { .. } => {
            tracing::debug!("Tray icon double-clicked");
        }
        _ => {}
    }
}

/// Copy the last transcription to clipboard
fn handle_copy_last(app: &AppHandle) {
    // Get last transcription from database
    match database::transcription::list_transcriptions(Some(1), Some(0)) {
        Ok(transcriptions) => {
            if let Some(t) = transcriptions.into_iter().next() {
                // Use the clipboard module to copy
                let app_clone = app.clone();
                let text = t.text.clone();
                tauri::async_runtime::spawn(async move {
                    match crate::clipboard::copy_transcription(app_clone, text, false).await {
                        Ok(_) => {
                            tracing::info!("Copied last transcription to clipboard");
                        }
                        Err(e) => {
                            tracing::error!("Failed to copy to clipboard: {}", e);
                        }
                    }
                });
            } else {
                tracing::info!("No transcriptions to copy");
            }
        }
        Err(e) => {
            tracing::error!("Failed to get last transcription: {}", e);
        }
    }
}

/// Open the history window
fn handle_open_history(app: &AppHandle) {
    // Show the main window and emit event to navigate to history
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        // Emit event to navigate to history view
        if let Err(e) = app.emit("navigate", "history") {
            tracing::warn!("Failed to emit navigate event: {}", e);
        }
    }
}

/// Open the transcribe pane
fn handle_open_transcribe(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        if let Err(e) = app.emit("navigate", "transcribe") {
            tracing::warn!("Failed to emit navigate event: {}", e);
        }
    }
}

/// Open the settings window
fn handle_open_settings(app: &AppHandle) {
    // Show the main window and emit event to navigate to settings
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        // Emit event to navigate to settings view
        if let Err(e) = app.emit("navigate", "overview") {
            tracing::warn!("Failed to emit navigate event: {}", e);
        }
    }
}

/// Handle audio device selection from the tray submenu
fn handle_select_audio_device(app: &AppHandle, device_id: Option<String>) {
    if let Err(e) = config::set_audio_device_config(device_id.clone()) {
        tracing::error!("Failed to save audio device: {}", e);
        return;
    }
    tracing::info!("Audio device changed via tray to: {:?}", device_id);

    // Notify frontend so the Settings UI stays in sync
    let _ = app.emit("audio-device-changed", &device_id);

    // Rebuild tray menu to update checkmarks
    rebuild_tray_menu(app);
}

/// Handle AI enhancement toggle from the tray submenu
fn handle_toggle_enhancement(app: &AppHandle) {
    match config::get_config() {
        Ok(mut cfg) => {
            cfg.enhancement.enabled = !cfg.enhancement.enabled;
            let enabled = cfg.enhancement.enabled;
            if let Err(e) = config::set_config(cfg) {
                tracing::error!("Failed to save enhancement toggle: {}", e);
                return;
            }
            tracing::info!("AI enhancement toggled via tray to: {}", enabled);

            // Notify frontend so the Settings UI stays in sync
            let _ = app.emit("enhancement-toggled", enabled);

            // Rebuild tray menu to update the toggle checkmark and prompt item states
            rebuild_tray_menu(app);
        }
        Err(e) => {
            tracing::error!("Failed to read config for enhancement toggle: {}", e);
        }
    }
}

/// Handle prompt selection from the tray submenu
fn handle_select_prompt(app: &AppHandle, prompt_id: String) {
    if let Err(e) = config::set_prompt_config(prompt_id.clone()) {
        tracing::error!("Failed to save prompt config: {}", e);
        return;
    }
    tracing::info!("Prompt changed via tray to: {:?}", prompt_id);

    // Notify frontend so the Settings UI stays in sync
    let _ = app.emit("prompt-changed", &prompt_id);

    // Rebuild tray menu to update checkmarks
    rebuild_tray_menu(app);
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// Get the current tray state
#[tauri::command]
pub fn get_tray_state_cmd() -> TrayStateInfo {
    let state = get_tray_state().read();
    TrayStateInfo {
        is_recording: state.is_recording,
        has_last_transcription: state.last_transcription.is_some(),
    }
}

/// Update tray recording state (called from pipeline)
#[tauri::command]
pub fn update_tray_recording_state(app: AppHandle, is_recording: bool) {
    set_recording_state(&app, is_recording);
}

/// Refresh the tray menu (e.g. after permissions change)
#[tauri::command]
pub fn refresh_tray_menu(app: AppHandle) {
    rebuild_tray_menu(&app);
}

/// Tray state info for frontend
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayStateInfo {
    pub is_recording: bool,
    pub has_last_transcription: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_tray_icons_are_valid_png() {
        // Verify the embedded PNG data can be decoded into RGBA images
        let idle = create_idle_icon();
        assert!(idle.width() > 0, "Idle icon should have non-zero width");
        assert!(idle.height() > 0, "Idle icon should have non-zero height");

        let recording = create_recording_icon();
        assert!(
            recording.width() > 0,
            "Recording icon should have non-zero width"
        );
        assert!(
            recording.height() > 0,
            "Recording icon should have non-zero height"
        );
    }

    #[test]
    fn test_tray_state_info_serialisation() {
        let info = TrayStateInfo {
            is_recording: true,
            has_last_transcription: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"isRecording\":true"));
        assert!(json.contains("\"hasLastTranscription\":true"));
    }
}
