//! Wayland global shortcuts via the XDG Desktop Portal.
//!
//! Wayland has no client-side global-hotkey API: an application cannot grab a
//! key combination the way it can under X11. The sanctioned route is the
//! `org.freedesktop.portal.GlobalShortcuts` portal, where the application asks
//! the compositor to bind shortcuts and the compositor delivers `Activated`
//! signals over D-Bus.
//!
//! Two consequences shape this module:
//!
//! - **The app does not choose the key.** `bind_shortcuts` sends a
//!   *preferred trigger* as a hint only; the compositor (and usually the user,
//!   via a system dialog) decides the actual binding. We surface the assigned
//!   `trigger_description` back to the frontend so the UI can show the real key
//!   rather than pretending it is the default. Forcing "F13" is not possible.
//! - **Compositor support varies.** KDE Plasma, wlroots-based compositors
//!   (Sway, Hyprland), and GNOME 48+ implement the portal. Older GNOME and
//!   minimal compositors do not. When the portal is unavailable, registration
//!   fails *loudly*: the frontend is told so the user can fall back to a
//!   function-key shortcut, instead of pressing a hotkey that silently does
//!   nothing.
//!
//! ## Lifetime model
//!
//! `GlobalShortcuts::new()` connects to the user's session bus via a static
//! interface name, so the proxy and its session carry a `'static` lifetime and
//! can live for the process. The proxy, session, and activation stream are all
//! owned by a single long-lived task on Tauri's async runtime; that task owning
//! them is what keeps the binding alive (the `Session` has no `Drop`, so it is
//! closed explicitly only if the stream ever ends).
//!
//! Every activation routes through [`super::manager::dispatch_shortcut_action`],
//! the same dispatcher the X11/macOS plugin callback uses, so behaviour cannot
//! drift between platforms.

use std::sync::OnceLock;

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::manager::shortcut_ids;

/// Ensures the portal session is opened exactly once per process. `setup()` is
/// reachable both at startup and from the `reregister_shortcuts` command (when
/// the user edits a shortcut). Because the portal binding is process-global, not
/// per-registration, a second session would bind the shortcuts again and start a
/// second activation loop — dispatching every press twice. This guard makes the
/// re-run a no-op.
static PORTAL_STARTED: OnceLock<()> = OnceLock::new();

/// The shortcuts Thoth requests from the portal, with the trigger we *prefer*
/// (the compositor may assign something else, or let the user pick).
///
/// Modifier-only and toggle-enhancement shortcuts are intentionally omitted: a
/// bare modifier cannot be expressed as a portal trigger, and the portal dialog
/// is heavyweight enough that we only register the recording toggles users
/// actually press. The frontend can request more later through the same path.
fn requested_shortcuts() -> Vec<NewShortcut> {
    vec![
        NewShortcut::new(shortcut_ids::TOGGLE_RECORDING, "Toggle recording")
            .preferred_trigger(Some("F13")),
        NewShortcut::new(
            shortcut_ids::COPY_LAST_TRANSCRIPTION,
            "Copy last transcription",
        )
        .preferred_trigger(Some("F14")),
    ]
}

/// Reported to the frontend (event `wayland-shortcuts-status`) so the UI can
/// tell the user whether global shortcuts are working on their compositor and,
/// if so, which keys were actually assigned.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortalStatus {
    /// True when the portal bound shortcuts successfully.
    pub available: bool,
    /// Human-readable detail (which keys were bound, or why it failed).
    pub message: String,
    /// Per-shortcut assigned trigger descriptions, e.g. `("toggle_recording",
    /// "Super+R")`. Empty when unavailable.
    pub bindings: Vec<(String, String)>,
}

/// Set up the Wayland portal global shortcuts.
///
/// Spawns one long-lived task that owns the portal session and consumes the
/// activation stream, and returns immediately. The outcome (available /
/// unavailable, and which keys were bound) is delivered asynchronously to the
/// frontend via the `wayland-shortcuts-status` event. This is fire-and-forget
/// by design — binding shows a compositor dialog that can take arbitrary time,
/// and we must not block app startup on it.
pub fn setup(app: &AppHandle) {
    // Open the portal session at most once per process (see PORTAL_STARTED).
    if PORTAL_STARTED.set(()).is_err() {
        tracing::debug!("Wayland shortcut portal already set up; skipping re-setup");
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run_portal(app).await;
    });
}

/// Own the proxy and session for the lifetime of this task, bind the shortcuts,
/// report the result to the frontend, then loop on activations until the stream
/// ends. The proxy, session, and stream are all owned within this single scope.
async fn run_portal(app: AppHandle) {
    use futures_util::StreamExt;

    // Bind. On failure, tell the user loudly and stop — this is the whole point
    // of the module: a compositor without the portal must not leave the user
    // pressing a hotkey that silently does nothing.
    let (shortcuts, session, bindings) = match bind().await {
        Ok(v) => v,
        Err(e) => {
            let message = format!(
                "Global keyboard shortcuts are not available on this Wayland compositor \
                 ({e}). Use a function-key shortcut (e.g. F13) or switch to an X11 session."
            );
            tracing::warn!("{message}");
            emit_status(
                &app,
                PortalStatus {
                    available: false,
                    message,
                    bindings: Vec::new(),
                },
            );
            return;
        }
    };

    let summary = if bindings.is_empty() {
        "Global shortcuts registered with the desktop portal.".to_string()
    } else {
        let keys = bindings
            .iter()
            .map(|(id, desc)| format!("{id} → {desc}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Global shortcuts registered with the desktop portal: {keys}")
    };
    tracing::info!("{summary}");
    emit_status(
        &app,
        PortalStatus {
            available: true,
            message: summary,
            bindings,
        },
    );

    // Create the activation stream here, after the proxy has been moved into
    // this scope, so its borrow of `shortcuts` is local to this task.
    let mut activated = match shortcuts.receive_activated().await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!("Failed to subscribe to Wayland shortcut activations: {e}");
            return;
        }
    };

    tracing::info!("Wayland global-shortcut activation loop started");
    while let Some(activation) = activated.next().await {
        let id = activation.shortcut_id().to_string();
        tracing::info!("Wayland portal activated shortcut: {id}");
        super::manager::dispatch_shortcut_action(&app, &id);
    }
    tracing::warn!("Wayland global-shortcut activation stream ended");

    // The stream ended (the session was revoked or the bus dropped). Close the
    // session explicitly — `Session` has no `Drop`, so without this the bus-side
    // session would linger until process exit. `shortcuts` and `session` are
    // then dropped at end of scope.
    if let Err(e) = session.close().await {
        tracing::debug!("Closing Wayland shortcut session returned: {e}");
    }
}

/// Create the portal proxy and session and bind the requested shortcuts.
/// Returns the proxy and session (for the caller to keep alive) and the
/// per-shortcut trigger descriptions the compositor assigned.
async fn bind() -> Result<
    (
        GlobalShortcuts,
        ashpd::desktop::Session<GlobalShortcuts>,
        Vec<(String, String)>,
    ),
    ashpd::Error,
> {
    let shortcuts = GlobalShortcuts::new().await?;
    let session = shortcuts
        .create_session(CreateSessionOptions::default())
        .await?;

    // No parent window handle: the portal dialog still works without one, and
    // wiring a Wayland `wl_surface` handle from Tauri is fragile. The dialog is
    // modal to the compositor, not to a specific Thoth window.
    let request = shortcuts
        .bind_shortcuts(
            &session,
            &requested_shortcuts(),
            None,
            BindShortcutsOptions::default(),
        )
        .await?;
    let bindings: Vec<(String, String)> = request
        .response()?
        .shortcuts()
        .iter()
        .map(|s| (s.id().to_string(), s.trigger_description().to_string()))
        .collect();

    Ok((shortcuts, session, bindings))
}

fn emit_status(app: &AppHandle, status: PortalStatus) {
    if let Err(e) = app.emit("wayland-shortcuts-status", &status) {
        tracing::error!("Failed to emit wayland-shortcuts-status event: {e}");
    }
}
