//! Tauri command handlers
//!
//! This module contains all IPC commands that can be invoked from the frontend.

use tauri::{AppHandle, Manager};

/// Greet command for testing
#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Thoth.", name)
}

/// Show a window by label
#[tauri::command]
pub fn show_window(app: AppHandle, label: &str) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(label) {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        tracing::info!("Showed window: {}", label);
        Ok(())
    } else {
        Err(format!("Window '{}' not found", label))
    }
}

/// Hide a window by label
#[tauri::command]
pub fn hide_window(app: AppHandle, label: &str) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(label) {
        window.hide().map_err(|e| e.to_string())?;
        tracing::info!("Hid window: {}", label);
        Ok(())
    } else {
        Err(format!("Window '{}' not found", label))
    }
}

/// Open a URL in the system's default browser
#[tauri::command]
pub fn open_url(url: &str) -> Result<(), String> {
    // Only allow http/https URLs for security
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Only http:// and https:// URLs are allowed".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        // Empty title ("") prevents cmd from treating URLs with special chars as commands
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    Ok(())
}

/// Set dock icon visibility (macOS) and persist to config
#[tauri::command]
pub fn set_show_in_dock(app: AppHandle, show: bool) -> Result<(), String> {
    // Update config
    let mut config =
        crate::config::get_config().map_err(|e| format!("Failed to load config: {}", e))?;
    config.general.show_in_dock = show;
    crate::config::set_config(config).map_err(|e| format!("Failed to save config: {}", e))?;

    // Apply immediately on macOS
    #[cfg(target_os = "macos")]
    {
        let policy = if show {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        app.set_activation_policy(policy)
            .map_err(|e| format!("Failed to set activation policy: {}", e))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app; // Suppress unused warning on non-macOS
    }

    tracing::info!("Dock visibility set to: {}", show);
    Ok(())
}

/// Get current dock visibility setting
#[tauri::command]
pub fn get_show_in_dock() -> bool {
    crate::config::get_config()
        .map(|c| c.general.show_in_dock)
        .unwrap_or(false)
}

/// Set the audio input device and persist to config
///
/// Uses a dedicated command (rather than full config save) to prevent
/// the device_id from being accidentally overwritten by other config saves.
#[tauri::command]
pub fn set_audio_device(device_id: Option<String>) -> Result<(), String> {
    crate::config::set_audio_device_config(device_id.clone())
        .map_err(|e| format!("Failed to save audio device: {}", e))?;
    tracing::info!("Audio device set to: {:?}", device_id);
    Ok(())
}

/// Get the current audio device setting
#[tauri::command]
pub fn get_audio_device() -> Option<String> {
    crate::config::get_config()
        .map(|c| c.audio.device_id)
        .unwrap_or(None)
}

/// Toggle window visibility
#[tauri::command]
pub fn toggle_window(app: AppHandle, label: &str) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window(label) {
        let visible = window.is_visible().map_err(|e| e.to_string())?;
        if visible {
            window.hide().map_err(|e| e.to_string())?;
            tracing::info!("Hid window: {}", label);
            Ok(false)
        } else {
            window.show().map_err(|e| e.to_string())?;
            window.set_focus().map_err(|e| e.to_string())?;
            tracing::info!("Showed window: {}", label);
            Ok(true)
        }
    } else {
        Err(format!("Window '{}' not found", label))
    }
}
