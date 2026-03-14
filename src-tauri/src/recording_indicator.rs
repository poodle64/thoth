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
        .or_else(|| indicator.primary_monitor().ok().flatten());

    if let Some(monitor) = monitor {
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
        return Ok(());
    }

    // On Wayland, monitor info may be unavailable — compositor rules handle positioning
    #[cfg(target_os = "linux")]
    if is_wayland() {
        tracing::info!("Monitor info unavailable on Wayland — relying on compositor window rules for positioning");
        return Ok(());
    }

    Err("Could not determine current monitor".to_string())
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

    // On Wayland, CursorDot can't work (no mouse tracking), so upgrade to Pill.
    let style = if is_wayland() && style == IndicatorStyle::CursorDot {
        tracing::info!("Wayland: upgrading CursorDot to Pill (mouse tracking unavailable)");
        IndicatorStyle::Pill
    } else {
        style
    };

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

    // Position based on style.  On Linux the window is already shown, so if
    // positioning fails we must hide it again — otherwise it sits visible at
    // an uncontrolled location and can intercept clicks.
    let position_result = match style {
        IndicatorStyle::CursorDot => {
            // Position at cursor or use the provided fallback
            if let Some((x, y)) = mouse_tracker::get_initial_position() {
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())
                    .map(|_| {
                        tracing::debug!("Cursor-dot indicator at ({}, {})", x, y);
                    })
            } else {
                tracing::info!("No mouse position available, using fallback position");
                fallback_position(app, &indicator)
            }
        }
        IndicatorStyle::FixedFloat => {
            position_at_fixed(app, &indicator, style)
        }
        IndicatorStyle::Pill => {
            position_pill(app, &indicator)
        }
    };

    // If positioning failed on Linux, hide the already-shown window
    #[cfg(target_os = "linux")]
    if let Err(ref e) = position_result {
        tracing::warn!("Positioning failed ({}), hiding indicator to prevent ghost window", e);
        let _ = indicator.hide();
    }

    position_result?;

    // On macOS, show after positioning
    #[cfg(not(target_os = "linux"))]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    // On Wayland/Hyprland, re-apply window rules after each show.
    // Properties like bordersize, noshadow, pin, and size/position may not
    // persist across hide/show cycles.
    #[cfg(target_os = "linux")]
    if is_wayland() {
        spawn_hyprland_rules_reapply();
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
pub fn hide_recording_indicator(app: AppHandle, reason: Option<String>) -> Result<(), String> {
    tracing::info!("hide_recording_indicator called, reason={}", reason.as_deref().unwrap_or("(direct Rust call)"));

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

    // On Wayland, CursorDot can't work (no mouse tracking), so upgrade to Pill.
    let style = if is_wayland() && style == IndicatorStyle::CursorDot {
        tracing::info!("Wayland: upgrading CursorDot to Pill (mouse tracking unavailable)");
        IndicatorStyle::Pill
    } else {
        style
    };

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

    // Position based on style.  On Linux the window is already shown, so if
    // positioning fails we must hide it again — otherwise it sits visible at
    // an uncontrolled location and can intercept clicks (e.g. the tray-click
    // mouse-up) which triggers the indicator's stop handler.
    let position_result = match style {
        IndicatorStyle::CursorDot => {
            // Position at cursor or fall back to primary monitor centre-bottom
            if let Some((x, y)) = mouse_tracker::get_initial_position() {
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())
                    .map(|_| {
                        tracing::debug!("Cursor-dot indicator at ({}, {})", x, y);
                    })
            } else {
                position_at_primary_monitor(&indicator);
                tracing::debug!("Cursor-dot indicator at fallback (bottom-centre)");
                Ok(())
            }
        }
        IndicatorStyle::FixedFloat => {
            // Position at the configured fixed location
            position_fixed_generic(&indicator)
        }
        IndicatorStyle::Pill => {
            // Position at top-centre of screen
            position_pill_generic(&indicator)
        }
    };

    // If positioning failed on Linux, hide the already-shown window
    #[cfg(target_os = "linux")]
    if let Err(ref e) = position_result {
        tracing::warn!("Positioning failed ({}), hiding indicator to prevent ghost window", e);
        let _ = indicator.hide();
    }

    position_result?;

    // On macOS, show after positioning
    #[cfg(not(target_os = "linux"))]
    {
        indicator.show().map_err(|e| e.to_string())?;
    }

    // On Wayland/Hyprland, re-apply window rules after each show.
    #[cfg(target_os = "linux")]
    if is_wayland() {
        spawn_hyprland_rules_reapply();
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

    let monitor = match indicator.primary_monitor().ok().flatten() {
        Some(m) => m,
        None => {
            // On Wayland, primary_monitor() may return None — compositor handles positioning
            #[cfg(target_os = "linux")]
            if is_wayland() {
                tracing::info!("Monitor info unavailable on Wayland — relying on compositor window rules for positioning");
                return Ok(());
            }
            return Err("Could not determine primary monitor".to_string());
        }
    };

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
    // Try Tauri's monitor API first
    if let Some(monitor) = indicator.primary_monitor().ok().flatten() {
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
        return Ok(());
    }

    // Fallback: on Wayland, primary_monitor() may return None.
    // Hyprland window rules handle positioning, so just log and proceed.
    #[cfg(target_os = "linux")]
    if is_wayland() {
        tracing::info!("primary_monitor() unavailable on Wayland — relying on compositor window rules for positioning");
        return Ok(());
    }

    Err("Could not determine primary monitor".to_string())
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
    let app_handle = app.clone();

    // Spawn async task to pre-warm without blocking startup
    tauri::async_runtime::spawn(async move {
        // Small delay to let the main window initialise first
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        if let Some(window) = app_handle.get_webview_window(INDICATOR_WINDOW_LABEL) {
            tracing::info!("Pre-warming recording indicator window");

            // On Linux, show then hide to map the window with compositor.
            // This ensures subsequent show() calls will work.
            #[cfg(target_os = "linux")]
            {
                tracing::info!("Pre-warming indicator for Linux - showing then hiding");

                // On Wayland/Hyprland, register windowrulev2 rules BEFORE
                // showing the window. These rules apply at window creation
                // time (first map), so they must be in place before show().
                if is_wayland() {
                    register_hyprland_window_rules().await;
                }

                // Show the window to register it with the compositor.
                // The windowrulev2 rules apply automatically at first map.
                if let Err(e) = window.show() {
                    tracing::warn!("Failed to show indicator for pre-warming: {}", e);
                }

                // Wait for compositor to acknowledge and map the window
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;

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

/// Spawn a background task to position the indicator on Hyprland after show.
///
/// The persistent windowrulev2 rules (registered during prewarm) handle
/// float/pin/noborder/size automatically. This only needs to position the
/// window at top-centre since that requires knowing the monitor resolution.
#[cfg(target_os = "linux")]
fn spawn_hyprland_rules_reapply() {
    tauri::async_runtime::spawn(async {
        // Wait for compositor to finish mapping the window
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        position_hyprland_indicator().await;
    });
}

/// Register persistent Hyprland windowrulev2 rules for the indicator.
///
/// These rules match by title "thoth-indicator" and apply automatically
/// every time the window is shown — no per-show fixup needed. Called once
/// during prewarm.
#[cfg(target_os = "linux")]
async fn register_hyprland_window_rules() {
    use crate::platform::linux::{display_session, WaylandCompositor};

    if display_session().compositor() != WaylandCompositor::Hyprland {
        tracing::debug!("Not Hyprland — skipping indicator window rules");
        return;
    }

    tracing::info!("Registering Hyprland windowrulev2 rules for indicator");

    // Rules that apply automatically to any window with title "thoth-indicator".
    // Both size and maxsize are set to prevent WebKitGTK from expanding beyond
    // the pill dimensions.
    let rules = [
        "float",
        "pin",
        "noborder",
        "noshadow 1",
        "noblur 1",
        "nodim 1",
        "noanim 1",
        "nofocus 1",
        "opaque override 0",
        &format!("size {} {}", PILL_WIDTH as i32, PILL_HEIGHT as i32),
        &format!("maxsize {} {}", PILL_WIDTH as i32, PILL_HEIGHT as i32),
        &format!("minsize {} {}", PILL_WIDTH as i32, PILL_HEIGHT as i32),
    ];

    for rule in &rules {
        let rule_str = format!("{},title:thoth-indicator", rule);
        run_hyprctl(&["keyword", "windowrulev2", &rule_str]).await;
    }

    tracing::info!("Hyprland window rules registered for title:thoth-indicator");
}

/// Position and size the indicator on Hyprland after each show.
///
/// Forces the correct size (in case the compositor didn't apply the
/// windowrulev2 size rule) and positions at top-centre of the focused
/// monitor.
#[cfg(target_os = "linux")]
async fn position_hyprland_indicator() {
    use crate::platform::linux::{display_session, WaylandCompositor};

    if display_session().compositor() != WaylandCompositor::Hyprland {
        return;
    }

    // Force correct size — windowrulev2 size rules may not re-apply on
    // subsequent show/hide cycles
    run_hyprctl(&[
        "dispatch",
        "resizewindowpixel",
        &format!(
            "exact {} {},title:thoth-indicator",
            PILL_WIDTH as i32, PILL_HEIGHT as i32
        ),
    ])
    .await;

    if let Some((mw, _mh)) = query_hyprland_focused_monitor().await {
        let x = (mw as f64 / 2.0 - PILL_WIDTH / 2.0) as i32;
        let y = PILL_EDGE_PADDING as i32;

        run_hyprctl(&[
            "dispatch",
            "movewindowpixel",
            &format!("exact {} {},title:thoth-indicator", x, y),
        ])
        .await;

        tracing::debug!(
            "Hyprland indicator: size {}x{}, position ({}, {})",
            PILL_WIDTH as i32,
            PILL_HEIGHT as i32,
            x,
            y
        );
    }
}

/// Run a hyprctl command, logging warnings on failure.
#[cfg(target_os = "linux")]
async fn run_hyprctl(args: &[&str]) {
    match tokio::process::Command::new("hyprctl")
        .args(args)
        .output()
        .await
    {
        Ok(out) if out.status.success() => {
            tracing::debug!("hyprctl {}: ok", args.join(" "));
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim().to_string()
            } else {
                stderr.trim().to_string()
            };
            tracing::warn!("hyprctl {} failed: {}", args.join(" "), detail);
        }
        Err(e) => {
            tracing::warn!("Failed to run hyprctl {}: {}", args.join(" "), e);
        }
    }
}

/// Query the focused monitor's resolution from Hyprland.
///
/// Returns `(width, height)` in pixels.
#[cfg(target_os = "linux")]
async fn query_hyprland_focused_monitor() -> Option<(u32, u32)> {
    let output = tokio::process::Command::new("hyprctl")
        .args(["monitors", "-j"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let monitors = json.as_array()?;

    // Find focused monitor, or fall back to first
    let monitor = monitors
        .iter()
        .find(|m| m["focused"].as_bool() == Some(true))
        .or_else(|| monitors.first())?;

    let width = monitor["width"].as_u64()? as u32;
    let height = monitor["height"].as_u64()? as u32;

    tracing::debug!("Hyprland focused monitor: {}x{}", width, height);
    Some((width, height))
}
