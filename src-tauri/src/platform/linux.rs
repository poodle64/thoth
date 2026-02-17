//! Linux-specific platform functionality
//!
//! Provides Linux implementations for platform-specific features:
//! - Microphone access via PulseAudio/PipeWire
//! - Accessibility (not needed on Linux, kept for API compatibility)

use std::process::Command;

/// Microphone authorization status values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophoneStatus {
    /// Microphone is available
    Granted,
    /// No microphone found or access denied
    Denied,
    /// Unable to determine status
    Unknown,
}

impl std::fmt::Display for MicrophoneStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MicrophoneStatus::Granted => write!(f, "granted"),
            MicrophoneStatus::Denied => write!(f, "denied"),
            MicrophoneStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Check if accessibility permission is granted
///
/// On Linux, accessibility permissions are not typically required
/// for global shortcuts. X11 allows key grabbing by default.
/// On Wayland, global shortcuts require portal support.
pub fn check_accessibility_permission() -> bool {
    // On Linux, we don't have a central accessibility permission system
    // like macOS. X11 allows key grabbing by default.
    // For Wayland, the XDG Desktop Portal handles this.
    true
}

/// Open accessibility settings
///
/// On Linux, no accessibility settings need to be opened.
/// This is kept for API compatibility with macOS.
pub fn open_accessibility_settings() {
    tracing::debug!("No accessibility settings needed on Linux");
}

/// Check microphone permission status
///
/// On Linux, microphone access is managed by PulseAudio or PipeWire.
/// We check if a default audio source (microphone) is available.
pub fn check_microphone_permission() -> MicrophoneStatus {
    // Try to check PulseAudio/PipeWire for available sources
    // Use pactl to list sources

    let output = Command::new("pactl")
        .args(["list", "short", "sources"])
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Check if there are any input sources (microphones)
                // Each line represents a source, we look for non-empty output
                let has_sources = stdout
                    .lines()
                    .any(|line| !line.trim().is_empty() && line.contains("input"));

                if has_sources {
                    tracing::debug!("Microphone available via PulseAudio/PipeWire");
                    MicrophoneStatus::Granted
                } else {
                    tracing::warn!("No microphone sources found");
                    MicrophoneStatus::Denied
                }
            } else {
                tracing::warn!("pactl command failed, trying pipewire...");
                // Try PipeWire directly if pactl fails
                check_pipewire_microphone()
            }
        }
        Err(e) => {
            tracing::warn!("pactl not available: {}, trying pipewire...", e);
            check_pipewire_microphone()
        }
    }
}

/// Check microphone via PipeWire's pw-cli
fn check_pipewire_microphone() -> MicrophoneStatus {
    let output = Command::new("pw-cli")
        .args(["list-objects"])
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Look for audio capture devices in pw-cli output
                let has_capture = stdout.contains("Audio/Source") || stdout.contains("capture");

                if has_capture {
                    tracing::debug!("Microphone available via PipeWire");
                    MicrophoneStatus::Granted
                } else {
                    tracing::warn!("No PipeWire capture devices found");
                    MicrophoneStatus::Denied
                }
            } else {
                tracing::warn!("pw-cli command failed");
                // Assume granted if we can't check - let the audio system handle it
                MicrophoneStatus::Unknown
            }
        }
        Err(e) => {
            tracing::warn!("pw-cli not available: {}", e);
            // Neither pactl nor pw-cli available - assume unknown
            // The actual audio capture will fail if there's no microphone
            MicrophoneStatus::Unknown
        }
    }
}

/// Request microphone permission
///
/// On Linux, microphone access is typically granted automatically by
/// PulseAudio/PipeWire. There's no system permission dialog like macOS.
/// This function is kept for API compatibility.
pub fn request_microphone_permission() {
    tracing::info!("Linux does not require explicit microphone permission");
    // Linux doesn't have a permission dialog system like macOS
    // Access is managed by PulseAudio/PipeWire which typically allows by default
}

/// Open system sound settings
///
/// Opens the desktop environment's sound settings panel.
pub fn open_microphone_settings() {
    // Try common desktop environment sound settings
    let commands = [
        // GNOME
        ("gnome-control-center", vec!["sound"]),
        // KDE
        ("systemsettings", vec!["kcm_pulseaudio"]),
        // Generic freedesktop
        ("xdg-open", vec!["settings://sound"]),
        // pavucontrol (PulseAudio volume control)
        ("pavucontrol", vec![]),
    ];

    for (cmd, args) in commands {
        let result = if args.is_empty() {
            Command::new(cmd).spawn()
        } else {
            Command::new(cmd).args(&args).spawn()
        };

        if result.is_ok() {
            tracing::info!("Opened sound settings via {}", cmd);
            return;
        }
    }

    tracing::warn!("Could not open sound settings - no supported settings app found");
}

/// Position of the text caret (insertion point) on screen
#[derive(Debug, Clone, Copy)]
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
/// On Linux, this requires AT-SPI2 (Assistive Technology Service Provider Interface).
/// This is a complex feature that requires:
/// 1. AT-SPI2 to be running
/// 2. The application to expose caret position via accessibility APIs
/// 3. Proper permissions to access accessibility bus
///
/// For now, this returns None as implementing AT-SPI2 support is complex.
pub fn get_caret_position() -> Option<CaretPosition> {
    // AT-SPI2 implementation would require the atspi crate
    // and proper D-Bus connection to the accessibility bus.
    // This is a significant undertaking and may not work reliably
    // across all applications and desktop environments.

    tracing::debug!("Caret position detection not implemented on Linux");
    None
}

/// Check if Input Monitoring permission is granted
///
/// On Linux, there's no equivalent to macOS Input Monitoring permission.
/// Keyboard access depends on X11/Wayland security model.
/// On X11, applications can grab keys by default.
/// On Wayland, global shortcuts require XDG Desktop Portal support.
pub fn check_input_monitoring_permission() -> bool {
    // No equivalent permission on Linux
    true
}

/// Open Input Monitoring settings
///
/// Not applicable on Linux. Logs a debug message.
pub fn open_input_monitoring_settings() {
    tracing::debug!("No input monitoring settings on Linux");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_accessibility_permission() {
        // Should always return true on Linux
        assert!(check_accessibility_permission());
    }

    #[test]
    fn test_check_microphone_permission() {
        // Just ensure it doesn't panic
        let status = check_microphone_permission();
        tracing::info!("Microphone status: {:?}", status);
    }

    #[test]
    fn test_check_input_monitoring() {
        assert!(check_input_monitoring_permission());
    }
}
