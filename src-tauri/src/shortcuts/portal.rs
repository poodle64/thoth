//! XDG Desktop Portal GlobalShortcuts backend for Wayland.
//!
//! On Hyprland, portal signal delivery via D-Bus is unreliable (the ashpd zbus
//! connection silently dies when launched from app launchers). Instead we use
//! `hyprctl keyword bind` with the `exec` dispatcher to write shortcut IDs to
//! a Unix socket. For other compositors the standard portal signal path is used.

use crate::recording_indicator;
use crate::shortcuts::manager::{shortcut_ids, ShortcutInfo};
use ashpd::desktop::global_shortcuts::GlobalShortcuts;
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct ShortcutEvent {
    pub id: String,
    pub state: String,
}

fn fifo_path() -> String {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        format!("{}/thoth-shortcuts.fifo", runtime_dir)
    } else {
        format!("/tmp/thoth-shortcuts-{}.fifo", std::process::id())
    }
}

const PRESS_DEBOUNCE_MS: u64 = 50;

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

struct PortalSession {
    proxy: GlobalShortcuts<'static>,
    session: ashpd::desktop::Session<'static, GlobalShortcuts<'static>>,
}

static PORTAL_SESSION: tokio::sync::OnceCell<PortalSession> = tokio::sync::OnceCell::const_new();

/// Initialize shortcut listeners on Wayland.
/// On Hyprland: uses native hyprctl binds + Unix socket (no portal needed).
/// On other compositors: uses XDG Desktop Portal signal streams.
pub async fn init(app: AppHandle) -> Result<(), String> {
    use crate::platform::linux::{display_session, DisplaySession, WaylandCompositor};

    let compositor = match display_session() {
        DisplaySession::Wayland(c) => *c,
        _ => WaylandCompositor::None,
    };

    if compositor == WaylandCompositor::Hyprland {
        tracing::info!("Hyprland detected: using native hyprctl binds (skipping portal)");
        start_fifo_listener(app.clone());
        hyprland_bind_shortcuts().await;
        return Ok(());
    }

    if compositor == WaylandCompositor::Sway {
        tracing::info!("Sway detected: using native swaymsg bindsym (skipping portal)");
        start_fifo_listener(app.clone());
        sway_bind_shortcuts().await;
        return Ok(());
    }

    // Non-Hyprland: use XDG Desktop Portal signals
    use ashpd::desktop::global_shortcuts::NewShortcut;
    use futures_util::StreamExt;

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

    let shortcuts_to_bind: Vec<NewShortcut> = {
        let state = get_shortcut_state().read();
        state
            .shortcuts
            .values()
            .map(|s| {
                let trigger = accelerator_to_portal_trigger(&s.accelerator);
                NewShortcut::new(&s.id, &s.description).preferred_trigger(Some(trigger.as_str()))
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

    tracing::info!("Portal GlobalShortcuts signal listeners started");
    Ok(())
}

fn handle_activated(app: &AppHandle, shortcut_id: &str) {
    if crate::keyboard_service::is_capture_active() {
        tracing::debug!(
            "Discarding portal shortcut '{}' — capture mode active",
            shortcut_id
        );
        return;
    }

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

    let event = ShortcutEvent {
        id: shortcut_id.to_string(),
        state: "pressed".to_string(),
    };
    let _ = app.emit("shortcut-triggered", shortcut_id.to_string());
    let _ = app.emit("shortcut-pressed", &event);
}

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

pub fn unregister(id: &str) -> Result<(), String> {
    {
        let mut state = get_shortcut_state().write();
        state.shortcuts.remove(id);
    }
    tracing::info!("Portal: unregistered shortcut '{}'", id);

    spawn_rebind();
    Ok(())
}

pub fn list_registered() -> Vec<ShortcutInfo> {
    let state = get_shortcut_state().read();
    state.shortcuts.values().cloned().collect()
}

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
// Async rebind
// ---------------------------------------------------------------------------

fn spawn_rebind() {
    if PORTAL_SESSION.get().is_none() {
        return;
    }
    tauri::async_runtime::spawn(async {
        if let Err(e) = rebind().await {
            tracing::warn!("Failed to rebind portal shortcuts: {}", e);
        }
    });
}

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
                NewShortcut::new(&s.id, &s.description).preferred_trigger(Some(trigger.as_str()))
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

    hyprland_bind_shortcuts().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Hyprland socket-based shortcut IPC
// ---------------------------------------------------------------------------

fn start_fifo_listener(app: AppHandle) {
    use std::fs;
    use std::io::{BufRead, BufReader};
    use std::process::Command;

    let path = fifo_path();
    let _ = fs::remove_file(&path);

    // Create the FIFO via mkfifo command
    match Command::new("mkfifo").arg(&path).status() {
        Ok(s) if s.success() => {}
        Ok(s) => {
            tracing::error!("mkfifo failed with status {}", s);
            return;
        }
        Err(e) => {
            tracing::error!("Failed to run mkfifo: {}", e);
            return;
        }
    }

    tracing::info!("Hyprland shortcut FIFO listening at {}", path);

    std::thread::spawn(move || {
        loop {
            // Open FIFO for reading. Blocks until a writer opens it.
            let file = match fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("Failed to open FIFO for reading: {}", e);
                    return;
                }
            };

            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(shortcut_id) = line {
                    let id = shortcut_id.trim();
                    if id.is_empty() || id.starts_with("__") {
                        continue;
                    }
                    handle_activated(&app, id);
                }
            }
            // Writer closed (EOF) — loop back to reopen and wait for next write
        }
    });
}

async fn hyprland_bind_shortcuts() {
    let is_hyprland = {
        use crate::platform::linux::{display_session, DisplaySession, WaylandCompositor};
        matches!(
            display_session(),
            DisplaySession::Wayland(WaylandCompositor::Hyprland)
        )
    };
    if !is_hyprland {
        return;
    }

    let fifo = fifo_path();

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
        // Modifier-only keys (Shift_R, Shift_L, etc.) need `bindr` (fire on release)
        let is_modifier_only = matches!(
            accelerator.as_str(),
            "ShiftRight" | "ShiftLeft" | "ControlRight" | "ControlLeft" | "AltRight" | "AltLeft"
        );
        let bind_type = if is_modifier_only { "bindr" } else { "bind" };

        // Unbind both `bind` and `bindr` variants to clear stale binds
        for unbind_type in &["unbind", "unbindr"] {
            let _ = tokio::process::Command::new("hyprctl")
                .args(["keyword", unbind_type, &hypr_bind])
                .output()
                .await;
        }

        // Also try title-case key name (e.g. "Space" vs "space")
        let parts: Vec<&str> = hypr_bind.splitn(2, ", ").collect();
        if parts.len() == 2 {
            let mut chars = parts[1].chars();
            let title_case: String = match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            };
            if title_case != parts[1] {
                let alt_bind = format!("{}, {}", parts[0], title_case);
                for unbind_type in &["unbind", "unbindr"] {
                    let _ = tokio::process::Command::new("hyprctl")
                        .args(["keyword", unbind_type, &alt_bind])
                        .output()
                        .await;
                }
            }
        }

        // Use shell built-in: echo > FIFO (no socat/nc needed)
        let exec_cmd = format!("echo {} > {}", id, fifo);
        let bind_arg = format!("{}, exec, {}", hypr_bind, exec_cmd);

        tracing::info!("Running: hyprctl keyword {} \"{}\"", bind_type, bind_arg);

        let result = tokio::process::Command::new("hyprctl")
            .args(["keyword", bind_type, &bind_arg])
            .output()
            .await;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if stdout == "ok" {
                    tracing::info!(
                        "Hyprland {} '{}' → {} (exec → FIFO)",
                        bind_type,
                        id,
                        hypr_bind,
                    );
                } else {
                    tracing::warn!(
                        "Hyprland {} failed for '{}': stdout='{}' stderr='{}'",
                        bind_type,
                        id,
                        stdout,
                        stderr
                    );
                }
            }
            Err(e) => tracing::warn!("Failed to run hyprctl for '{}': {}", id, e),
        }
    }

    // Self-test: write to FIFO via shell
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    match tokio::process::Command::new("sh")
        .args(["-c", &format!("echo __ping > {}", fifo)])
        .output()
        .await
    {
        Ok(out) if out.status.success() => {
            tracing::info!("Hyprland shortcut self-test: FIFO reachable");
        }
        Ok(out) => {
            tracing::warn!(
                "Hyprland shortcut self-test: write failed (exit {})",
                out.status
            );
        }
        Err(e) => {
            tracing::warn!("Hyprland shortcut self-test: failed to write: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// Sway native shortcut bindings (swaymsg)
// ---------------------------------------------------------------------------

async fn sway_bind_shortcuts() {
    let is_sway = {
        use crate::platform::linux::{display_session, DisplaySession, WaylandCompositor};
        matches!(
            display_session(),
            DisplaySession::Wayland(WaylandCompositor::Sway)
        )
    };
    if !is_sway {
        return;
    }

    let fifo = fifo_path();

    let shortcuts: Vec<(String, String)> = {
        let state = get_shortcut_state().read();
        state
            .shortcuts
            .values()
            .map(|s| (s.id.clone(), s.accelerator.clone()))
            .collect()
    };

    for (id, accelerator) in shortcuts {
        let sway_binding = accelerator_to_sway_bind(&accelerator);
        let is_modifier_only = matches!(
            accelerator.as_str(),
            "ShiftRight" | "ShiftLeft" | "ControlRight" | "ControlLeft" | "AltRight" | "AltLeft"
        );

        // Unbind first to avoid duplicates
        let unbind_cmd = format!("unbindsym {}", sway_binding);
        let _ = tokio::process::Command::new("swaymsg")
            .arg(&unbind_cmd)
            .output()
            .await;

        // For modifier-only keys, use --release flag
        let exec_cmd = format!("echo {} > {}", id, fifo);
        let bind_cmd = if is_modifier_only {
            format!("bindsym --release {} exec '{}'", sway_binding, exec_cmd)
        } else {
            format!("bindsym {} exec '{}'", sway_binding, exec_cmd)
        };

        tracing::info!("Running: swaymsg '{}'", bind_cmd);

        let result = tokio::process::Command::new("swaymsg")
            .arg(&bind_cmd)
            .output()
            .await;

        match result {
            Ok(output) if output.status.success() => {
                tracing::info!("Sway bound '{}' → {} (exec → FIFO)", id, sway_binding);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                tracing::warn!("Sway bind failed for '{}': {}", id, stderr);
            }
            Err(e) => tracing::warn!("Failed to run swaymsg for '{}': {}", id, e),
        }
    }

    // Self-test
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    match tokio::process::Command::new("sh")
        .args(["-c", &format!("echo __ping > {}", fifo)])
        .output()
        .await
    {
        Ok(out) if out.status.success() => {
            tracing::info!("Sway shortcut self-test: FIFO reachable");
        }
        _ => {
            tracing::warn!("Sway shortcut self-test: FIFO may not be reachable");
        }
    }
}

// ---------------------------------------------------------------------------
// Accelerator conversion
// ---------------------------------------------------------------------------

fn accelerator_to_sway_bind(accel: &str) -> String {
    let parts: Vec<&str> = accel.split('+').collect();
    let mut components = Vec::new();

    for part in &parts {
        match *part {
            "CommandOrControl" | "CmdOrCtrl" | "Control" | "Ctrl" => components.push("Ctrl"),
            "Shift" => components.push("Shift"),
            "Alt" | "Option" => components.push("Alt"),
            "Meta" | "Super" | "Command" | "Cmd" => components.push("Mod4"),
            "ShiftRight" => components.push("Shift_R"),
            "ShiftLeft" => components.push("Shift_L"),
            "ControlRight" => components.push("Control_R"),
            "ControlLeft" => components.push("Control_L"),
            "AltRight" => components.push("Alt_R"),
            "AltLeft" => components.push("Alt_L"),
            other => components.push(other),
        }
    }

    components.join("+")
}

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
            // Use keycodes for modifier-only keys (more reliable across keyboards)
            "ShiftRight" => key = "code:62".to_string(),
            "ShiftLeft" => key = "code:42".to_string(),
            "ControlRight" => key = "code:97".to_string(),
            "ControlLeft" => key = "code:29".to_string(),
            "AltRight" => key = "code:100".to_string(),
            "AltLeft" => key = "code:56".to_string(),
            other => key = other.to_lowercase(),
        }
    }

    let mods = modifiers.join(" ");
    format!("{}, {}", mods, key)
}

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
    }

    #[test]
    fn test_accelerator_to_hyprland_bind() {
        assert_eq!(
            accelerator_to_hyprland_bind("CommandOrControl+Shift+Space"),
            "CTRL SHIFT, space"
        );
        assert_eq!(accelerator_to_hyprland_bind("F13"), ", f13");
        assert_eq!(accelerator_to_hyprland_bind("ShiftRight"), ", code:62");
    }

    #[test]
    fn test_accelerator_to_sway_bind() {
        assert_eq!(
            accelerator_to_sway_bind("CommandOrControl+Shift+Space"),
            "Ctrl+Shift+Space"
        );
        assert_eq!(accelerator_to_sway_bind("ShiftRight"), "Shift_R");
    }
}
