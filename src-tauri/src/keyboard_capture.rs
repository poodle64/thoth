//! Native keyboard capture for shortcut recording
//!
//! Uses device_query to poll keyboard state at the system level,
//! bypassing webview limitations. This captures keys before they're
//! consumed by macOS system shortcuts.

use device_query::{DeviceQuery, DeviceState, Keycode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Poll interval for keyboard state (ms)
const POLL_INTERVAL_MS: u64 = 20;

/// Global capture state
static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Captured key combination
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedShortcut {
    /// The formatted accelerator string (e.g., "CommandOrControl+Shift+Y")
    pub accelerator: String,
    /// Individual keys pressed (for display)
    pub keys: Vec<String>,
    /// Whether this is a valid shortcut
    pub is_valid: bool,
}

/// Event payload for real-time key updates
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyCaptureEvent {
    /// Currently pressed keys
    pub keys: Vec<String>,
    /// Current accelerator string (may be incomplete)
    pub accelerator: String,
    /// Whether the current combination is valid
    pub is_valid: bool,
}

/// Check if Input Monitoring permission is available
#[tauri::command]
pub fn check_input_monitoring() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::platform::check_input_monitoring_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true // Always available on non-macOS
    }
}

/// Request Input Monitoring permission (opens System Preferences)
#[tauri::command]
pub fn request_input_monitoring() {
    #[cfg(target_os = "macos")]
    {
        crate::platform::open_input_monitoring_settings();
    }
}

/// Start capturing keyboard input
///
/// Spawns a background thread that polls keyboard state and emits events.
/// Call `stop_key_capture` to stop.
///
/// On macOS, requires Input Monitoring permission. Returns an error if
/// permission is not granted.
#[tauri::command]
pub fn start_key_capture(app: AppHandle) -> Result<(), String> {
    // Check Input Monitoring permission on macOS
    #[cfg(target_os = "macos")]
    {
        if !crate::platform::check_input_monitoring_permission() {
            tracing::warn!("Input Monitoring permission not granted");
            return Err("Input Monitoring permission required. Please grant permission in System Preferences > Privacy & Security > Input Monitoring".to_string());
        }
    }

    if CAPTURE_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("Capture already active".to_string());
    }

    tracing::info!("Starting native keyboard capture");

    // Spawn capture thread
    thread::spawn(move || {
        let device_state = DeviceState::new();
        let mut previous_keys: HashSet<Keycode> = HashSet::new();

        while CAPTURE_ACTIVE.load(Ordering::SeqCst) {
            let keys: HashSet<Keycode> = device_state.get_keys().into_iter().collect();

            // Only emit when keys change
            if keys != previous_keys {
                let (accelerator, key_names, is_valid) = format_keys(&keys);

                let event = KeyCaptureEvent {
                    keys: key_names,
                    accelerator: accelerator.clone(),
                    is_valid,
                };

                if let Err(e) = app.emit("key-capture-update", &event) {
                    tracing::warn!("Failed to emit key capture event: {}", e);
                }

                // If we have a valid shortcut, emit completion
                // Valid means: has a non-modifier key OR is a standalone right modifier
                let is_standalone_right_modifier =
                    keys.len() == 1 && keys.iter().any(is_right_modifier);
                if is_valid && (has_non_modifier_key(&keys) || is_standalone_right_modifier) {
                    tracing::info!("Valid shortcut captured: {}", accelerator);

                    let result = CapturedShortcut {
                        accelerator,
                        keys: event.keys,
                        is_valid: true,
                    };

                    if let Err(e) = app.emit("key-capture-complete", &result) {
                        tracing::warn!("Failed to emit key capture complete: {}", e);
                    }

                    // Auto-stop capture after valid shortcut
                    CAPTURE_ACTIVE.store(false, Ordering::SeqCst);
                    break;
                }

                previous_keys = keys;
            }

            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }

        tracing::info!("Keyboard capture stopped");
    });

    Ok(())
}

/// Stop capturing keyboard input
#[tauri::command]
pub fn stop_key_capture() -> Result<(), String> {
    CAPTURE_ACTIVE.store(false, Ordering::SeqCst);
    tracing::info!("Stopping keyboard capture");
    Ok(())
}

/// Check if capture is currently active
#[tauri::command]
pub fn is_key_capture_active() -> bool {
    CAPTURE_ACTIVE.load(Ordering::SeqCst)
}

/// Check if the key set contains any non-modifier key
fn has_non_modifier_key(keys: &HashSet<Keycode>) -> bool {
    keys.iter().any(|k| !is_modifier(k))
}

/// Check if a keycode is a left-side modifier (used as modifier in combinations)
fn is_left_modifier(key: &Keycode) -> bool {
    matches!(
        key,
        Keycode::LShift | Keycode::LControl | Keycode::LAlt | Keycode::LMeta
    )
}

/// Check if a keycode is a right-side modifier (can be used as standalone key)
fn is_right_modifier(key: &Keycode) -> bool {
    matches!(
        key,
        Keycode::RShift | Keycode::RControl | Keycode::RAlt | Keycode::RMeta
    )
}

/// Check if a keycode is any modifier
fn is_modifier(key: &Keycode) -> bool {
    is_left_modifier(key) || is_right_modifier(key)
}

/// Convert right modifier keycode to Tauri Code name for use as primary key
fn right_modifier_to_code(key: &Keycode) -> Option<String> {
    match key {
        Keycode::RShift => Some("ShiftRight".to_string()),
        Keycode::RControl => Some("ControlRight".to_string()),
        Keycode::RAlt => Some("AltRight".to_string()),
        Keycode::RMeta => Some("MetaRight".to_string()),
        _ => None,
    }
}

/// Convert right modifier keycode to display string
fn right_modifier_to_display(key: &Keycode) -> String {
    match key {
        Keycode::RShift => "Right Shift".to_string(),
        Keycode::RControl => "Right Ctrl".to_string(),
        Keycode::RAlt => "Right Option".to_string(),
        Keycode::RMeta => "Right Cmd".to_string(),
        _ => format!("{:?}", key),
    }
}

/// Format keys into an accelerator string and key names
///
/// Right-side modifiers (RShift, RAlt, RControl, RMeta) can be used as standalone
/// shortcut keys using their Code names (ShiftRight, AltRight, etc.)
fn format_keys(keys: &HashSet<Keycode>) -> (String, Vec<String>, bool) {
    let mut parts: Vec<String> = Vec::new();
    let mut key_names: Vec<String> = Vec::new();
    let mut has_main_key = false;

    // Check if ONLY a right-side modifier is pressed (standalone use)
    let only_right_modifier = keys.len() == 1 && keys.iter().any(is_right_modifier);

    if only_right_modifier {
        // Use the right modifier as the main key
        for key in keys {
            if let Some(name) = right_modifier_to_code(key) {
                parts.push(name.clone());
                key_names.push(right_modifier_to_display(key));
                has_main_key = true;
            }
        }
    } else {
        // Normal processing: left modifiers as modifiers, right modifiers also as modifiers
        // Check for modifiers first (in consistent order)
        if keys.contains(&Keycode::LMeta) || keys.contains(&Keycode::RMeta) {
            parts.push("CommandOrControl".to_string());
            key_names.push("Cmd".to_string());
        } else if keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl) {
            parts.push("CommandOrControl".to_string());
            key_names.push("Ctrl".to_string());
        }

        if keys.contains(&Keycode::LAlt) || keys.contains(&Keycode::RAlt) {
            parts.push("Alt".to_string());
            key_names.push("Alt".to_string());
        }

        if keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift) {
            parts.push("Shift".to_string());
            key_names.push("Shift".to_string());
        }

        // Add non-modifier keys
        for key in keys {
            if !is_modifier(key) {
                if let Some(name) = keycode_to_accelerator(key) {
                    parts.push(name.clone());
                    key_names.push(keycode_to_display(key));
                    has_main_key = true;
                }
            }
        }
    }

    let accelerator = parts.join("+");

    // Valid if we have at least one main key
    let is_valid = has_main_key;

    (accelerator, key_names, is_valid)
}

/// Convert keycode to Tauri accelerator string
fn keycode_to_accelerator(key: &Keycode) -> Option<String> {
    Some(
        match key {
            // Letters
            Keycode::A => "A",
            Keycode::B => "B",
            Keycode::C => "C",
            Keycode::D => "D",
            Keycode::E => "E",
            Keycode::F => "F",
            Keycode::G => "G",
            Keycode::H => "H",
            Keycode::I => "I",
            Keycode::J => "J",
            Keycode::K => "K",
            Keycode::L => "L",
            Keycode::M => "M",
            Keycode::N => "N",
            Keycode::O => "O",
            Keycode::P => "P",
            Keycode::Q => "Q",
            Keycode::R => "R",
            Keycode::S => "S",
            Keycode::T => "T",
            Keycode::U => "U",
            Keycode::V => "V",
            Keycode::W => "W",
            Keycode::X => "X",
            Keycode::Y => "Y",
            Keycode::Z => "Z",

            // Numbers
            Keycode::Key0 => "0",
            Keycode::Key1 => "1",
            Keycode::Key2 => "2",
            Keycode::Key3 => "3",
            Keycode::Key4 => "4",
            Keycode::Key5 => "5",
            Keycode::Key6 => "6",
            Keycode::Key7 => "7",
            Keycode::Key8 => "8",
            Keycode::Key9 => "9",

            // Function keys
            Keycode::F1 => "F1",
            Keycode::F2 => "F2",
            Keycode::F3 => "F3",
            Keycode::F4 => "F4",
            Keycode::F5 => "F5",
            Keycode::F6 => "F6",
            Keycode::F7 => "F7",
            Keycode::F8 => "F8",
            Keycode::F9 => "F9",
            Keycode::F10 => "F10",
            Keycode::F11 => "F11",
            Keycode::F12 => "F12",
            Keycode::F13 => "F13",
            Keycode::F14 => "F14",
            Keycode::F15 => "F15",
            Keycode::F16 => "F16",
            Keycode::F17 => "F17",
            Keycode::F18 => "F18",
            Keycode::F19 => "F19",
            Keycode::F20 => "F20",

            // Special keys
            Keycode::Space => "Space",
            Keycode::Enter => "Enter",
            Keycode::Tab => "Tab",
            Keycode::Backspace => "Backspace",
            Keycode::Delete => "Delete",
            Keycode::Insert => "Insert",
            Keycode::Home => "Home",
            Keycode::End => "End",
            Keycode::PageUp => "PageUp",
            Keycode::PageDown => "PageDown",
            Keycode::Up => "Up",
            Keycode::Down => "Down",
            Keycode::Left => "Left",
            Keycode::Right => "Right",

            // Punctuation
            Keycode::Minus => "-",
            Keycode::Equal => "=",
            Keycode::LeftBracket => "[",
            Keycode::RightBracket => "]",
            Keycode::BackSlash => "\\",
            Keycode::Semicolon => ";",
            Keycode::Apostrophe => "'",
            Keycode::Grave => "`",
            Keycode::Comma => ",",
            Keycode::Dot => ".",
            Keycode::Slash => "/",

            // Skip modifiers and unrecognised keys
            _ => return None,
        }
        .to_string(),
    )
}

/// Convert keycode to display string
fn keycode_to_display(key: &Keycode) -> String {
    match key {
        // Use symbols for special keys on macOS
        Keycode::Space => "Space".to_string(),
        Keycode::Enter => "Return".to_string(),
        Keycode::Tab => "Tab".to_string(),
        Keycode::Backspace => "Delete".to_string(),
        Keycode::Delete => "Del".to_string(),
        Keycode::Up => "↑".to_string(),
        Keycode::Down => "↓".to_string(),
        Keycode::Left => "←".to_string(),
        Keycode::Right => "→".to_string(),
        // For most keys, use the accelerator name
        _ => keycode_to_accelerator(key).unwrap_or_else(|| format!("{:?}", key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_keys_cmd_y() {
        let mut keys = HashSet::new();
        keys.insert(Keycode::LMeta);
        keys.insert(Keycode::Y);

        let (accelerator, _, is_valid) = format_keys(&keys);
        assert_eq!(accelerator, "CommandOrControl+Y");
        assert!(is_valid);
    }

    #[test]
    fn test_format_keys_modifier_only() {
        let mut keys = HashSet::new();
        keys.insert(Keycode::LMeta);

        let (_, _, is_valid) = format_keys(&keys);
        assert!(!is_valid);
    }

    #[test]
    fn test_format_keys_function_key_alone() {
        let mut keys = HashSet::new();
        keys.insert(Keycode::F13);

        let (accelerator, _, is_valid) = format_keys(&keys);
        assert_eq!(accelerator, "F13");
        assert!(is_valid);
    }

    #[test]
    fn test_format_keys_single_letter() {
        let mut keys = HashSet::new();
        keys.insert(Keycode::H);

        let (accelerator, _, is_valid) = format_keys(&keys);
        assert_eq!(accelerator, "H");
        assert!(is_valid); // Single letter keys are now valid
    }

    #[test]
    fn test_format_keys_right_shift_alone() {
        let mut keys = HashSet::new();
        keys.insert(Keycode::RShift);

        let (accelerator, key_names, is_valid) = format_keys(&keys);
        assert_eq!(accelerator, "ShiftRight");
        assert_eq!(key_names, vec!["Right Shift"]);
        assert!(is_valid); // Right modifiers can be standalone keys
    }

    #[test]
    fn test_format_keys_right_option_alone() {
        let mut keys = HashSet::new();
        keys.insert(Keycode::RAlt);

        let (accelerator, key_names, is_valid) = format_keys(&keys);
        assert_eq!(accelerator, "AltRight");
        assert_eq!(key_names, vec!["Right Option"]);
        assert!(is_valid);
    }

    #[test]
    fn test_format_keys_left_shift_alone() {
        // Left modifiers alone should NOT be valid
        let mut keys = HashSet::new();
        keys.insert(Keycode::LShift);

        let (_, _, is_valid) = format_keys(&keys);
        assert!(!is_valid);
    }
}
