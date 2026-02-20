//! Recording indicator window management
//!
//! Handles showing/hiding the floating recording indicator pill near the text
//! cursor (caret), similar to macOS dictation. Falls back to bottom-centre of
//! the main window's monitor if caret position cannot be determined.
//!
//! The indicator window is pre-warmed at app startup to eliminate any delay
//! when showing it for the first time.
//!
//! Can be disabled via config (general.show_recording_indicator) for users
//! who prefer no visual indicator (e.g., tiling window manager users).
//!
//! Note: On Wayland, mouse tracking and precise window positioning don't work
//! reliably due to Wayland's security model. Users may want to disable the
//! indicator on Wayland.

use crate::config;
use crate::mouse_tracker;
use tauri::{AppHandle, LogicalPosition, Manager, Runtime, WebviewWindow};

/// Label for the recording indicator window (must match tauri.conf.json)
const INDICATOR_WINDOW_LABEL: &str = "recording-indicator";

/// Indicator dimensions in logical pixels (must match frontend)
const PILL_WIDTH: f64 = 58.0;
const PILL_HEIGHT: f64 = 58.0;

/// Fallback: padding from bottom of screen (above dock)
const BOTTOM_PADDING: f64 = 120.0;

/// Check if we're running on Wayland (where indicator positioning won't work well)
#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    // Check XDG_SESSION_TYPE first (most reliable)
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        if session_type.to_lowercase() == "wayland" {
            return true;
        }
    }
    // Also check WAYLAND_DISPLAY
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

#[cfg(not(target_os = "linux"))]
fn is_wayland() -> bool {
    false
}

/// Get the recording indicator window
fn get_indicator_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(INDICATOR_WINDOW_LABEL)
}

/// Get the recording indicator window (generic version for use from shortcut handler)
fn get_indicator_window_generic<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<tauri::WebviewWindow<R>> {
    app.get_webview_window(INDICATOR_WINDOW_LABEL)
}

/// Find the monitor containing a point (in logical pixels).
///
/// Returns `(mon_x, mon_y, mon_width, mon_height, scale_factor)` in logical pixels.
pub(crate) fn find_monitor_for_point(
    app: &AppHandle,
    x: f64,
    y: f64,
) -> Option<(f64, f64, f64, f64, f64)> {
    // Get all monitors - try main window first, then any other window
    let monitors = app
        .get_webview_window("main")
        .and_then(|w| w.available_monitors().ok())
        .or_else(|| {
            app.get_webview_window(INDICATOR_WINDOW_LABEL)
                .and_then(|w| w.available_monitors().ok())
        })?;

    for monitor in monitors {
        let scale_factor = monitor.scale_factor();
        let pos = monitor.position();
        let size = monitor.size();

        // Convert to logical pixels
        let mon_x = pos.x as f64 / scale_factor;
        let mon_y = pos.y as f64 / scale_factor;
        let mon_width = size.width as f64 / scale_factor;
        let mon_height = size.height as f64 / scale_factor;

        // Check if point is within this monitor
        if x >= mon_x && x < mon_x + mon_width && y >= mon_y && y < mon_y + mon_height {
            return Some((mon_x, mon_y, mon_width, mon_height, scale_factor));
        }
    }

    None
}

/// Shared logic for showing the recording indicator.
///
/// Checks config, warns on Wayland, gets the window, shows on Linux first,
/// positions at cursor (using `fallback_position` when mouse position is
/// unavailable), shows on macOS, and starts mouse tracking.
fn show_indicator_common<F>(app: &AppHandle, fallback_position: F) -> Result<(), String>
where
    F: FnOnce(&AppHandle, &WebviewWindow) -> Result<(), String>,
{
    let show_indicator = config::get_config()
        .map(|c| c.general.show_recording_indicator)
        .unwrap_or(true);

    if !show_indicator {
        tracing::info!("Recording indicator disabled in config, skipping show");
        return Ok(());
    }

    if is_wayland() {
        tracing::warn!("Running on Wayland - indicator positioning may not work correctly");
    }

    let indicator = get_indicator_window(app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // On Linux, show before positioning to ensure the window is mapped
    #[cfg(target_os = "linux")]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    // Position at cursor or use the provided fallback
    if let Some((x, y)) = mouse_tracker::get_initial_position() {
        indicator
            .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
            .map_err(|e| e.to_string())?;
        tracing::debug!("Recording indicator positioned at cursor ({}, {})", x, y);
    } else {
        tracing::info!("No mouse position available, using fallback position");
        fallback_position(app, &indicator)?;
    }

    // On macOS, show after positioning
    #[cfg(not(target_os = "linux"))]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    mouse_tracker::start_tracking();
    Ok(())
}

/// Position the indicator at the primary monitor's centre-bottom.
///
/// Used as fallback when mouse position is unavailable in the generic
/// (`show_indicator_instant`) code path.
fn position_at_primary_monitor<R: Runtime>(indicator: &tauri::WebviewWindow<R>) {
    if let Ok(Some(monitor)) = indicator.primary_monitor() {
        let scale = monitor.scale_factor();
        let pos = monitor.position();
        let size = monitor.size();
        let mx = pos.x as f64 / scale;
        let my = pos.y as f64 / scale;
        let mw = size.width as f64 / scale;
        let mh = size.height as f64 / scale;
        let x = mx + (mw / 2.0) - (PILL_WIDTH / 2.0);
        let y = my + mh - PILL_HEIGHT - BOTTOM_PADDING;
        let _ = indicator.set_position(tauri::Position::Logical(LogicalPosition::new(x, y)));
    }
}

/// Show the recording indicator window at the current cursor position.
///
/// Positions the indicator near the mouse cursor and starts cursor-following
/// tracking. Falls back to bottom-centre of the main window's monitor if
/// the mouse position cannot be determined.
///
/// Returns silently if the recording indicator is disabled in config.
#[tauri::command]
pub fn show_recording_indicator(app: AppHandle) -> Result<(), String> {
    tracing::info!("show_recording_indicator called");
    show_indicator_common(&app, |app_handle, indicator| {
        position_at_bottom_centre(app_handle, indicator)
    })
}

/// Position the indicator at the bottom centre of the main window's monitor
fn position_at_bottom_centre(app: &AppHandle, indicator: &WebviewWindow) -> Result<(), String> {
    // Get monitor for positioning - try main window first, then indicator window, then primary monitor
    let monitor = if let Some(main_window) = app.get_webview_window("main") {
        main_window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| indicator.current_monitor().ok().flatten())
            .or_else(|| indicator.primary_monitor().ok().flatten())
    } else {
        // Main window not available, use indicator window or primary monitor
        indicator
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| indicator.primary_monitor().ok().flatten())
    }
    .ok_or_else(|| "Could not determine current monitor".to_string())?;

    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();
    let scale_factor = monitor.scale_factor();

    // Convert to logical pixels
    let monitor_x = monitor_pos.x as f64 / scale_factor;
    let monitor_y = monitor_pos.y as f64 / scale_factor;
    let monitor_width = monitor_size.width as f64 / scale_factor;
    let monitor_height = monitor_size.height as f64 / scale_factor;

    tracing::info!(
        "Monitor: pos=({}, {}), size={}x{}, scale={}",
        monitor_x,
        monitor_y,
        monitor_width,
        monitor_height,
        scale_factor
    );

    // Calculate centre-bottom position
    let x = monitor_x + (monitor_width / 2.0) - (PILL_WIDTH / 2.0);
    let y = monitor_y + monitor_height - PILL_HEIGHT - BOTTOM_PADDING;

    tracing::info!("Setting indicator position to ({}, {})", x, y);

    indicator
        .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Hide the recording indicator window by moving it off-screen.
///
/// We move off-screen instead of using hide() to avoid macOS window
/// show/hide animation delays - this makes subsequent shows instant.
///
/// On Linux/Wayland, we use actual hide() since the window has been mapped
/// during pre-warm. Positioning doesn't work (compositor controls it).
#[tauri::command]
pub fn hide_recording_indicator(app: AppHandle) -> Result<(), String> {
    tracing::info!("hide_recording_indicator called");

    // Stop mouse tracking before hiding
    mouse_tracker::stop_tracking();

    let window = get_indicator_window(&app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // On Linux, use actual hide() - this should work now that the window
    // has been mapped during pre-warm.
    #[cfg(target_os = "linux")]
    {
        window.hide().map_err(|e| e.to_string())?;
        tracing::info!("Recording indicator hidden (Linux)");
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Move off-screen instead of hiding - this avoids animation delays
        window
            .set_position(tauri::Position::Logical(LogicalPosition::new(
                -10000.0, -10000.0,
            )))
            .map_err(|e| e.to_string())?;
        tracing::info!("Recording indicator moved off-screen (hidden)");
    }

    Ok(())
}

/// Show the recording indicator immediately (generic version for shortcut handler).
///
/// Positions the indicator at the current mouse cursor position and starts
/// cursor-following tracking. Skips the caret position lookup for instant response.
///
/// The window is kept always-visible but positioned off-screen when "hidden"
/// to avoid macOS window show/hide animation delays.
///
/// On Linux/Wayland, uses actual show()/hide() since compositor controls positioning.
///
/// Returns silently if the recording indicator is disabled in config.
pub fn show_indicator_instant<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    tracing::info!("show_indicator_instant called (fast path)");

    // Delegate to the AppHandle (non-generic) path. The generic version
    // exists for API compatibility; internally we use the concrete type.
    // On macOS (the only current platform using Runtime generics), AppHandle
    // and AppHandle<R> both resolve to the same Tauri runtime.
    let indicator = get_indicator_window_generic(app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // Check config
    let show_indicator = config::get_config()
        .map(|c| c.general.show_recording_indicator)
        .unwrap_or(true);

    if !show_indicator {
        tracing::info!("Recording indicator disabled in config, skipping instant show");
        return Ok(());
    }

    if is_wayland() {
        tracing::warn!("Running on Wayland - indicator positioning may not work correctly");
    }

    // On Linux, show before positioning to ensure the window is mapped
    #[cfg(target_os = "linux")]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    // Position at cursor or fall back to primary monitor centre-bottom
    if let Some((x, y)) = mouse_tracker::get_initial_position() {
        indicator
            .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
            .map_err(|e| e.to_string())?;
        tracing::debug!("Recording indicator positioned at cursor ({}, {})", x, y);
    } else {
        position_at_primary_monitor(&indicator);
        tracing::debug!("Recording indicator positioned at fallback (bottom-centre)");
    }

    // On macOS, show after positioning
    #[cfg(not(target_os = "linux"))]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    mouse_tracker::start_tracking();
    Ok(())
}

/// Pre-warm the recording indicator window by loading its content.
///
/// This eliminates the delay on first show by ensuring the webview
/// content is fully loaded and rendered before the user triggers recording.
/// Should be called during app startup.
///
/// On macOS, the window is left visible but off-screen - we never hide() it
/// to avoid show/hide animation delays.
///
/// On Linux/Wayland, we show then hide the window during pre-warm to ensure
/// it's properly mapped with the compositor. Subsequent hide/show should work.
pub fn prewarm_indicator_window(app: &AppHandle) {
    // On Wayland, warn that the indicator may not work well
    if is_wayland() {
        tracing::info!("Running on Wayland - recording indicator positioning won't work (compositor controls it). Users can disable in settings.");
    }

    let app_handle = app.clone();

    // Spawn async task to pre-warm without blocking startup
    tauri::async_runtime::spawn(async move {
        // Small delay to let the main window initialise first
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        if let Some(window) = app_handle.get_webview_window(INDICATOR_WINDOW_LABEL) {
            tracing::info!("Pre-warming recording indicator window");

            // On Linux/Wayland, show then hide to map the window with compositor.
            // This ensures subsequent show() calls will work.
            #[cfg(target_os = "linux")]
            {
                tracing::info!("Pre-warming indicator for Linux/Wayland - showing then hiding");

                // Show the window to register it with the compositor
                if let Err(e) = window.show() {
                    tracing::warn!("Failed to show indicator for pre-warming: {}", e);
                }

                // Wait for compositor to acknowledge
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                // Now hide it - since it's been mapped, show() should work later
                if let Err(e) = window.hide() {
                    tracing::warn!("Failed to hide indicator after pre-warming: {}", e);
                }

                tracing::info!("Recording indicator window ready (Linux - hidden until needed)");
            }

            // On macOS, move off-screen and keep visible to avoid animation delays
            #[cfg(not(target_os = "linux"))]
            {
                // Move to off-screen position (far off-screen so it's never visible)
                if let Err(e) = window.set_position(tauri::Position::Logical(LogicalPosition::new(
                    -10000.0, -10000.0,
                ))) {
                    tracing::warn!("Failed to position indicator off-screen: {}", e);
                }

                // Show the window to trigger webview content load - we keep it visible
                // (but off-screen) permanently to avoid show/hide animation delays
                if let Err(e) = window.show() {
                    tracing::warn!("Failed to show indicator for pre-warming: {}", e);
                }

                // Wait for the webview to fully load and render
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                // Window stays visible but off-screen - ready for instant positioning
                tracing::info!("Recording indicator window pre-warmed and ready (kept visible off-screen)");
            }
        } else {
            tracing::warn!("Recording indicator window not found for pre-warming");
        }
    });
}
