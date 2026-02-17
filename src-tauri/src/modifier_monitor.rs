//! Modifier key monitoring for standalone modifier shortcuts
//!
//! This module provides continuous keyboard polling to detect modifier keys
//! (Right Shift, Right Alt, etc.) used as standalone shortcuts. This is needed
//! because Tauri's GlobalShortcut system doesn't support registering modifier
//! keys alone - they must be combined with a regular key.
//!
//! Uses device_query for cross-platform keyboard state polling.

use device_query::{DeviceQuery, DeviceState, Keycode};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Runtime};

/// Poll interval for keyboard state (ms)
const POLL_INTERVAL_MS: u64 = 20;

/// Cooldown between shortcut triggers to prevent double-firing (ms)
const TRIGGER_COOLDOWN_MS: u64 = 500;

/// Threshold for "brief press" vs "hold" in toggle mode (ms)
const BRIEF_PRESS_THRESHOLD_MS: u64 = 500;

/// Modifier keys that can be used as standalone shortcuts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModifierKey {
    ShiftRight,
    ShiftLeft,
    ControlRight,
    ControlLeft,
    AltRight,
    AltLeft,
    MetaRight,
    MetaLeft,
}

impl ModifierKey {
    /// Convert from accelerator string
    pub fn from_accelerator(s: &str) -> Option<Self> {
        match s {
            "ShiftRight" => Some(Self::ShiftRight),
            "ShiftLeft" => Some(Self::ShiftLeft),
            "ControlRight" => Some(Self::ControlRight),
            "ControlLeft" => Some(Self::ControlLeft),
            "AltRight" => Some(Self::AltRight),
            "AltLeft" => Some(Self::AltLeft),
            "MetaRight" => Some(Self::MetaRight),
            "MetaLeft" => Some(Self::MetaLeft),
            _ => None,
        }
    }

    /// Convert to accelerator string
    pub fn to_accelerator(self) -> &'static str {
        match self {
            Self::ShiftRight => "ShiftRight",
            Self::ShiftLeft => "ShiftLeft",
            Self::ControlRight => "ControlRight",
            Self::ControlLeft => "ControlLeft",
            Self::AltRight => "AltRight",
            Self::AltLeft => "AltLeft",
            Self::MetaRight => "MetaRight",
            Self::MetaLeft => "MetaLeft",
        }
    }

    /// Convert to device_query Keycode
    fn to_keycode(self) -> Keycode {
        match self {
            Self::ShiftRight => Keycode::RShift,
            Self::ShiftLeft => Keycode::LShift,
            Self::ControlRight => Keycode::RControl,
            Self::ControlLeft => Keycode::LControl,
            Self::AltRight => Keycode::RAlt,
            Self::AltLeft => Keycode::LAlt,
            Self::MetaRight => Keycode::RMeta,
            Self::MetaLeft => Keycode::LMeta,
        }
    }

    /// Check if this is a standalone modifier accelerator
    pub fn is_modifier_accelerator(accelerator: &str) -> bool {
        Self::from_accelerator(accelerator).is_some()
    }
}

/// Registered modifier shortcut
#[derive(Debug, Clone)]
struct ModifierShortcut {
    id: String,
    modifier: ModifierKey,
    description: String,
}

/// State for tracking key press timing
#[derive(Debug, Default)]
struct KeyState {
    is_pressed: bool,
    press_time: Option<Instant>,
    last_trigger: Option<Instant>,
    /// For toggle mode: tracks if we're in "hands-free" mode after a brief press
    hands_free_mode: bool,
}

/// Global monitor state
struct MonitorState {
    shortcuts: HashMap<String, ModifierShortcut>,
    key_states: HashMap<ModifierKey, KeyState>,
    is_running: AtomicBool,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            shortcuts: HashMap::new(),
            key_states: HashMap::new(),
            is_running: AtomicBool::new(false),
        }
    }
}

/// Global monitor instance
static MONITOR: OnceLock<RwLock<MonitorState>> = OnceLock::new();

fn get_monitor() -> &'static RwLock<MonitorState> {
    MONITOR.get_or_init(|| RwLock::new(MonitorState::default()))
}

/// Check if an accelerator is a standalone modifier
pub fn is_modifier_shortcut(accelerator: &str) -> bool {
    ModifierKey::is_modifier_accelerator(accelerator)
}

/// Register a modifier shortcut for monitoring
pub fn register_modifier_shortcut(id: String, accelerator: String, description: String) -> bool {
    let Some(modifier) = ModifierKey::from_accelerator(&accelerator) else {
        return false;
    };

    let mut state = get_monitor().write();
    state.shortcuts.insert(
        id.clone(),
        ModifierShortcut {
            id,
            modifier,
            description,
        },
    );
    state.key_states.entry(modifier).or_default();

    tracing::info!(
        "Registered modifier shortcut: {} -> {:?}",
        accelerator,
        modifier
    );
    true
}

/// Unregister a modifier shortcut
pub fn unregister_modifier_shortcut(id: &str) -> bool {
    let mut state = get_monitor().write();
    if state.shortcuts.remove(id).is_some() {
        tracing::info!("Unregistered modifier shortcut: {}", id);
        true
    } else {
        false
    }
}

/// Check if a modifier shortcut is registered
pub fn is_modifier_shortcut_registered(id: &str) -> bool {
    get_monitor().read().shortcuts.contains_key(id)
}

/// Get all registered modifier shortcuts
pub fn list_modifier_shortcuts() -> Vec<(String, String, String)> {
    get_monitor()
        .read()
        .shortcuts
        .values()
        .map(|s| {
            (
                s.id.clone(),
                s.modifier.to_accelerator().to_string(),
                s.description.clone(),
            )
        })
        .collect()
}

/// Start the modifier key monitor
///
/// Spawns a background thread that polls keyboard state and emits events
/// when registered modifier keys are pressed/released.
///
/// Note: On Wayland, this won't work because device_query requires X11.
/// Users on Wayland should use regular function key shortcuts instead.
pub fn start_monitor<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let state = get_monitor().read();
    if state.is_running.load(Ordering::SeqCst) {
        return Ok(()); // Already running
    }
    drop(state);

    // Check if there are any shortcuts to monitor
    let shortcuts_to_monitor: Vec<_> = {
        let state = get_monitor().read();
        state.shortcuts.values().cloned().collect()
    };

    if shortcuts_to_monitor.is_empty() {
        tracing::debug!("No modifier shortcuts registered, not starting monitor");
        return Ok(());
    }

    // On Linux, check if we're on Wayland (device_query requires X11)
    #[cfg(target_os = "linux")]
    {
        let display_server = crate::shortcuts::get_display_server();
        if display_server == crate::shortcuts::DisplayServer::Wayland {
            tracing::warn!(
                "Modifier key shortcuts are not supported on Wayland. \
                 Device_query requires X11. Please use function key shortcuts (F13, F14) instead, \
                 or switch to an X11 session."
            );
            // Don't start the monitor on Wayland - it won't work
            return Ok(());
        }
    }

    get_monitor()
        .write()
        .is_running
        .store(true, Ordering::SeqCst);

    tracing::info!(
        "Starting modifier key monitor for {} shortcuts",
        shortcuts_to_monitor.len()
    );

    // Spawn monitor thread
    thread::spawn(move || {
        let device_state = DeviceState::new();

        while get_monitor().read().is_running.load(Ordering::SeqCst) {
            let keys: HashSet<Keycode> = device_state.get_keys().into_iter().collect();

            // Get shortcuts to check
            let shortcuts: Vec<_> = {
                let state = get_monitor().read();
                state.shortcuts.values().cloned().collect()
            };

            for shortcut in shortcuts {
                let keycode = shortcut.modifier.to_keycode();
                let is_pressed = keys.contains(&keycode);

                // Get current state
                let (was_pressed, press_time, last_trigger, hands_free) = {
                    let state = get_monitor().read();
                    let key_state = state.key_states.get(&shortcut.modifier);
                    (
                        key_state.map(|s| s.is_pressed).unwrap_or(false),
                        key_state.and_then(|s| s.press_time),
                        key_state.and_then(|s| s.last_trigger),
                        key_state.map(|s| s.hands_free_mode).unwrap_or(false),
                    )
                };

                // Check cooldown
                if let Some(last) = last_trigger {
                    if last.elapsed().as_millis() < TRIGGER_COOLDOWN_MS as u128 {
                        continue;
                    }
                }

                if is_pressed && !was_pressed {
                    // Key just pressed
                    {
                        let mut state = get_monitor().write();
                        if let Some(key_state) = state.key_states.get_mut(&shortcut.modifier) {
                            key_state.is_pressed = true;
                            key_state.press_time = Some(Instant::now());
                        }
                    }

                    if hands_free {
                        // In hands-free mode, pressing again stops recording
                        {
                            let mut state = get_monitor().write();
                            if let Some(key_state) = state.key_states.get_mut(&shortcut.modifier) {
                                key_state.hands_free_mode = false;
                                key_state.last_trigger = Some(Instant::now());
                            }
                        }
                        emit_shortcut_event(&app, &shortcut.id, "pressed");
                    } else {
                        // Start recording on press
                        {
                            let mut state = get_monitor().write();
                            if let Some(key_state) = state.key_states.get_mut(&shortcut.modifier) {
                                key_state.last_trigger = Some(Instant::now());
                            }
                        }
                        emit_shortcut_event(&app, &shortcut.id, "pressed");
                    }
                } else if !is_pressed && was_pressed {
                    // Key just released
                    let press_duration = press_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);

                    {
                        let mut state = get_monitor().write();
                        if let Some(key_state) = state.key_states.get_mut(&shortcut.modifier) {
                            key_state.is_pressed = false;
                            key_state.press_time = None;

                            if press_duration < BRIEF_PRESS_THRESHOLD_MS as u128 {
                                // Brief press - enter hands-free mode (don't stop yet)
                                key_state.hands_free_mode = true;
                                tracing::debug!(
                                    "Brief press detected ({}ms), entering hands-free mode",
                                    press_duration
                                );
                            } else {
                                // Long press - stop recording
                                key_state.hands_free_mode = false;
                                key_state.last_trigger = Some(Instant::now());
                            }
                        }
                    }

                    // Only emit release if it was a long press (not brief/hands-free)
                    if press_duration >= BRIEF_PRESS_THRESHOLD_MS as u128 {
                        emit_shortcut_event(&app, &shortcut.id, "released");
                    }
                }
            }

            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }

        tracing::info!("Modifier key monitor stopped");
    });

    Ok(())
}

/// Stop the modifier key monitor
pub fn stop_monitor() {
    let state = get_monitor().read();
    if state.is_running.load(Ordering::SeqCst) {
        drop(state);
        get_monitor()
            .write()
            .is_running
            .store(false, Ordering::SeqCst);
        tracing::info!("Stopping modifier key monitor");
    }
}

/// Restart the monitor (call after registering/unregistering shortcuts)
pub fn restart_monitor<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    stop_monitor();
    // Give the thread time to stop
    thread::sleep(Duration::from_millis(50));
    start_monitor(app)
}

/// Emit a shortcut event to the frontend
fn emit_shortcut_event<R: Runtime>(app: &AppHandle<R>, id: &str, state: &str) {
    tracing::info!("Modifier shortcut {}: {}", state, id);

    // Emit the same events as the regular shortcut system
    if state == "pressed" {
        // Show recording indicator immediately
        if id.contains("toggle_recording") {
            if let Err(e) = crate::recording_indicator::show_indicator_instant(app) {
                tracing::warn!("Failed to show recording indicator: {}", e);
            }
        }

        // Emit shortcut-triggered for toggle mode compatibility
        if let Err(e) = app.emit("shortcut-triggered", id.to_string()) {
            tracing::error!("Failed to emit shortcut-triggered: {}", e);
        }

        // Emit shortcut-pressed with full info
        let event = serde_json::json!({
            "id": id,
            "state": "pressed"
        });
        if let Err(e) = app.emit("shortcut-pressed", &event) {
            tracing::error!("Failed to emit shortcut-pressed: {}", e);
        }
    } else if state == "released" {
        let event = serde_json::json!({
            "id": id,
            "state": "released"
        });
        if let Err(e) = app.emit("shortcut-released", &event) {
            tracing::error!("Failed to emit shortcut-released: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifier_key_from_accelerator() {
        assert_eq!(
            ModifierKey::from_accelerator("ShiftRight"),
            Some(ModifierKey::ShiftRight)
        );
        assert_eq!(
            ModifierKey::from_accelerator("AltLeft"),
            Some(ModifierKey::AltLeft)
        );
        assert_eq!(ModifierKey::from_accelerator("F13"), None);
        assert_eq!(ModifierKey::from_accelerator("CommandOrControl+S"), None);
    }

    #[test]
    fn test_is_modifier_shortcut() {
        assert!(is_modifier_shortcut("ShiftRight"));
        assert!(is_modifier_shortcut("ControlLeft"));
        assert!(!is_modifier_shortcut("F13"));
        assert!(!is_modifier_shortcut("CommandOrControl+Space"));
    }

    #[test]
    fn test_register_unregister() {
        let id = "test_modifier_shortcut";

        // Register
        assert!(register_modifier_shortcut(
            id.to_string(),
            "ShiftRight".to_string(),
            "Test".to_string()
        ));
        assert!(is_modifier_shortcut_registered(id));

        // Unregister
        assert!(unregister_modifier_shortcut(id));
        assert!(!is_modifier_shortcut_registered(id));

        // Can't unregister twice
        assert!(!unregister_modifier_shortcut(id));
    }

    #[test]
    fn test_register_invalid_accelerator() {
        assert!(!register_modifier_shortcut(
            "test".to_string(),
            "F13".to_string(),
            "Test".to_string()
        ));
    }
}
