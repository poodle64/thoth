//! Process-wide access to the Tauri `AppHandle`.
//!
//! Most code receives an `AppHandle` through Tauri command parameters or the
//! setup closure. A few deep, hot paths (audio device selection, for example)
//! need to surface a user-facing event but are called far from any handle and
//! threading one through every call would be invasive on timing-sensitive code.
//! This module stores the handle once at startup so those paths can emit events
//! without plumbing.
//!
//! Prefer passing an `AppHandle` explicitly where it is already available; reach
//! for this only when a path genuinely cannot obtain one.

use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Store the global handle. Called once during setup; later calls are ignored.
pub fn set(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
}

/// Get the global handle, if it has been set.
pub fn get() -> Option<AppHandle> {
    APP_HANDLE.get().cloned()
}

/// Emit an event to the frontend through the global handle, if available.
/// No-op (with a debug log) when the handle is not yet set or emission fails,
/// so callers on hot paths never have to handle the error.
pub fn emit<S: serde::Serialize + Clone>(event: &str, payload: S) {
    match APP_HANDLE.get() {
        Some(app) => {
            if let Err(e) = app.emit(event, payload) {
                tracing::debug!("Failed to emit '{event}' via global handle: {e}");
            }
        }
        None => tracing::debug!("Global app handle not set; dropping '{event}' event"),
    }
}
