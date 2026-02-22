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
use crate::config::IndicatorStyle;
use crate::mouse_tracker;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Runtime, WebviewWindow};

/// Label for the recording indicator window (must match tauri.conf.json)
const INDICATOR_WINDOW_LABEL: &str = "recording-indicator";

/// Cursor-dot/fixed-float dimensions in logical pixels
const DOT_WIDTH: f64 = 58.0;
const DOT_HEIGHT: f64 = 58.0;

/// Pill dimensions in logical pixels
const PILL_WIDTH: f64 = 280.0;
const PILL_HEIGHT: f64 = 44.0;

/// Fallback: padding from bottom of screen (above dock)
const BOTTOM_PADDING: f64 = 120.0;

/// Padding from screen edge for pill style
const PILL_EDGE_PADDING: f64 = 12.0;

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

/// Get the window dimensions for a given indicator style.
pub fn dimensions_for_style(style: IndicatorStyle) -> (f64, f64) {
    match style {
        IndicatorStyle::CursorDot | IndicatorStyle::FixedFloat => (DOT_WIDTH, DOT_HEIGHT),
        IndicatorStyle::Pill => (PILL_WIDTH, PILL_HEIGHT),
    }
}

/// Resize the indicator window to match the current style.
fn resize_indicator(indicator: &WebviewWindow, style: IndicatorStyle) -> Result<(), String> {
    let (w, h) = dimensions_for_style(style);
    indicator
        .set_size(tauri::Size::Logical(LogicalSize::new(w, h)))
        .map_err(|e| e.to_string())
}

/// Position the indicator at a fixed location based on `RecorderPosition` config.
fn position_at_fixed(
    app: &AppHandle,
    indicator: &WebviewWindow,
    style: IndicatorStyle,
) -> Result<(), String> {
    let cfg = config::get_config().unwrap_or_default();
    let pos = cfg.recorder.position;
    let (iw, ih) = dimensions_for_style(style);

    let monitor = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| indicator.current_monitor().ok().flatten())
        .or_else(|| indicator.primary_monitor().ok().flatten())
        .ok_or_else(|| "Could not determine current monitor".to_string())?;

    let scale = monitor.scale_factor();
    let mp = monitor.position();
    let ms = monitor.size();
    let mx = mp.x as f64 / scale;
    let my = mp.y as f64 / scale;
    let mw = ms.width as f64 / scale;
    let mh = ms.height as f64 / scale;

    let padding = 20.0;
    let (x, y) = match pos {
        config::RecorderPosition::Cursor => {
            // For fixed-float with cursor position: use centre-bottom fallback
            (mx + (mw / 2.0) - (iw / 2.0), my + mh - ih - BOTTOM_PADDING)
        }
        config::RecorderPosition::TrayIcon => {
            // Near top-right (where tray typically is)
            (mx + mw - iw - padding, my + padding + 30.0)
        }
        config::RecorderPosition::TopLeft => (mx + padding, my + padding + 30.0),
        config::RecorderPosition::TopRight => (mx + mw - iw - padding, my + padding + 30.0),
        config::RecorderPosition::BottomLeft => (mx + padding, my + mh - ih - BOTTOM_PADDING),
        config::RecorderPosition::BottomRight => {
            (mx + mw - iw - padding, my + mh - ih - BOTTOM_PADDING)
        }
        config::RecorderPosition::Centre => {
            (mx + (mw / 2.0) - (iw / 2.0), my + (mh / 2.0) - (ih / 2.0))
        }
    };

    indicator
        .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;

    tracing::debug!("Fixed-float indicator at ({}, {}) position={:?}", x, y, pos);
    Ok(())
}

/// Position the pill indicator at the top-centre of the screen.
fn position_pill(app: &AppHandle, indicator: &WebviewWindow) -> Result<(), String> {
    let monitor = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| indicator.current_monitor().ok().flatten())
        .or_else(|| indicator.primary_monitor().ok().flatten())
        .ok_or_else(|| "Could not determine current monitor".to_string())?;

    let scale = monitor.scale_factor();
    let mp = monitor.position();
    let ms = monitor.size();
    let mx = mp.x as f64 / scale;
    let my = mp.y as f64 / scale;
    let mw = ms.width as f64 / scale;

    let x = mx + (mw / 2.0) - (PILL_WIDTH / 2.0);
    let y = my + PILL_EDGE_PADDING + 30.0; // Below menu bar

    indicator
        .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;

    tracing::debug!("Pill indicator at top-centre ({}, {})", x, y);
    Ok(())
}

/// Shared logic for showing the recording indicator.
///
/// Checks config, warns on Wayland, gets the window, resizes and positions
/// based on the current indicator style, then shows and optionally starts
/// mouse tracking (cursor-dot only).
fn show_indicator_common<F>(app: &AppHandle, fallback_position: F) -> Result<(), String>
where
    F: FnOnce(&AppHandle, &WebviewWindow) -> Result<(), String>,
{
    let cfg = config::get_config().unwrap_or_default();
    let style = cfg.general.indicator_style;

    if !cfg.general.show_recording_indicator {
        tracing::info!("Recording indicator disabled in config, skipping show");
        return Ok(());
    }

    if is_wayland() {
        tracing::warn!("Running on Wayland - indicator positioning may not work correctly");
    }

    let indicator = get_indicator_window(app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // Resize window for the current style
    resize_indicator(&indicator, style)?;

    // Emit style to frontend so it knows how to render
    let _ = indicator.emit("indicator-style", style);

    // On Linux, show before positioning to ensure the window is mapped
    #[cfg(target_os = "linux")]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    match style {
        IndicatorStyle::CursorDot => {
            // Position at cursor or use the provided fallback
            if let Some((x, y)) = mouse_tracker::get_initial_position() {
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())?;
                tracing::debug!("Cursor-dot indicator at ({}, {})", x, y);
            } else {
                tracing::info!("No mouse position available, using fallback position");
                fallback_position(app, &indicator)?;
            }
        }
        IndicatorStyle::FixedFloat => {
            position_at_fixed(app, &indicator, style)?;
        }
        IndicatorStyle::Pill => {
            position_pill(app, &indicator)?;
        }
    }

    // On macOS, show after positioning
    #[cfg(not(target_os = "linux"))]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    // Only track mouse for cursor-dot style
    if style == IndicatorStyle::CursorDot {
        mouse_tracker::start_tracking();
    }

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
        let x = mx + (mw / 2.0) - (DOT_WIDTH / 2.0);
        let y = my + mh - DOT_HEIGHT - BOTTOM_PADDING;
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
    let x = monitor_x + (monitor_width / 2.0) - (DOT_WIDTH / 2.0);
    let y = monitor_y + monitor_height - DOT_HEIGHT - BOTTOM_PADDING;

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
/// Positions the indicator based on the configured style. For cursor-dot,
/// positions at the current mouse cursor position and starts cursor-following
/// tracking. For fixed-float and pill, uses static positioning.
///
/// The window is kept always-visible but positioned off-screen when "hidden"
/// to avoid macOS window show/hide animation delays.
///
/// On Linux/Wayland, uses actual show()/hide() since compositor controls positioning.
///
/// Returns silently if the recording indicator is disabled in config.
pub fn show_indicator_instant<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    tracing::info!("show_indicator_instant called (fast path)");

    let indicator = get_indicator_window_generic(app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

    // Check config
    let cfg = config::get_config().unwrap_or_default();
    let style = cfg.general.indicator_style;

    if !cfg.general.show_recording_indicator {
        tracing::info!("Recording indicator disabled in config, skipping instant show");
        return Ok(());
    }

    if is_wayland() {
        tracing::warn!("Running on Wayland - indicator positioning may not work correctly");
    }

    // Resize window for the current style
    let (w, h) = dimensions_for_style(style);
    let _ = indicator.set_size(tauri::Size::Logical(LogicalSize::new(w, h)));

    // Emit style to frontend
    let _ = indicator.emit("indicator-style", style);

    // On Linux, show before positioning to ensure the window is mapped
    #[cfg(target_os = "linux")]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    match style {
        IndicatorStyle::CursorDot => {
            // Position at cursor or fall back to primary monitor centre-bottom
            if let Some((x, y)) = mouse_tracker::get_initial_position() {
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())?;
                tracing::debug!("Cursor-dot indicator at ({}, {})", x, y);
            } else {
                position_at_primary_monitor(&indicator);
                tracing::debug!("Cursor-dot indicator at fallback (bottom-centre)");
            }
        }
        IndicatorStyle::FixedFloat => {
            // Position at the configured fixed location
            position_fixed_generic(&indicator)?;
        }
        IndicatorStyle::Pill => {
            // Position at top-centre of screen
            position_pill_generic(&indicator)?;
        }
    }

    // On macOS, show after positioning
    #[cfg(not(target_os = "linux"))]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    // Only track mouse for cursor-dot style
    if style == IndicatorStyle::CursorDot {
        mouse_tracker::start_tracking();
    }

    Ok(())
}

/// Position indicator at fixed location (generic version for shortcut handler).
fn position_fixed_generic<R: Runtime>(
    indicator: &tauri::WebviewWindow<R>,
) -> Result<(), String> {
    let cfg = config::get_config().unwrap_or_default();
    let pos = cfg.recorder.position;
    let style = cfg.general.indicator_style;
    let (iw, ih) = dimensions_for_style(style);

    let monitor = indicator
        .primary_monitor()
        .ok()
        .flatten()
        .ok_or_else(|| "Could not determine primary monitor".to_string())?;

    let scale = monitor.scale_factor();
    let mp = monitor.position();
    let ms = monitor.size();
    let mx = mp.x as f64 / scale;
    let my = mp.y as f64 / scale;
    let mw = ms.width as f64 / scale;
    let mh = ms.height as f64 / scale;

    let padding = 20.0;
    let (x, y) = match pos {
        config::RecorderPosition::Cursor => {
            (mx + (mw / 2.0) - (iw / 2.0), my + mh - ih - BOTTOM_PADDING)
        }
        config::RecorderPosition::TrayIcon => (mx + mw - iw - padding, my + padding + 30.0),
        config::RecorderPosition::TopLeft => (mx + padding, my + padding + 30.0),
        config::RecorderPosition::TopRight => (mx + mw - iw - padding, my + padding + 30.0),
        config::RecorderPosition::BottomLeft => (mx + padding, my + mh - ih - BOTTOM_PADDING),
        config::RecorderPosition::BottomRight => {
            (mx + mw - iw - padding, my + mh - ih - BOTTOM_PADDING)
        }
        config::RecorderPosition::Centre => {
            (mx + (mw / 2.0) - (iw / 2.0), my + (mh / 2.0) - (ih / 2.0))
        }
    };

    indicator
        .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Position pill at top-centre (generic version for shortcut handler).
fn position_pill_generic<R: Runtime>(
    indicator: &tauri::WebviewWindow<R>,
) -> Result<(), String> {
    let monitor = indicator
        .primary_monitor()
        .ok()
        .flatten()
        .ok_or_else(|| "Could not determine primary monitor".to_string())?;

    let scale = monitor.scale_factor();
    let mp = monitor.position();
    let ms = monitor.size();
    let mx = mp.x as f64 / scale;
    let my = mp.y as f64 / scale;
    let mw = ms.width as f64 / scale;

    let x = mx + (mw / 2.0) - (PILL_WIDTH / 2.0);
    let y = my + PILL_EDGE_PADDING + 30.0;

    indicator
        .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;
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
