//! Shortcut conflict detection for Thoth
//!
//! Provides conflict detection, alternative suggestions, and graceful
//! fallback when shortcut registration fails.

use serde::{Deserialize, Serialize};

/// Information about a detected shortcut conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutConflict {
    /// The shortcut that failed to register
    pub shortcut: String,
    /// The shortcut ID that was being registered
    pub shortcut_id: String,
    /// Human-readable reason for the conflict
    pub reason: String,
    /// Suggested alternative shortcuts
    pub suggestions: Vec<String>,
}

/// Result of attempting to register a shortcut with conflict detection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RegistrationResult {
    /// Shortcut was registered successfully
    Success {
        shortcut: String,
        shortcut_id: String,
    },
    /// Shortcut registration failed due to conflict
    Conflict(ShortcutConflict),
}

/// Common modifier key combinations
const MODIFIER_COMBOS: &[&str] = &[
    "CommandOrControl+Shift",
    "CommandOrControl+Alt",
    "CommandOrControl+Shift+Alt",
    "Alt+Shift",
    "Ctrl+Shift",
    "Ctrl+Alt",
];

/// Common single-key shortcuts (function keys, etc.)
const SINGLE_KEYS: &[&str] = &["F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20"];

/// Common base keys for modifier combinations
const BASE_KEYS: &[&str] = &["Space", "R", "T", "M", "J", "K", "L", "Semicolon"];

/// Generate alternative shortcut suggestions for a failed registration
///
/// Generates a list of alternative shortcuts that the user might try
/// when their preferred shortcut is not available.
///
/// # Arguments
/// * `failed_shortcut` - The shortcut that failed to register
///
/// # Returns
/// A vector of suggested alternative shortcuts
pub fn suggest_alternatives(failed_shortcut: &str) -> Vec<String> {
    let mut suggestions = Vec::new();
    let failed_lower = failed_shortcut.to_lowercase();

    // If it's a function key, suggest other function keys
    if failed_lower.starts_with('f') && failed_lower.len() <= 3 {
        for key in SINGLE_KEYS {
            if key.to_lowercase() != failed_lower {
                suggestions.push(key.to_string());
                if suggestions.len() >= 3 {
                    break;
                }
            }
        }
    }

    // If it's a modifier combo, suggest variations
    if failed_shortcut.contains('+') {
        // Extract the base key
        let parts: Vec<&str> = failed_shortcut.split('+').collect();
        if let Some(base_key) = parts.last() {
            // Suggest same key with different modifiers
            for modifier in MODIFIER_COMBOS {
                let suggestion = format!("{}+{}", modifier, base_key);
                if suggestion != failed_shortcut && !suggestions.contains(&suggestion) {
                    suggestions.push(suggestion);
                    if suggestions.len() >= 3 {
                        break;
                    }
                }
            }
        }

        // If we still need more suggestions, try different base keys
        if suggestions.len() < 3 {
            // Extract the modifiers
            let modifier_part = if parts.len() > 1 {
                parts[..parts.len() - 1].join("+")
            } else {
                "CommandOrControl+Shift".to_string()
            };

            for base_key in BASE_KEYS {
                let suggestion = format!("{}+{}", modifier_part, base_key);
                if suggestion != failed_shortcut && !suggestions.contains(&suggestion) {
                    suggestions.push(suggestion);
                    if suggestions.len() >= 5 {
                        break;
                    }
                }
            }
        }
    }

    // Always include some function key suggestions if we have room
    if suggestions.len() < 5 {
        for key in SINGLE_KEYS {
            if key.to_lowercase() != failed_lower && !suggestions.contains(&key.to_string()) {
                suggestions.push(key.to_string());
                if suggestions.len() >= 5 {
                    break;
                }
            }
        }
    }

    suggestions
}

/// Classify the type of registration error
///
/// # Arguments
/// * `error_message` - The error message from the registration attempt
///
/// # Returns
/// A user-friendly description of what went wrong
pub fn classify_error(error_message: &str) -> String {
    let error_lower = error_message.to_lowercase();

    if error_lower.contains("already registered") || error_lower.contains("in use") {
        "This shortcut is already registered by another application.".to_string()
    } else if error_lower.contains("invalid") || error_lower.contains("parse") {
        "This shortcut format is not recognised. Please use a valid key combination.".to_string()
    } else if error_lower.contains("permission") || error_lower.contains("access") {
        "Accessibility permission is required to register global shortcuts.".to_string()
    } else if error_lower.contains("reserved") || error_lower.contains("system") {
        "This shortcut is reserved by the operating system.".to_string()
    } else {
        format!(
            "Failed to register shortcut. The system reported: {}",
            error_message
        )
    }
}

/// Create a conflict result from a registration error
///
/// # Arguments
/// * `shortcut` - The shortcut that failed to register
/// * `shortcut_id` - The ID of the shortcut being registered
/// * `error` - The error message from the registration attempt
///
/// # Returns
/// A `ShortcutConflict` with suggestions and user-friendly messaging
pub fn create_conflict(shortcut: &str, shortcut_id: &str, error: &str) -> ShortcutConflict {
    ShortcutConflict {
        shortcut: shortcut.to_string(),
        shortcut_id: shortcut_id.to_string(),
        reason: classify_error(error),
        suggestions: suggest_alternatives(shortcut),
    }
}

/// Validate a shortcut string format before attempting registration
///
/// # Arguments
/// * `shortcut` - The shortcut string to validate
///
/// # Returns
/// * `Ok(())` if the format appears valid
/// * `Err(String)` with a description of the format issue
pub fn validate_shortcut_format(shortcut: &str) -> Result<(), String> {
    if shortcut.is_empty() {
        return Err("Shortcut cannot be empty.".to_string());
    }

    // Check for valid characters
    let valid_chars = shortcut
        .chars()
        .all(|c| c.is_alphanumeric() || c == '+' || c == '-' || c == '_');
    if !valid_chars {
        return Err("Shortcut contains invalid characters.".to_string());
    }

    // Check that it doesn't start or end with +
    if shortcut.starts_with('+') || shortcut.ends_with('+') {
        return Err("Shortcut cannot start or end with '+'.".to_string());
    }

    // Check for empty parts
    if shortcut.contains("++") {
        return Err("Shortcut contains empty key parts.".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suggest_alternatives_for_function_key() {
        let suggestions = suggest_alternatives("F13");
        assert!(!suggestions.is_empty());
        assert!(!suggestions.contains(&"F13".to_string()));
        assert!(
            suggestions.contains(&"F14".to_string()) || suggestions.contains(&"F15".to_string())
        );
    }

    #[test]
    fn test_suggest_alternatives_for_modifier_combo() {
        let suggestions = suggest_alternatives("CommandOrControl+Shift+Space");
        assert!(!suggestions.is_empty());
        // Should suggest different modifier combinations or different keys
        assert!(suggestions.len() >= 2);
    }

    #[test]
    fn test_classify_error_already_registered() {
        let reason = classify_error("Shortcut already registered");
        assert!(reason.contains("already registered"));
    }

    #[test]
    fn test_classify_error_permission() {
        let reason = classify_error("Permission denied");
        assert!(reason.contains("Accessibility permission"));
    }

    #[test]
    fn test_classify_error_generic() {
        let reason = classify_error("Unknown error occurred");
        assert!(reason.contains("system reported"));
    }

    #[test]
    fn test_validate_shortcut_format_valid() {
        assert!(validate_shortcut_format("F13").is_ok());
        assert!(validate_shortcut_format("CommandOrControl+Shift+Space").is_ok());
        assert!(validate_shortcut_format("Alt+R").is_ok());
    }

    #[test]
    fn test_validate_shortcut_format_empty() {
        let result = validate_shortcut_format("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_validate_shortcut_format_invalid_chars() {
        let result = validate_shortcut_format("Ctrl+@");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_shortcut_format_plus_at_edges() {
        assert!(validate_shortcut_format("+Space").is_err());
        assert!(validate_shortcut_format("Space+").is_err());
    }

    #[test]
    fn test_create_conflict() {
        let conflict = create_conflict("F13", "toggle_recording", "Shortcut already registered");
        assert_eq!(conflict.shortcut, "F13");
        assert_eq!(conflict.shortcut_id, "toggle_recording");
        assert!(conflict.reason.contains("already registered"));
        assert!(!conflict.suggestions.is_empty());
    }
}
