//! XDG Desktop Portal GlobalShortcuts backend for Wayland
//!
//! Uses `ashpd` to communicate with `org.freedesktop.portal.GlobalShortcuts`
//! via DBus. The compositor presents a system dialog for the user to confirm
//! shortcut bindings.
//!
//! Supported compositors: KDE Plasma 5.27+, GNOME 44+, Hyprland, Sway (with
//! xdg-desktop-portal-wlr), and any compositor implementing the
//! org.freedesktop.portal.GlobalShortcuts interface.
//!
//! Session lifecycle: A single session is created at app startup and kept alive
//! for the entire app lifetime. All rebinds reuse this session. Dropping the
//! session or proxy would kill the signal listeners.

use crate::recording_indicator;
use crate::shortcuts::manager::{shortcut_ids, ShortcutEvent, ShortcutInfo};
use ashpd::desktop::global_shortcuts::GlobalShortcuts;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// Debounce window for portal shortcut events (ms)
const PRESS_DEBOUNCE_MS: u64 = 50;

// ---------------------------------------------------------------------------
// Shortcut state (sync access for register/list)
// ---------------------------------------------------------------------------

struct PortalShortcutState {
    shortcuts: HashMap<String, ShortcutInfo>,
    last_press_times: HashMap<String, Instant>,
}

static SHORTCUT_STATE: OnceLock<RwLock<PortalShortcutState>> = OnceLock::new();

fn get_shortcut_state() -> &'static RwLock<PortalShortcutState> {
    SHORTCUT_STATE.get_or_init(|| {
        RwLock::new(PortalShortcutState {
            shortcuts: HashMap::new(),
            last_press_times: HashMap::new(),
        })
    })
}

// ---------------------------------------------------------------------------
// Persistent portal session (async, initialized once)
// ---------------------------------------------------------------------------

/// Holds the portal proxy and session for the app lifetime.
/// Both must stay alive or signal streams will stop working.
struct PortalSession {
    proxy: GlobalShortcuts<'static>,
    session: ashpd::desktop::Session<'static, GlobalShortcuts<'static>>,
}

static PORTAL_SESSION: tokio::sync::OnceCell<PortalSession> = tokio::sync::OnceCell::const_new();

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the portal GlobalShortcuts session and start listening for signals.
///
/// Must be called once during app setup (from lib.rs) on Wayland.
/// Creates a persistent session and spawns tokio tasks that listen for
/// Activated/Deactivated signals, emitting the same Tauri events as manager.rs.
pub async fn init(app: AppHandle) -> Result<(), String> {
    use ashpd::desktop::global_shortcuts::NewShortcut;
    use futures_util::StreamExt;

    // Initialize the session (stored for app lifetime)
    let portal = PORTAL_SESSION
        .get_or_try_init(|| async {
            let proxy = GlobalShortcuts::new()
                .await
                .map_err(|e| format!("Failed to connect to GlobalShortcuts portal: {}", e))?;

            let session = proxy
                .create_session()
                .await
                .map_err(|e| format!("Failed to create GlobalShortcuts session: {}", e))?;

            tracing::info!("XDG Portal GlobalShortcuts session created");
            Ok::<_, String>(PortalSession { proxy, session })
        })
        .await?;

    // Bind initial shortcuts
    let shortcuts_to_bind: Vec<NewShortcut> = {
        let state = get_shortcut_state().read();
        state
            .shortcuts
            .values()
            .map(|s| {
                let trigger = accelerator_to_portal_trigger(&s.accelerator);
                NewShortcut::new(&s.id, &s.description)
                    .preferred_trigger(Some(trigger.as_str()))
            })
            .collect()
    };

    if !shortcuts_to_bind.is_empty() {
        match portal
            .proxy
            .bind_shortcuts(
                &portal.session,
                &shortcuts_to_bind,
                None::<&ashpd::WindowIdentifier>,
            )
            .await
        {
            Ok(response) => match response.response() {
                Ok(bound) => {
                    for shortcut in bound.shortcuts() {
                        tracing::info!(
                            "Portal bound shortcut '{}': trigger='{}'",
                            shortcut.id(),
                            shortcut.trigger_description()
                        );
                    }
                }
                Err(e) => tracing::warn!("Portal rejected shortcut binding: {}", e),
            },
            Err(e) => tracing::warn!("Failed to bind shortcuts: {}", e),
        }
    }

    // On Hyprland, the portal doesn't auto-assign triggers from preferred_trigger.
    // We use hyprctl to bind the keys at runtime.
    hyprland_bind_shortcuts().await;

    // Listen for Activated signals
    let app_activated = app.clone();
    let mut activated_stream = portal
        .proxy
        .receive_activated()
        .await
        .map_err(|e| format!("Failed to subscribe to Activated signal: {}", e))?;

    tokio::spawn(async move {
        while let Some(event) = activated_stream.next().await {
            let shortcut_id = event.shortcut_id().to_string();
            handle_activated(&app_activated, &shortcut_id);
        }
        tracing::warn!("Portal Activated signal stream ended");
    });

    // Listen for Deactivated signals
    let app_deactivated = app.clone();
    let mut deactivated_stream = portal
        .proxy
        .receive_deactivated()
        .await
        .map_err(|e| format!("Failed to subscribe to Deactivated signal: {}", e))?;

    tokio::spawn(async move {
        while let Some(event) = deactivated_stream.next().await {
            let shortcut_id = event.shortcut_id().to_string();
            handle_deactivated(&app_deactivated, &shortcut_id);
        }
        tracing::warn!("Portal Deactivated signal stream ended");
    });

    // Listen for ShortcutsChanged signals (compositor may reassign triggers)
    let mut changed_stream = portal
        .proxy
        .receive_shortcuts_changed()
        .await
        .map_err(|e| format!("Failed to subscribe to ShortcutsChanged signal: {}", e))?;

    tokio::spawn(async move {
        while let Some(event) = changed_stream.next().await {
            for shortcut in event.shortcuts() {
                tracing::info!(
                    "Portal shortcut changed: '{}' → trigger='{}'",
                    shortcut.id(),
                    shortcut.trigger_description()
                );
            }
        }
        tracing::warn!("Portal ShortcutsChanged signal stream ended");
    });

    tracing::info!("Portal GlobalShortcuts signal listeners started");
    Ok(())
}

// ---------------------------------------------------------------------------
// Event handlers (mirror manager.rs behavior)
// ---------------------------------------------------------------------------

/// Handle portal Activated signal (equivalent to key press)
fn handle_activated(app: &AppHandle, shortcut_id: &str) {
    // Guard: capture mode active
    if crate::keyboard_service::is_capture_active() {
        tracing::debug!(
            "Discarding portal shortcut '{}' — capture mode active",
            shortcut_id
        );
        return;
    }

    // Guard: screen locked
    if crate::platform::is_screen_locked() {
        tracing::debug!(
            "Discarding portal shortcut '{}' — screen locked",
            shortcut_id
        );
        return;
    }

    // Debounce
    {
        let mut state = get_shortcut_state().write();
        if let Some(last) = state.last_press_times.get(shortcut_id) {
            if last.elapsed().as_millis() < PRESS_DEBOUNCE_MS as u128 {
                return;
            }
        }
        state
            .last_press_times
            .insert(shortcut_id.to_string(), Instant::now());
    }

    tracing::info!("Portal shortcut activated: {}", shortcut_id);

    // Show recording indicator and play start sound immediately (same as manager.rs)
    if (shortcut_id == shortcut_ids::TOGGLE_RECORDING
        || shortcut_id == shortcut_ids::TOGGLE_RECORDING_ALT)
        && !crate::pipeline::is_pipeline_running()
        && crate::transcription::is_transcription_ready()
    {
        if let Err(e) = recording_indicator::show_indicator_instant(app) {
            tracing::warn!(
                "Failed to show recording indicator from portal shortcut: {}",
                e
            );
        }
        crate::sound::play_sound(crate::sound::SoundEvent::RecordingStart);
    }

    // Handle copy-last-transcription directly (no frontend round-trip)
    if shortcut_id == shortcut_ids::COPY_LAST_TRANSCRIPTION {
        match crate::database::transcription::list_transcriptions(Some(1), Some(0)) {
            Ok(transcriptions) => {
                if let Some(t) = transcriptions.into_iter().next() {
                    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(t.text)) {
                        Ok(()) => {
                            tracing::info!("Copied last transcription via portal shortcut")
                        }
                        Err(e) => tracing::error!("Failed to copy to clipboard: {}", e),
                    }
                }
            }
            Err(e) => tracing::error!("Failed to get last transcription: {}", e),
        }
        return;
    }

    // Emit events (same as manager.rs)
    let event = ShortcutEvent {
        id: shortcut_id.to_string(),
        state: "pressed".to_string(),
    };
    let _ = app.emit("shortcut-triggered", shortcut_id.to_string());
    let _ = app.emit("shortcut-pressed", &event);
}

/// Handle portal Deactivated signal (equivalent to key release)
fn handle_deactivated(app: &AppHandle, shortcut_id: &str) {
    if crate::keyboard_service::is_capture_active() {
        return;
    }

    tracing::info!("Portal shortcut deactivated: {}", shortcut_id);

    let event = ShortcutEvent {
        id: shortcut_id.to_string(),
        state: "released".to_string(),
    };
    let _ = app.emit("shortcut-released", &event);
}

// ---------------------------------------------------------------------------
// Public API (sync — called from Tauri commands)
// ---------------------------------------------------------------------------

/// Register a shortcut via the portal.
///
/// Stores in internal state and triggers an async rebind to update the
/// portal session. The portal's `BindShortcuts` replaces all previous
/// bindings, so we always rebind everything.
pub fn register(id: String, accelerator: String, description: String) -> Result<(), String> {
    let info = ShortcutInfo {
        id: id.clone(),
        accelerator,
        description,
        is_enabled: true,
    };

    {
        let mut state = get_shortcut_state().write();
        state.shortcuts.insert(id.clone(), info);
    }
    tracing::info!("Portal: registered shortcut '{}'", id);

    spawn_rebind();
    Ok(())
}

/// Unregister a shortcut from portal state and rebind.
pub fn unregister(id: &str) -> Result<(), String> {
    {
        let mut state = get_shortcut_state().write();
        state.shortcuts.remove(id);
    }
    tracing::info!("Portal: unregistered shortcut '{}'", id);

    spawn_rebind();
    Ok(())
}

/// List all portal-registered shortcuts.
pub fn list_registered() -> Vec<ShortcutInfo> {
    let state = get_shortcut_state().read();
    state.shortcuts.values().cloned().collect()
}

/// Unregister all portal shortcuts and rebind (clears all bindings).
pub fn unregister_all() -> Result<(), String> {
    {
        let mut state = get_shortcut_state().write();
        state.shortcuts.clear();
    }
    tracing::info!("Portal: unregistered all shortcuts");

    spawn_rebind();
    Ok(())
}

// ---------------------------------------------------------------------------
// Async rebind (reuses existing session)
// ---------------------------------------------------------------------------

/// Spawn an async rebind task. Safe to call from sync code.
/// No-op if the portal session hasn't been initialized yet (shortcuts
/// registered during startup are bound when `init()` runs).
fn spawn_rebind() {
    // Only rebind if session exists (init() has been called)
    if PORTAL_SESSION.get().is_none() {
        return;
    }
    tauri::async_runtime::spawn(async {
        if let Err(e) = rebind().await {
            tracing::warn!("Failed to rebind portal shortcuts: {}", e);
        }
    });
}

/// Rebind all registered shortcuts using the existing portal session.
///
/// The portal's `BindShortcuts` replaces all previous bindings on the
/// same session. This reuses the session created in `init()`.
async fn rebind() -> Result<(), String> {
    use ashpd::desktop::global_shortcuts::NewShortcut;

    let portal = PORTAL_SESSION
        .get()
        .ok_or("Portal session not initialized — rebind skipped")?;

    let shortcuts_to_bind: Vec<NewShortcut> = {
        let state = get_shortcut_state().read();
        state
            .shortcuts
            .values()
            .map(|s| {
                let trigger = accelerator_to_portal_trigger(&s.accelerator);
                NewShortcut::new(&s.id, &s.description)
                    .preferred_trigger(Some(trigger.as_str()))
            })
            .collect()
    };

    let response = portal
        .proxy
        .bind_shortcuts(
            &portal.session,
            &shortcuts_to_bind,
            None::<&ashpd::WindowIdentifier>,
        )
        .await
        .map_err(|e| format!("Failed to bind shortcuts: {}", e))?;

    match response.response() {
        Ok(bound) => {
            for shortcut in bound.shortcuts() {
                tracing::info!(
                    "Portal rebound shortcut '{}': trigger='{}'",
                    shortcut.id(),
                    shortcut.trigger_description()
                );
            }
        }
        Err(e) => tracing::warn!("Portal rejected rebind: {}", e),
    }

    // Re-apply Hyprland bindings after rebind
    hyprland_bind_shortcuts().await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Hyprland-specific binding
// ---------------------------------------------------------------------------

/// On Hyprland, the portal registers shortcuts but doesn't auto-assign triggers.
/// We use `hyprctl keyword bind` to map our preferred triggers to the portal
/// shortcuts at runtime.
async fn hyprland_bind_shortcuts() {
    use crate::platform::linux::{display_session, DisplaySession, WaylandCompositor};

    let session = display_session();
    if !matches!(session, DisplaySession::Wayland(WaylandCompositor::Hyprland)) {
        return;
    }

    // First, get the app_id prefix that Hyprland uses for our shortcuts.
    // Tauri/Chromium registers as "org.chromium.Chromium" by default.
    // We need to find our actual prefix from `hyprctl globalshortcuts`.
    let prefix = match tokio::process::Command::new("hyprctl")
        .args(["globalshortcuts", "-j"])
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse JSON to find our shortcut prefix
            // Look for an entry containing ":toggle_recording"
            if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) {
                entries
                    .iter()
                    .find_map(|e| {
                        let name = e.get("name")?.as_str()?;
                        if name.ends_with(":toggle_recording") {
                            // Extract prefix before ":toggle_recording"
                            Some(name.strip_suffix(":toggle_recording")?.to_string())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "org.chromium.Chromium".to_string())
            } else {
                "org.chromium.Chromium".to_string()
            }
        }
        Err(e) => {
            tracing::warn!("Failed to query hyprctl globalshortcuts: {}", e);
            return;
        }
    };

    tracing::info!("Hyprland portal shortcut prefix: {}", prefix);

    // Bind each registered shortcut via hyprctl
    let shortcuts: Vec<(String, String)> = {
        let state = get_shortcut_state().read();
        state
            .shortcuts
            .values()
            .map(|s| (s.id.clone(), s.accelerator.clone()))
            .collect()
    };

    for (id, accelerator) in shortcuts {
        let hypr_bind = accelerator_to_hyprland_bind(&accelerator);
        let global_name = format!("{}:{}", prefix, id);

        let result = tokio::process::Command::new("hyprctl")
            .args(["keyword", "bind", &format!("{}, global, {}", hypr_bind, global_name)])
            .output()
            .await;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if stdout == "ok" {
                    tracing::info!(
                        "Hyprland bound '{}' → {} (portal: {})",
                        id,
                        hypr_bind,
                        global_name
                    );
                } else {
                    tracing::warn!(
                        "Hyprland bind failed for '{}': {}",
                        id,
                        stdout
                    );
                }
            }
            Err(e) => tracing::warn!("Failed to run hyprctl for '{}': {}", id, e),
        }
    }
}

/// Convert a Tauri accelerator to a Hyprland bind string.
///
/// Hyprland bind format: "MODS, key"
/// Examples:
///   "CommandOrControl+Shift+Space" → "CTRL SHIFT, Space"
///   "F13" → ", F13"
///   "ShiftRight" → ", Shift_R"
fn accelerator_to_hyprland_bind(accel: &str) -> String {
    let parts: Vec<&str> = accel.split('+').collect();
    let mut modifiers = Vec::new();
    let mut key = String::new();

    for part in &parts {
        match *part {
            "CommandOrControl" | "CmdOrCtrl" | "Control" | "Ctrl" => modifiers.push("CTRL"),
            "Shift" => modifiers.push("SHIFT"),
            "Alt" | "Option" => modifiers.push("ALT"),
            "Meta" | "Super" | "Command" | "Cmd" => modifiers.push("SUPER"),
            "ShiftRight" => key = "Shift_R".to_string(),
            "ShiftLeft" => key = "Shift_L".to_string(),
            "ControlRight" => key = "Control_R".to_string(),
            "ControlLeft" => key = "Control_L".to_string(),
            "AltRight" => key = "Alt_R".to_string(),
            "AltLeft" => key = "Alt_L".to_string(),
            "MetaRight" => key = "Super_R".to_string(),
            "MetaLeft" => key = "Super_L".to_string(),
            other => key = other.to_string(),
        }
    }

    let mods = modifiers.join(" ");
    format!("{}, {}", mods, key)
}

// ---------------------------------------------------------------------------
// Accelerator conversion
// ---------------------------------------------------------------------------

/// Convert a Tauri accelerator string to an XDG Portal trigger hint.
///
/// Portal triggers use XKB-style names. This is a best-effort hint —
/// the compositor may assign a different trigger and present a dialog.
fn accelerator_to_portal_trigger(accel: &str) -> String {
    let parts: Vec<&str> = accel.split('+').collect();
    let mut result = Vec::new();

    for part in &parts {
        match *part {
            "CommandOrControl" | "CmdOrCtrl" | "Control" | "Ctrl" => result.push("CTRL"),
            "Shift" => result.push("SHIFT"),
            "Alt" | "Option" => result.push("ALT"),
            "Meta" | "Super" | "Command" | "Cmd" => result.push("SUPER"),
            "Space" => result.push("space"),
            "ShiftRight" => result.push("Shift_R"),
            "ShiftLeft" => result.push("Shift_L"),
            "ControlRight" => result.push("Control_R"),
            "ControlLeft" => result.push("Control_L"),
            "AltRight" => result.push("Alt_R"),
            "AltLeft" => result.push("Alt_L"),
            "MetaRight" => result.push("Super_R"),
            "MetaLeft" => result.push("Super_L"),
            other => result.push(other),
        }
    }

    result.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accelerator_to_portal_trigger() {
        assert_eq!(
            accelerator_to_portal_trigger("CommandOrControl+Shift+Space"),
            "CTRL+SHIFT+space"
        );
        assert_eq!(accelerator_to_portal_trigger("F13"), "F13");
        assert_eq!(accelerator_to_portal_trigger("ShiftRight"), "Shift_R");
        assert_eq!(
            accelerator_to_portal_trigger("Alt+Shift+T"),
            "ALT+SHIFT+T"
        );
    }

    #[test]
    fn test_accelerator_to_hyprland_bind() {
        assert_eq!(
            accelerator_to_hyprland_bind("CommandOrControl+Shift+Space"),
            "CTRL SHIFT, Space"
        );
        assert_eq!(accelerator_to_hyprland_bind("F13"), ", F13");
        assert_eq!(accelerator_to_hyprland_bind("ShiftRight"), ", Shift_R");
    }
}
