//! Recording indicator window management
//!
//! Handles showing/hiding the floating recording indicator pill near the text
//! cursor (caret), similar to macOS dictation. Falls back to bottom-centre of
//! the main window's monitor if caret position cannot be determined.
//!
//! The indicator window is pre-warmed at app startup to eliminate any delay
//! when showing it for the first time.

use crate::mouse_tracker;
use tauri::{AppHandle, LogicalPosition, Manager, Runtime, WebviewWindow};

/// Label for the recording indicator window (must match tauri.conf.json)
const INDICATOR_WINDOW_LABEL: &str = "recording-indicator";

/// Indicator dimensions in logical pixels (must match frontend)
const PILL_WIDTH: f64 = 58.0;
const PILL_HEIGHT: f64 = 58.0;

/// Fallback: padding from bottom of screen (above dock)
const BOTTOM_PADDING: f64 = 120.0;

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

/// Show the recording indicator window at the current cursor position.
///
/// Positions the indicator near the mouse cursor and starts cursor-following
/// tracking. Falls back to bottom-centre of the main window's monitor if
/// the mouse position cannot be determined.
#[tauri::command]
pub fn show_recording_indicator(app: AppHandle) -> Result<(), String> {
    let indicator = get_indicator_window(&app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // Position at current mouse cursor for instant feedback
    if let Some((x, y)) = mouse_tracker::get_initial_position() {
        indicator
            .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
            .map_err(|e| e.to_string())?;
        indicator.show().map_err(|e| e.to_string())?;
        tracing::debug!("Recording indicator shown at cursor ({}, {})", x, y);
    } else {
        // Fallback: position at bottom centre of main window's monitor
        tracing::info!("No mouse position available, falling back to bottom-centre");
        position_at_bottom_centre(&app, &indicator)?;
        indicator.show().map_err(|e| e.to_string())?;
    }

    mouse_tracker::start_tracking();
    Ok(())
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
#[tauri::command]
pub fn hide_recording_indicator(app: AppHandle) -> Result<(), String> {
    // Stop mouse tracking before moving off-screen
    mouse_tracker::stop_tracking();

    let window = get_indicator_window(&app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // Move off-screen instead of hiding - this avoids animation delays
    window
        .set_position(tauri::Position::Logical(LogicalPosition::new(
            -10000.0, -10000.0,
        )))
        .map_err(|e| e.to_string())?;

    tracing::debug!("Recording indicator moved off-screen (hidden)");
    Ok(())
}

/// Show the recording indicator immediately (generic version for shortcut handler).
///
/// Positions the indicator at the current mouse cursor position and starts
/// cursor-following tracking. Skips the caret position lookup for instant response.
///
/// The window is kept always-visible but positioned off-screen when "hidden"
/// to avoid macOS window show/hide animation delays.
pub fn show_indicator_instant<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    tracing::info!("show_indicator_instant called (fast path)");

    let indicator = get_indicator_window_generic(app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // Position at current mouse cursor for instant feedback.
    // The mouse tracker will then keep it following the cursor.
    if let Some((x, y)) = mouse_tracker::get_initial_position() {
        indicator
            .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
            .map_err(|e| e.to_string())?;
        let _ = indicator.show();
        tracing::debug!("Recording indicator shown at cursor ({}, {})", x, y);
    } else {
        // Fallback if mouse position unavailable: show at primary monitor centre-bottom
        let _ = indicator.show();
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
        tracing::debug!("Recording indicator shown at fallback position");
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
/// The window is left visible but off-screen - we never hide() it to avoid
/// macOS window show/hide animation delays.
pub fn prewarm_indicator_window(app: &AppHandle) {
    let app_handle = app.clone();

    // Spawn async task to pre-warm without blocking startup
    tauri::async_runtime::spawn(async move {
        // Small delay to let the main window initialise first
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        if let Some(window) = app_handle.get_webview_window(INDICATOR_WINDOW_LABEL) {
            tracing::info!("Pre-warming recording indicator window");

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
        } else {
            tracing::warn!("Recording indicator window not found for pre-warming");
        }
    });
}
