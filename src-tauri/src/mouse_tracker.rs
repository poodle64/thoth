//! Mouse cursor tracking for the recording indicator window.
//!
//! When recording, the indicator window follows the mouse cursor so it's
//! always visible near where the user is working. Uses a background polling
//! thread at ~60fps to read the cursor position via Core Graphics and
//! reposition the indicator window.
//!
//! The indicator window is made click-through during tracking so it never
//! intercepts user clicks. The user stops recording via keyboard shortcut.
//!
//! Coordinate system: `CGEvent::location()` returns logical points with
//! origin at the top-left of the primary display, matching Tauri's
//! `LogicalPosition` directly.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use parking_lot::RwLock;
use tauri::{AppHandle, LogicalPosition, Manager};

use crate::recording_indicator;

/// Poll interval in milliseconds (~60fps)
const POLL_INTERVAL_MS: u64 = 16;

/// Offset from cursor: window centred directly above with 12px gap
const CURSOR_OFFSET_X: f64 = -29.0; // half of 58px width (centres horizontally)
const CURSOR_OFFSET_Y: f64 = -70.0; // 58px height + 12px gap (directly above)

/// Skip repositioning if cursor moved less than this (logical pixels)
const MOVE_THRESHOLD: f64 = 1.0;

/// Re-query monitor bounds when cursor is within this margin of cached edges
const MONITOR_REFRESH_MARGIN: f64 = 100.0;

/// Recording indicator window label (must match tauri.conf.json)
const INDICATOR_WINDOW_LABEL: &str = "recording-indicator";

/// Cached monitor bounds (x, y, width, height) in logical pixels
struct CachedMonitor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

struct TrackerState {
    is_running: AtomicBool,
}

impl Default for TrackerState {
    fn default() -> Self {
        Self {
            is_running: AtomicBool::new(false),
        }
    }
}

/// Global tracker state
static TRACKER: OnceLock<RwLock<TrackerState>> = OnceLock::new();

/// Global app handle, stored at init with concrete type
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

fn get_tracker() -> &'static RwLock<TrackerState> {
    TRACKER.get_or_init(|| RwLock::new(TrackerState::default()))
}

/// Initialise the mouse tracker with the app handle.
///
/// Must be called during `setup()` where the AppHandle type is concrete.
pub fn init(app: &AppHandle) {
    APP_HANDLE.set(app.clone()).ok();
    // Eagerly initialise tracker state
    let _ = get_tracker();
    tracing::info!("Mouse tracker initialised");
}

/// Get the current mouse cursor position using Core Graphics.
///
/// Returns `(x, y)` in logical points with origin at top-left of primary display.
#[cfg(target_os = "macos")]
fn get_mouse_position() -> Option<(f64, f64)> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let point = event.location();
    Some((point.x, point.y))
}

/// Get the current mouse cursor position using X11.
///
/// Returns `(x, y)` in logical pixels with origin at top-left of primary display.
/// Returns `None` on Wayland (where X11 isn't available).
#[cfg(target_os = "linux")]
fn get_mouse_position() -> Option<(f64, f64)> {
    // Check if we're on Wayland - X11 won't work there
    if is_wayland() {
        return None;
    }

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    // Get the DISPLAY environment variable
    let display = std::env::var("DISPLAY").ok()?;

    // Connect to X11 display
    let (conn, screen_num) = x11rb::connect(Some(&display)).ok()?;

    // Get the root window of the default screen
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Query pointer position
    let pointer = conn.query_pointer(root).ok()?.reply().ok()?;

    // root_x and root_y are relative to the root window (entire screen)
    Some((pointer.root_x as f64, pointer.root_y as f64))
}

/// Check if we're running on Wayland (where X11 won't work)
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

#[cfg(all(not(target_os = "macos"), not(target_os = "linux")))]
fn get_mouse_position() -> Option<(f64, f64)> {
    None
}

/// Get the initial indicator position based on the current cursor location.
///
/// Returns the offset position `(x, y)` ready for `set_position()`, or `None`
/// if the mouse position cannot be determined.
pub fn get_initial_position() -> Option<(f64, f64)> {
    let (cx, cy) = get_mouse_position()?;
    let x = cx + CURSOR_OFFSET_X;
    let y = cy + CURSOR_OFFSET_Y;
    Some((x.max(0.0), y.max(0.0)))
}

/// Check if the cursor is near the edge of cached monitor bounds.
fn needs_monitor_refresh(cursor_x: f64, cursor_y: f64, monitor: &CachedMonitor) -> bool {
    cursor_x < monitor.x + MONITOR_REFRESH_MARGIN
        || cursor_x > monitor.x + monitor.width - MONITOR_REFRESH_MARGIN
        || cursor_y < monitor.y + MONITOR_REFRESH_MARGIN
        || cursor_y > monitor.y + monitor.height - MONITOR_REFRESH_MARGIN
}

/// Refresh cached monitor bounds for the given cursor position.
fn refresh_monitor_bounds(app: &AppHandle, cursor_x: f64, cursor_y: f64) -> Option<CachedMonitor> {
    let (mon_x, mon_y, mon_w, mon_h, _scale) =
        recording_indicator::find_monitor_for_point(app, cursor_x, cursor_y)?;
    Some(CachedMonitor {
        x: mon_x,
        y: mon_y,
        width: mon_w,
        height: mon_h,
    })
}

/// Clamp the indicator position to stay within monitor bounds.
fn clamp_to_monitor(x: f64, y: f64, monitor: &CachedMonitor) -> (f64, f64) {
    let indicator_w = 58.0;
    let indicator_h = 58.0;

    let clamped_x = x
        .max(monitor.x)
        .min(monitor.x + monitor.width - indicator_w);
    let clamped_y = y
        .max(monitor.y)
        .min(monitor.y + monitor.height - indicator_h - 50.0);

    (clamped_x, clamped_y)
}

/// Start tracking the mouse cursor and repositioning the indicator window.
///
/// The indicator window is made click-through during tracking. Calling this
/// when already tracking is a no-op (uses atomic compare_exchange).
pub fn start_tracking() {
    let tracker = get_tracker();

    // Atomic compare_exchange to prevent double-start
    if tracker
        .read()
        .is_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        tracing::debug!("Mouse tracking already running, skipping start");
        return;
    }

    let Some(app) = APP_HANDLE.get().cloned() else {
        tracing::warn!("Mouse tracker not initialised, cannot start tracking");
        tracker.read().is_running.store(false, Ordering::SeqCst);
        return;
    };

    // Make indicator window click-through
    if let Some(window) = app.get_webview_window(INDICATOR_WINDOW_LABEL) {
        if let Err(e) = window.set_ignore_cursor_events(true) {
            tracing::warn!("Failed to set ignore cursor events: {}", e);
        }
    }

    tracing::info!("Starting mouse cursor tracking for recording indicator");

    thread::spawn(move || {
        let mut last_x: f64 = f64::NAN;
        let mut last_y: f64 = f64::NAN;
        let mut cached_monitor: Option<CachedMonitor> = None;

        while get_tracker().read().is_running.load(Ordering::SeqCst) {
            if let Some((cx, cy)) = get_mouse_position() {
                let dx = (cx - last_x).abs();
                let dy = (cy - last_y).abs();

                // First iteration (NaN) or cursor moved: reposition
                if dx > MOVE_THRESHOLD || dy > MOVE_THRESHOLD || last_x.is_nan() {
                    last_x = cx;
                    last_y = cy;

                    // Refresh monitor bounds if cursor near edge or no cache
                    if cached_monitor.is_none()
                        || cached_monitor
                            .as_ref()
                            .is_some_and(|m| needs_monitor_refresh(cx, cy, m))
                    {
                        cached_monitor = refresh_monitor_bounds(&app, cx, cy);
                    }

                    // Calculate indicator position (above-left of cursor)
                    let target_x = cx + CURSOR_OFFSET_X;
                    let target_y = cy + CURSOR_OFFSET_Y;

                    // Clamp to monitor bounds if available
                    let (final_x, final_y) = if let Some(ref monitor) = cached_monitor {
                        clamp_to_monitor(target_x, target_y, monitor)
                    } else {
                        (target_x, target_y)
                    };

                    // Reposition the indicator window
                    if let Some(window) = app.get_webview_window(INDICATOR_WINDOW_LABEL) {
                        let _ = window.set_position(tauri::Position::Logical(
                            LogicalPosition::new(final_x, final_y),
                        ));
                    }
                }
            }

            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }

        tracing::info!("Mouse cursor tracking stopped");
    });
}

/// Stop tracking the mouse cursor.
///
/// The indicator window is restored to accept cursor events.
pub fn stop_tracking() {
    let tracker = get_tracker();
    let was_running = tracker.read().is_running.swap(false, Ordering::SeqCst);

    if !was_running {
        return;
    }

    tracing::info!("Stopping mouse cursor tracking");

    // Restore cursor events on the indicator window
    if let Some(app) = APP_HANDLE.get() {
        if let Some(window) = app.get_webview_window(INDICATOR_WINDOW_LABEL) {
            if let Err(e) = window.set_ignore_cursor_events(false) {
                tracing::warn!("Failed to restore cursor events: {}", e);
            }
        }
    }
}

/// Shut down the mouse tracker. Called on app exit.
pub fn shutdown() {
    stop_tracking();
    // Thread will exit within one poll interval (16ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_to_monitor() {
        let monitor = CachedMonitor {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };

        // Normal position (within bounds)
        let (x, y) = clamp_to_monitor(500.0, 300.0, &monitor);
        assert_eq!(x, 500.0);
        assert_eq!(y, 300.0);

        // Left edge
        let (x, _y) = clamp_to_monitor(-20.0, 300.0, &monitor);
        assert_eq!(x, 0.0);

        // Right edge (58px indicator width)
        let (x, _y) = clamp_to_monitor(1900.0, 300.0, &monitor);
        assert_eq!(x, 1862.0); // 1920 - 58

        // Top edge
        let (_x, y) = clamp_to_monitor(500.0, -20.0, &monitor);
        assert_eq!(y, 0.0);

        // Bottom edge (58px indicator height + 50px dock padding)
        let (_x, y) = clamp_to_monitor(500.0, 1050.0, &monitor);
        assert_eq!(y, 972.0); // 1080 - 58 - 50
    }

    #[test]
    fn test_needs_monitor_refresh() {
        let monitor = CachedMonitor {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };

        // Centre of screen: no refresh needed
        assert!(!needs_monitor_refresh(960.0, 540.0, &monitor));

        // Near left edge: refresh needed
        assert!(needs_monitor_refresh(50.0, 540.0, &monitor));

        // Near right edge: refresh needed
        assert!(needs_monitor_refresh(1870.0, 540.0, &monitor));

        // Near top edge: refresh needed
        assert!(needs_monitor_refresh(960.0, 50.0, &monitor));

        // Near bottom edge: refresh needed
        assert!(needs_monitor_refresh(960.0, 1030.0, &monitor));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_mouse_position_returns_some() {
        // On macOS, this should succeed (no special permissions needed for CGEvent)
        let pos = get_mouse_position();
        assert!(
            pos.is_some(),
            "get_mouse_position should return a position on macOS"
        );
        let (x, y) = pos.unwrap();
        assert!(x >= 0.0, "x should be non-negative");
        assert!(y >= 0.0, "y should be non-negative");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_get_mouse_position_returns_some_on_x11() {
        // On Linux with X11, this should succeed if DISPLAY is set
        // May return None on Wayland without XWayland
        if std::env::var("DISPLAY").is_ok() {
            let pos = get_mouse_position();
            assert!(
                pos.is_some(),
                "get_mouse_position should return a position on Linux X11"
            );
            let (x, y) = pos.unwrap();
            assert!(x >= 0.0, "x should be non-negative");
            assert!(y >= 0.0, "y should be non-negative");
        }
    }
}
