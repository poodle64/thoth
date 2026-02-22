//! Shortcut manager for Thoth
//!
//! Handles registration and management of global keyboard shortcuts
//! for controlling recording and other application features.

use crate::recording_indicator;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// Information about a registered shortcut
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutInfo {
    /// Unique identifier for the shortcut
    pub id: String,
    /// Keyboard accelerator string (e.g., "F13", "Cmd+Shift+Space")
    pub accelerator: String,
    /// Human-readable description of what the shortcut does
    pub description: String,
    /// Whether the shortcut is currently enabled
    pub is_enabled: bool,
}

/// Default shortcut identifiers
pub mod shortcut_ids {
    pub const TOGGLE_RECORDING: &str = "toggle_recording";
    pub const TOGGLE_RECORDING_ALT: &str = "toggle_recording_alt";
    pub const COPY_LAST_TRANSCRIPTION: &str = "copy_last_transcription";
}

/// Global shortcut manager instance
static MANAGER: OnceLock<RwLock<ShortcutManagerState>> = OnceLock::new();

/// Minimum interval between press events for the same shortcut (debounce).
/// 50ms is enough to absorb electrical key bounce while allowing rapid intentional presses.
const PRESS_DEBOUNCE_MS: u64 = 50;

/// Internal state for the shortcut manager
struct ShortcutManagerState {
    /// Registered shortcuts by ID
    shortcuts: HashMap<String, ShortcutInfo>,
    /// Last press timestamp per shortcut ID (for debouncing key bounce)
    last_press_times: HashMap<String, Instant>,
}

impl ShortcutManagerState {
    fn new() -> Self {
        Self {
            shortcuts: HashMap::new(),
            last_press_times: HashMap::new(),
        }
    }
}

fn get_manager() -> &'static RwLock<ShortcutManagerState> {
    MANAGER.get_or_init(|| RwLock::new(ShortcutManagerState::new()))
}

/// Get the default shortcuts for Thoth
pub fn get_defaults() -> Vec<ShortcutInfo> {
    vec![
        ShortcutInfo {
            id: shortcut_ids::TOGGLE_RECORDING.to_string(),
            accelerator: "F13".to_string(),
            description: "Toggle recording (push-to-talk)".to_string(),
            is_enabled: false,
        },
        ShortcutInfo {
            id: shortcut_ids::COPY_LAST_TRANSCRIPTION.to_string(),
            accelerator: "F14".to_string(),
            description: "Copy last transcription to clipboard".to_string(),
            is_enabled: false,
        },
        ShortcutInfo {
            id: shortcut_ids::TOGGLE_RECORDING_ALT.to_string(),
            accelerator: "ShiftRight".to_string(),
            description: "Toggle recording (alternative)".to_string(),
            is_enabled: false,
        },
    ]
}

/// Payload for shortcut events
#[derive(Debug, Clone, Serialize)]
pub struct ShortcutEvent {
    /// Shortcut identifier
    pub id: String,
    /// Key state: "pressed" or "released"
    pub state: String,
}

/// Register a global shortcut with the given ID and accelerator
///
/// The shortcut will emit events to the frontend when triggered:
/// - Event name: "shortcut-triggered" (for backwards compatibility, key pressed only)
/// - Event name: "shortcut-pressed" (key down)
/// - Event name: "shortcut-released" (key up, for PTT mode)
/// - Payload: ShortcutEvent with id and state
pub fn register<R: Runtime>(
    app: &AppHandle<R>,
    id: String,
    accelerator: String,
    description: String,
) -> Result<(), String> {
    let global_shortcut = app.global_shortcut();

    // Check if already registered with the system
    if global_shortcut.is_registered(accelerator.as_str()) {
        tracing::debug!(
            "Shortcut '{}' already registered, skipping duplicate registration",
            accelerator
        );
        return Err(format!("Shortcut '{}' is already registered", accelerator));
    }

    // Clone values for the closure
    let shortcut_id = id.clone();
    let shortcut_accel = accelerator.clone();
    let app_handle = app.clone();

    tracing::debug!(
        "Registering shortcut handler for '{}' (accelerator: '{}')",
        id,
        accelerator
    );

    // Register with the global shortcut plugin
    global_shortcut
        .on_shortcut(accelerator.as_str(), move |_app, shortcut, event| {
            // Discard events during capture mode. Queued OS events may fire
            // even after unregistration; this guard prevents phantom triggers.
            if crate::keyboard_service::is_capture_active() {
                tracing::debug!(
                    "Discarding shortcut event for '{}' — capture mode active",
                    shortcut_id
                );
                return;
            }

            // Suppress shortcuts when the screen is locked or the screensaver
            // is active. Prevents accidental recording when the user presses a
            // key to dismiss the lock screen.
            if crate::platform::is_screen_locked() {
                tracing::debug!(
                    "Discarding shortcut event for '{}' — screen is locked",
                    shortcut_id
                );
                return;
            }

            // Log at INFO level so it always shows
            tracing::info!(
                ">>> Shortcut callback fired for '{}' (accelerator: '{}', shortcut: {:?})",
                shortcut_id,
                shortcut_accel,
                shortcut
            );
            let state_str = match event.state {
                ShortcutState::Pressed => "pressed",
                ShortcutState::Released => "released",
            };

            let shortcut_event = ShortcutEvent {
                id: shortcut_id.clone(),
                state: state_str.to_string(),
            };

            match event.state {
                ShortcutState::Pressed => {
                    // Debounce rapid press events (key bounce protection).
                    // Only allow one press per PRESS_DEBOUNCE_MS window per shortcut.
                    {
                        let mut manager = get_manager().write();
                        if let Some(last) = manager.last_press_times.get(&shortcut_id) {
                            let elapsed = last.elapsed().as_millis();
                            if elapsed < PRESS_DEBOUNCE_MS as u128 {
                                tracing::info!(
                                    "Debounced shortcut press for '{}' ({}ms since last, threshold {}ms)",
                                    shortcut_id,
                                    elapsed,
                                    PRESS_DEBOUNCE_MS
                                );
                                return;
                            }
                        }
                        manager
                            .last_press_times
                            .insert(shortcut_id.clone(), Instant::now());
                    }

                    tracing::info!("Shortcut pressed: {}", shortcut_id);

                    // For recording shortcuts, show indicator IMMEDIATELY in Rust
                    // before emitting to frontend. This eliminates JS round-trip delay.
                    // Only show when starting (not already recording).
                    if (shortcut_id == shortcut_ids::TOGGLE_RECORDING
                        || shortcut_id == shortcut_ids::TOGGLE_RECORDING_ALT)
                        && !crate::pipeline::is_pipeline_running()
                        && crate::transcription::is_transcription_ready()
                    {
                        if let Err(e) = recording_indicator::show_indicator_instant(&app_handle) {
                            tracing::warn!(
                                "Failed to show recording indicator from shortcut: {}",
                                e
                            );
                        }
                    }

                    // Handle copy-last-transcription directly in Rust
                    // (no frontend round-trip needed)
                    if shortcut_id == shortcut_ids::COPY_LAST_TRANSCRIPTION {
                        match crate::database::transcription::list_transcriptions(
                            Some(1),
                            Some(0),
                        ) {
                            Ok(transcriptions) => {
                                if let Some(t) = transcriptions.into_iter().next() {
                                    match arboard::Clipboard::new()
                                        .and_then(|mut cb| cb.set_text(t.text))
                                    {
                                        Ok(()) => tracing::info!(
                                            "Copied last transcription to clipboard via shortcut"
                                        ),
                                        Err(e) => tracing::error!(
                                            "Failed to copy to clipboard: {}",
                                            e
                                        ),
                                    }
                                } else {
                                    tracing::info!("No transcriptions to copy");
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to get last transcription: {}", e)
                            }
                        }
                        return;
                    }

                    // Emit legacy event for backwards compatibility
                    match app_handle.emit("shortcut-triggered", shortcut_id.clone()) {
                        Ok(_) => {
                            tracing::info!("Emitted shortcut-triggered event for: {}", shortcut_id)
                        }
                        Err(e) => tracing::error!("Failed to emit shortcut-triggered event: {}", e),
                    }
                    // Emit new event with full state info
                    match app_handle.emit("shortcut-pressed", &shortcut_event) {
                        Ok(_) => tracing::debug!("Emitted shortcut-pressed event"),
                        Err(e) => tracing::error!("Failed to emit shortcut-pressed event: {}", e),
                    }
                }
                ShortcutState::Released => {
                    tracing::info!("Shortcut released: {}", shortcut_id);
                    // Emit release event for PTT mode
                    if let Err(e) = app_handle.emit("shortcut-released", &shortcut_event) {
                        tracing::error!("Failed to emit shortcut-released event: {}", e);
                    }
                }
            }
        })
        .map_err(|e| format!("Failed to register shortcut '{}': {}", accelerator, e))?;

    // Store in our manager state
    let info = ShortcutInfo {
        id: id.clone(),
        accelerator: accelerator.clone(),
        description,
        is_enabled: true,
    };

    {
        let mut manager = get_manager().write();
        manager.shortcuts.insert(id.clone(), info);
    }

    tracing::info!(
        "Registered shortcut '{}' with accelerator '{}'",
        id,
        accelerator
    );
    Ok(())
}

/// Unregister a shortcut by its ID
pub fn unregister<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    let accelerator = {
        let manager = get_manager().read();
        manager
            .shortcuts
            .get(id)
            .map(|info| info.accelerator.clone())
            .ok_or_else(|| format!("Shortcut '{}' is not registered", id))?
    };

    let global_shortcut = app.global_shortcut();

    global_shortcut
        .unregister(accelerator.as_str())
        .map_err(|e| format!("Failed to unregister shortcut '{}': {}", accelerator, e))?;

    {
        let mut manager = get_manager().write();
        manager.shortcuts.remove(id);
    }

    tracing::info!("Unregistered shortcut '{}'", id);
    Ok(())
}

/// List all currently registered shortcuts
pub fn list_registered() -> Vec<ShortcutInfo> {
    let manager = get_manager().read();
    manager.shortcuts.values().cloned().collect()
}

/// Register all default shortcuts
pub fn register_defaults<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let defaults = get_defaults();
    let mut errors = Vec::new();

    for shortcut in defaults {
        if let Err(e) = register(
            app,
            shortcut.id.clone(),
            shortcut.accelerator.clone(),
            shortcut.description.clone(),
        ) {
            tracing::warn!(
                "Failed to register default shortcut '{}': {}",
                shortcut.id,
                e
            );
            errors.push(format!("{}: {}", shortcut.id, e));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Some default shortcuts failed to register: {}",
            errors.join("; ")
        ))
    }
}

/// Unregister all shortcuts
pub fn unregister_all<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let global_shortcut = app.global_shortcut();

    global_shortcut
        .unregister_all()
        .map_err(|e| format!("Failed to unregister all shortcuts: {}", e))?;

    {
        let mut manager = get_manager().write();
        manager.shortcuts.clear();
    }

    tracing::info!("Unregistered all shortcuts");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_defaults_returns_expected_shortcuts() {
        let defaults = get_defaults();

        assert_eq!(defaults.len(), 3);

        let toggle = defaults
            .iter()
            .find(|s| s.id == shortcut_ids::TOGGLE_RECORDING);
        assert!(toggle.is_some());
        assert_eq!(toggle.unwrap().accelerator, "F13");

        let copy = defaults
            .iter()
            .find(|s| s.id == shortcut_ids::COPY_LAST_TRANSCRIPTION);
        assert!(copy.is_some());
        assert_eq!(copy.unwrap().accelerator, "F14");

        let alt = defaults
            .iter()
            .find(|s| s.id == shortcut_ids::TOGGLE_RECORDING_ALT);
        assert!(alt.is_some());
        assert_eq!(alt.unwrap().accelerator, "ShiftRight");
    }

    #[test]
    fn test_shortcut_info_serialisation() {
        let info = ShortcutInfo {
            id: "test".to_string(),
            accelerator: "Ctrl+T".to_string(),
            description: "Test shortcut".to_string(),
            is_enabled: true,
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialised: ShortcutInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialised.id, info.id);
        assert_eq!(deserialised.accelerator, info.accelerator);
        assert_eq!(deserialised.description, info.description);
        assert_eq!(deserialised.is_enabled, info.is_enabled);
    }
}
