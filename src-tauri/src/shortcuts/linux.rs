//! Linux-specific shortcut implementation
//!
//! Handles global shortcuts on Linux with support for both X11 and Wayland:
//! - X11: Uses Tauri's GlobalShortcut plugin (works via X11 grab)
//! - Wayland: Attempts to use Tauri's plugin (may have partial support)
//!
//! The module automatically detects the display server at runtime and
//! provides appropriate warnings if shortcuts may not work.

use tauri::{AppHandle, Runtime};

use super::manager::{self, ShortcutInfo};
use super::portal;

/// Detected display server type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    /// X11 display server (Xorg)
    X11,
    /// Wayland compositor
    Wayland,
    /// Unknown or unable to detect
    Unknown,
}

impl std::fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayServer::X11 => write!(f, "X11"),
            DisplayServer::Wayland => write!(f, "Wayland"),
            DisplayServer::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Detect the current display server
///
/// Checks environment variables and XDG session type to determine
/// whether we're running on X11 or Wayland.
pub fn detect_display_server() -> DisplayServer {
    // Check XDG_SESSION_TYPE first (most reliable)
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        match session_type.to_lowercase().as_str() {
            "wayland" => {
                tracing::info!("Detected Wayland session via XDG_SESSION_TYPE");
                return DisplayServer::Wayland;
            }
            "x11" | "xorg" => {
                tracing::info!("Detected X11 session via XDG_SESSION_TYPE");
                return DisplayServer::X11;
            }
            _ => {}
        }
    }

    // Check WAYLAND_DISPLAY (Wayland-specific)
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        tracing::info!("Detected Wayland via WAYLAND_DISPLAY");
        return DisplayServer::Wayland;
    }

    // Check DISPLAY (X11-specific)
    if std::env::var("DISPLAY").is_ok() {
        tracing::info!("Detected X11 via DISPLAY");
        return DisplayServer::X11;
    }

    tracing::warn!("Could not detect display server type");
    DisplayServer::Unknown
}

/// Cached display server detection
static DISPLAY_SERVER: std::sync::OnceLock<DisplayServer> = std::sync::OnceLock::new();

/// Get the detected display server (cached after first call)
pub fn get_display_server() -> DisplayServer {
    *DISPLAY_SERVER.get_or_init(detect_display_server)
}

/// Register a shortcut on Linux
///
/// On Wayland, uses XDG Desktop Portal GlobalShortcuts.
/// On X11, uses Tauri's GlobalShortcut plugin.
pub fn register<R: Runtime>(
    app: &AppHandle<R>,
    id: String,
    accelerator: String,
    description: String,
) -> Result<(), String> {
    let display_server = get_display_server();
    tracing::info!("Registering Linux shortcut '{}' on {}", id, display_server);

    match display_server {
        DisplayServer::Wayland => portal::register(id, accelerator, description),
        _ => manager::register(app, id, accelerator, description),
    }
}

/// Unregister a shortcut on Linux
pub fn unregister<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    match get_display_server() {
        DisplayServer::Wayland => portal::unregister(id),
        _ => manager::unregister(app, id),
    }
}

/// List all registered shortcuts on Linux
pub fn list_registered() -> Vec<ShortcutInfo> {
    match get_display_server() {
        DisplayServer::Wayland => portal::list_registered(),
        _ => manager::list_registered(),
    }
}

/// Unregister all shortcuts on Linux
pub fn unregister_all<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    match get_display_server() {
        DisplayServer::Wayland => portal::unregister_all(),
        _ => manager::unregister_all(app),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_server_detection() {
        // This test just ensures the detection doesn't panic
        let server = detect_display_server();
        tracing::info!("Detected display server: {:?}", server);
    }

    #[test]
    fn test_get_display_server_cached() {
        let server1 = get_display_server();
        let server2 = get_display_server();
        assert_eq!(server1, server2);
    }
}
