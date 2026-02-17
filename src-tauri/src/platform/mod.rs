//! Platform-specific functionality

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

/// Check if accessibility permissions are available
#[tauri::command]
pub fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::check_accessibility_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true // Not needed on other platforms
    }
}

/// Request accessibility permission (opens settings if needed)
#[tauri::command]
pub fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        if !macos::check_accessibility_permission() {
            macos::open_accessibility_settings();
            false
        } else {
            true
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Check if Input Monitoring permission is granted
///
/// This is required for capturing keyboard input at the system level.
pub fn check_input_monitoring_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::check_input_monitoring_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true // Not needed on other platforms
    }
}

/// Open Input Monitoring settings
pub fn open_input_monitoring_settings() {
    #[cfg(target_os = "macos")]
    {
        macos::open_input_monitoring_settings();
    }
}

/// Check microphone permission status
///
/// Returns the permission status as a string:
/// - "granted" - Permission has been granted
/// - "denied" - Permission was explicitly denied
/// - "not_determined" - User hasn't been asked yet
/// - "restricted" - Access is restricted (e.g., parental controls)
/// - "unknown" - Unable to determine status
#[tauri::command]
pub fn check_microphone_permission() -> String {
    #[cfg(target_os = "macos")]
    {
        macos::check_microphone_permission().to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "granted".to_string() // Not needed on other platforms
    }
}

/// Caret (text cursor) position on screen
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct CaretPosition {
    /// X coordinate in screen pixels
    pub x: f64,
    /// Y coordinate in screen pixels
    pub y: f64,
    /// Height of the caret/text line in pixels
    pub height: f64,
}

/// Get the position of the text caret in the currently focused application
///
/// Uses platform accessibility APIs to find the focused text element and get
/// the position of the text insertion point. This is used to position the
/// recording indicator near where text will be inserted (like macOS dictation).
///
/// Returns None if no text field is focused or caret position cannot be determined.
pub fn get_caret_position() -> Option<CaretPosition> {
    #[cfg(target_os = "macos")]
    {
        macos::get_caret_position().map(|p| CaretPosition {
            x: p.x,
            y: p.y,
            height: p.height,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None // Not implemented on other platforms yet
    }
}

/// Request microphone permission
///
/// Triggers the system permission dialog. If permission was already denied,
/// this will open System Preferences instead.
#[tauri::command]
pub fn request_microphone_permission() {
    #[cfg(target_os = "macos")]
    {
        let status = macos::check_microphone_permission();
        match status {
            macos::MicrophoneStatus::NotDetermined => {
                // First time - trigger the system dialog
                macos::request_microphone_permission();
            }
            macos::MicrophoneStatus::Denied | macos::MicrophoneStatus::Restricted => {
                // Already denied - open System Preferences
                macos::open_microphone_settings();
            }
            macos::MicrophoneStatus::Authorized => {
                // Already granted, nothing to do
                tracing::info!("Microphone permission already granted");
            }
            macos::MicrophoneStatus::Unknown => {
                // Try requesting anyway
                macos::request_microphone_permission();
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Nothing needed on other platforms
    }
}
