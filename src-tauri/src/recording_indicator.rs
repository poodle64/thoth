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

pub mod native;

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
///
/// Determines which screen to use based on:
/// 1. Caret position (if available) - shows on screen where user is typing
/// 2. Mouse cursor position - shows on screen where mouse is
/// 3. Main window's screen - fallback to Thoth's window screen
/// 4. Primary monitor - last resort
fn position_pill(app: &AppHandle, indicator: &WebviewWindow) -> Result<(), String> {
    // Try to determine which monitor based on caret/cursor position
    let monitor = {
        // First try: get monitor from caret position (where user is typing)
        if let Some(caret) = crate::platform::get_caret_position() {
            if let Some((mx, my, mw, mh, scale)) =
                find_monitor_for_point(app, caret.x, caret.y)
            {
                tracing::debug!(
                    "Pill positioning: using caret screen at ({:.0}, {:.0})",
                    caret.x,
                    caret.y
                );
                Some((mx, my, mw, mh, scale))
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| {
            // Second try: get monitor from mouse cursor position
            if let Some((cx, cy)) = mouse_tracker::get_initial_position() {
                if let Some(mon) = find_monitor_for_point(app, cx, cy) {
                    tracing::debug!(
                        "Pill positioning: using cursor screen at ({:.0}, {:.0})",
                        cx,
                        cy
                    );
                    Some(mon)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .or_else(|| {
            // Third try: main window's monitor
            app.get_webview_window("main")
                .and_then(|w| w.current_monitor().ok().flatten())
                .map(|m| {
                    let scale = m.scale_factor();
                    let mp = m.position();
                    let ms = m.size();
                    let mx = mp.x as f64 / scale;
                    let my = mp.y as f64 / scale;
                    let mw = ms.width as f64 / scale;
                    let mh = ms.height as f64 / scale;
                    tracing::debug!("Pill positioning: using main window screen");
                    (mx, my, mw, mh, scale)
                })
        })
        .or_else(|| {
            // Last resort: primary monitor
            indicator.primary_monitor().ok().flatten().map(|m| {
                let scale = m.scale_factor();
                let mp = m.position();
                let ms = m.size();
                let mx = mp.x as f64 / scale;
                let my = mp.y as f64 / scale;
                let mw = ms.width as f64 / scale;
                let mh = ms.height as f64 / scale;
                tracing::debug!("Pill positioning: using primary monitor (fallback)");
                (mx, my, mw, mh, scale)
            })
        })
        .ok_or_else(|| "Could not determine current monitor".to_string())?
    };

    let (mx, my, mw, _mh, _scale) = monitor;

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
            // Position at mouse cursor, then try caret, then use provided fallback
            if let Some((x, y)) = mouse_tracker::get_initial_position() {
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())?;
                #[cfg(debug_assertions)]
                {
                    if let Ok(pos) = indicator.outer_position() {
                        tracing::debug!(
                            "Cursor-dot indicator positioned at ({:.0}, {:.0}), actual window position: ({}, {})",
                            x, y, pos.x, pos.y
                        );
                    } else {
                        tracing::debug!("Cursor-dot indicator positioned at ({:.0}, {:.0})", x, y);
                    }
                }
                #[cfg(not(debug_assertions))]
                tracing::debug!("Cursor-dot indicator at mouse ({:.0}, {:.0})", x, y);
            } else if let Some(pos) = crate::platform::get_caret_position() {
                let x = pos.x - (DOT_WIDTH / 2.0);
                let y = pos.y - DOT_HEIGHT - 12.0;
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())?;
                #[cfg(debug_assertions)]
                {
                    if let Ok(window_pos) = indicator.outer_position() {
                        tracing::debug!(
                            "Cursor-dot indicator at caret ({:.0}, {:.0}), actual window position: ({}, {})",
                            x, y, window_pos.x, window_pos.y
                        );
                    } else {
                        tracing::debug!("Cursor-dot indicator at caret ({:.0}, {:.0})", x, y);
                    }
                }
                #[cfg(not(debug_assertions))]
                tracing::debug!("Cursor-dot indicator at caret ({:.0}, {:.0})", x, y);
            } else {
                tracing::debug!("No mouse or caret position, using fallback");
                fallback_position(app, &indicator)?;
                #[cfg(debug_assertions)]
                {
                    if let Ok(pos) = indicator.outer_position() {
                        tracing::debug!("Indicator fallback position set, actual window at ({}, {})", pos.x, pos.y);
                    }
                }
            }
        }
        IndicatorStyle::FixedFloat => {
            position_at_fixed(app, &indicator, style)?;
            #[cfg(debug_assertions)]
            {
                if let Ok(pos) = indicator.outer_position() {
                    tracing::debug!("Fixed-float indicator, actual window position: ({}, {})", pos.x, pos.y);
                }
            }
        }
        IndicatorStyle::Pill => {
            position_pill(app, &indicator)?;
            #[cfg(debug_assertions)]
            {
                if let Ok(pos) = indicator.outer_position() {
                    tracing::debug!("Pill indicator, actual window position: ({}, {})", pos.x, pos.y);
                }
            }
        }
    }

    // On macOS, window is already shown (kept visible off-screen to avoid animation delays)
    #[cfg(not(target_os = "linux"))]
    {
        // Check if window is already visible to avoid redundant show() call
        if !indicator.is_visible().unwrap_or(false) {
            indicator.show().map_err(|e| e.to_string())?;
            tracing::debug!("Indicator window was hidden, showing it (common path)");
        } else {
            tracing::debug!("Indicator window already visible, skipping show() (common path)");
        }
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

    // Try native indicator first on supported platforms
    #[cfg(all(feature = "native-indicator", target_os = "macos"))]
    {
        // Initialize native indicator if not already done
        let style = native::IndicatorStyle::CursorDot; // TODO: Get from config
        match native::init_native_indicator(style) {
            Ok(()) => {
                // Calculate position (bottom-centre of primary monitor)
                let position = calculate_indicator_position(&app)?;

                // Show native indicator
                native::show_native_indicator(position.x, position.y)
                    .map_err(|e| e.to_string())?;

                // Set initial state
                if let Err(e) = native::set_native_indicator_state(native::VisualizerState::Idle) {
                    tracing::warn!("Failed to set native indicator state: {:?}", e);
                }

                tracing::info!("Native indicator shown at ({}, {})", position.x, position.y);
                return Ok(());
            }
            Err(e) => {
                tracing::warn!("Failed to initialize native indicator: {:?}, falling back to WebView", e);
                // Fall through to WebView fallback
            }
        }
    }

    // WebView fallback (used on non-macOS or when native fails)
    show_indicator_common(&app, |app_handle, indicator| {
        position_at_bottom_centre(app_handle, indicator)
    })
}

/// Calculate indicator position (bottom-centre of primary monitor)
#[cfg(all(feature = "native-indicator", target_os = "macos"))]
fn calculate_indicator_position<R: Runtime>(app: &AppHandle<R>) -> Result<LogicalPosition<f64>, String> {
    // Try to get monitor from main window, fallback to primary
    let monitor = if let Some(main_window) = app.get_webview_window("main") {
        main_window
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| main_window.primary_monitor().ok().flatten())
    } else {
        // Use primary monitor
        app.primary_monitor().ok().flatten()
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

    // Calculate centre-bottom position (using DOT_WIDTH for now - native indicator handles sizing)
    let x = monitor_x + (monitor_width / 2.0) - (DOT_WIDTH / 2.0);
    let y = monitor_y + monitor_height - DOT_HEIGHT - BOTTOM_PADDING;

    tracing::debug!("Calculated indicator position: ({}, {})", x, y);

    Ok(LogicalPosition::new(x, y))
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

    tracing::debug!("Setting indicator position to ({}, {})", x, y);

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

    // Try native indicator first on supported platforms
    #[cfg(all(feature = "native-indicator", target_os = "macos"))]
    {
        match native::hide_native_indicator() {
            Ok(()) => {
                tracing::info!("Native indicator hidden");
                return Ok(());
            }
            Err(e) => {
                tracing::warn!("Failed to hide native indicator: {:?}, falling back to WebView", e);
                // Fall through to WebView fallback
            }
        }
    }

    // WebView fallback (used on non-macOS or when native fails)
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

        #[cfg(debug_assertions)]
        {
            if let Ok(pos) = window.outer_position() {
                tracing::debug!(
                    "Recording indicator moved off-screen, actual window position: ({}, {})",
                    pos.x, pos.y
                );
            } else {
                tracing::debug!("Recording indicator moved off-screen");
            }
        }
        #[cfg(not(debug_assertions))]
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

    // Check config
    let cfg = config::get_config().unwrap_or_default();
    let style = cfg.general.indicator_style;

    if !cfg.general.show_recording_indicator {
        tracing::info!("Recording indicator disabled in config, skipping instant show");
        return Ok(());
    }

    // Try native indicator first on supported platforms
    #[cfg(all(feature = "native-indicator", target_os = "macos"))]
    {
        let native_style = match style {
            IndicatorStyle::Pill => native::IndicatorStyle::Pill,
            IndicatorStyle::CursorDot => native::IndicatorStyle::CursorDot,
            IndicatorStyle::FixedFloat => native::IndicatorStyle::FixedFloat,
        };

        match native::init_native_indicator(native_style) {
            Ok(()) => {
                // Calculate position based on style
                let position = match style {
                    IndicatorStyle::CursorDot => {
                        // Try mouse cursor first
                        if let Some((x, y)) = mouse_tracker::get_initial_position() {
                            LogicalPosition::new(x, y)
                        } else if let Some(pos) = crate::platform::get_caret_position() {
                            LogicalPosition::new(pos.x - (DOT_WIDTH / 2.0), pos.y - DOT_HEIGHT - 12.0)
                        } else {
                            calculate_indicator_position(app)?
                        }
                    }
                    IndicatorStyle::Pill => calculate_indicator_position(app)?,
                    IndicatorStyle::FixedFloat => calculate_indicator_position(app)?,
                };

                // Show native indicator
                native::show_native_indicator(position.x, position.y)
                    .map_err(|e| e.to_string())?;

                // Set initial state to Idle (will be updated to Recording when recording starts)
                if let Err(e) = native::set_native_indicator_state(native::VisualizerState::Idle) {
                    tracing::warn!("Failed to set native indicator state: {:?}", e);
                }

                // Only track mouse for cursor-dot style
                if style == IndicatorStyle::CursorDot {
                    mouse_tracker::start_tracking();
                }

                tracing::info!("Native indicator shown at ({}, {})", position.x, position.y);
                return Ok(());
            }
            Err(e) => {
                tracing::warn!("Failed to initialize native indicator: {:?}, falling back to WebView", e);
                // Fall through to WebView fallback
            }
        }
    }

    // WebView fallback (non-macOS or native failed)
    let indicator = get_indicator_window_generic(app)
        .ok_or_else(|| "Recording indicator window not found".to_string())?;

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
            // Position at mouse cursor, then try caret, then fall back to bottom-centre
            if let Some((x, y)) = mouse_tracker::get_initial_position() {
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())?;
                tracing::debug!("Cursor-dot indicator at mouse ({:.0}, {:.0})", x, y);
            } else if let Some(pos) = crate::platform::get_caret_position() {
                let x = pos.x - (DOT_WIDTH / 2.0);
                let y = pos.y - DOT_HEIGHT - 12.0;
                indicator
                    .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
                    .map_err(|e| e.to_string())?;
                tracing::debug!("Cursor-dot indicator at caret ({:.0}, {:.0})", x, y);
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

    // On macOS, window is already shown (kept visible off-screen to avoid animation delays)
    // Just ensure it's visible - if already visible, this is a no-op
    #[cfg(not(target_os = "linux"))]
    {
        // Check if window is already visible to avoid redundant show() call
        if !indicator.is_visible().unwrap_or(false) {
            indicator.show().map_err(|e| e.to_string())?;
            tracing::debug!("Indicator window was hidden, showing it");
        } else {
            tracing::debug!("Indicator window already visible, skipping show()");
        }
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
///
/// Similar to `position_pill()` but works with generic Runtime parameter.
/// Determines screen based on caret or cursor position if available.
fn position_pill_generic<R: Runtime>(
    indicator: &tauri::WebviewWindow<R>,
) -> Result<(), String> {
    // Try to determine which monitor based on caret/cursor position
    let monitor = {
        // First try: get monitor from caret position
        if let Some(caret) = crate::platform::get_caret_position() {
            // We need an AppHandle to call find_monitor_for_point, but we have Runtime generic
            // So we'll manually find the monitor containing this point
            if let Ok(monitors) = indicator.available_monitors() {
                monitors.into_iter().find_map(|m| {
                    let scale = m.scale_factor();
                    let pos = m.position();
                    let size = m.size();
                    let mx = pos.x as f64 / scale;
                    let my = pos.y as f64 / scale;
                    let mw = size.width as f64 / scale;
                    let mh = size.height as f64 / scale;

                    // Check if caret is within this monitor
                    if caret.x >= mx
                        && caret.x < mx + mw
                        && caret.y >= my
                        && caret.y < my + mh
                    {
                        tracing::debug!(
                            "Pill positioning (generic): using caret screen at ({:.0}, {:.0})",
                            caret.x,
                            caret.y
                        );
                        Some((mx, my, mw, mh))
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        } else {
            None
        }
        .or_else(|| {
            // Second try: get monitor from cursor position
            if let Some((cx, cy)) = mouse_tracker::get_initial_position() {
                if let Ok(monitors) = indicator.available_monitors() {
                    monitors.into_iter().find_map(|m| {
                        let scale = m.scale_factor();
                        let pos = m.position();
                        let size = m.size();
                        let mx = pos.x as f64 / scale;
                        let my = pos.y as f64 / scale;
                        let mw = size.width as f64 / scale;
                        let mh = size.height as f64 / scale;

                        if cx >= mx && cx < mx + mw && cy >= my && cy < my + mh {
                            tracing::debug!(
                                "Pill positioning (generic): using cursor screen at ({:.0}, {:.0})",
                                cx,
                                cy
                            );
                            Some((mx, my, mw, mh))
                        } else {
                            None
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .or_else(|| {
            // Last resort: primary monitor
            indicator.primary_monitor().ok().flatten().map(|m| {
                let scale = m.scale_factor();
                let mp = m.position();
                let ms = m.size();
                let mx = mp.x as f64 / scale;
                let my = mp.y as f64 / scale;
                let mw = ms.width as f64 / scale;
                let mh = ms.height as f64 / scale;
                tracing::debug!("Pill positioning (generic): using primary monitor (fallback)");
                (mx, my, mw, mh)
            })
        })
        .ok_or_else(|| "Could not determine current monitor".to_string())?
    };

    let (mx, my, mw, _mh) = monitor;

    let x = mx + (mw / 2.0) - (PILL_WIDTH / 2.0);
    let y = my + PILL_EDGE_PADDING + 30.0;

    indicator
        .set_position(tauri::Position::Logical(LogicalPosition::new(x, y)))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Reposition the indicator window to its off-screen location.
///
/// Called after wake-from-sleep to ensure the window is back in the
/// expected off-screen position, in case macOS moved it during sleep.
pub fn reposition_indicator_offscreen(app: &AppHandle) {
    #[cfg(not(target_os = "linux"))]
    {
        if let Some(window) = app.get_webview_window(INDICATOR_WINDOW_LABEL) {
            if let Ok(pos) = window.outer_position() {
                let x = pos.x as f64;
                let y = pos.y as f64;

                // Check if window is NOT where we expect it (off-screen at -10000, -10000)
                if (x - (-10000.0)).abs() > 100.0 || (y - (-10000.0)).abs() > 100.0 {
                    tracing::warn!(
                        "Indicator window found at unexpected position ({:.0}, {:.0}) after wake - repositioning to off-screen",
                        x, y
                    );

                    // Move back off-screen
                    if let Err(e) = window.set_position(tauri::Position::Logical(LogicalPosition::new(
                        -10000.0, -10000.0,
                    ))) {
                        tracing::error!("Failed to reposition indicator off-screen after wake: {}", e);
                    } else {
                        tracing::debug!("Indicator window repositioned to off-screen after wake");
                    }
                } else {
                    tracing::debug!("Indicator window still at expected off-screen position after wake");
                }
            } else {
                tracing::warn!("Could not get indicator window position after wake");
            }
        }
    }
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
                } else {
                    tracing::debug!("Positioned indicator off-screen at (-10000, -10000)");
                }

                // Show the window to trigger webview content load - we keep it visible
                // (but off-screen) permanently to avoid show/hide animation delays
                if let Err(e) = window.show() {
                    tracing::warn!("Failed to show indicator for pre-warming: {}", e);
                } else {
                    tracing::debug!("Indicator window shown (off-screen)");
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
