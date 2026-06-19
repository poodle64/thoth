//! Native Hyprland global shortcuts via `hyprctl` binds + a FIFO.
//!
//! On Hyprland the XDG GlobalShortcuts portal registers a shortcut but never
//! actually binds a key (the binding comes back empty), so global hotkeys never
//! fire. Instead we bind directly with `hyprctl keyword bind`, whose `exec`
//! dispatcher writes the shortcut id to a FIFO we listen on; each id is then
//! routed through [`super::manager::dispatch_shortcut_action`] — the same
//! dispatcher the X11/macOS plugin callback uses, so behaviour is identical.

use std::process::Command;
use tauri::{AppHandle, Runtime};

/// Whether the current session is Hyprland.
pub fn is_hyprland() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

fn fifo_path() -> String {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => format!("{dir}/thoth-shortcuts.fifo"),
        _ => format!("/tmp/thoth-shortcuts-{}.fifo", std::process::id()),
    }
}

/// Set up native Hyprland binds and the FIFO listener.
pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    start_fifo_listener(app.clone());
    bind_shortcuts();
}

/// Convert a Tauri accelerator ("CommandOrControl+Shift+Space", "ShiftRight")
/// into a Hyprland "MODS, key" bind string. Modifier-only keys use keycodes,
/// which are more reliable than key names across keyboard layouts.
fn accelerator_to_hyprland_bind(accel: &str) -> String {
    let mut modifiers = Vec::new();
    let mut key = String::new();
    for part in accel.split('+') {
        match part {
            "CommandOrControl" | "CmdOrCtrl" | "Control" | "Ctrl" => modifiers.push("CTRL"),
            "Shift" => modifiers.push("SHIFT"),
            "Alt" | "Option" => modifiers.push("ALT"),
            "Meta" | "Super" | "Command" | "Cmd" => modifiers.push("SUPER"),
            "ShiftRight" => key = "code:62".to_string(),
            "ShiftLeft" => key = "code:42".to_string(),
            "ControlRight" => key = "code:97".to_string(),
            "ControlLeft" => key = "code:29".to_string(),
            "AltRight" => key = "code:100".to_string(),
            "AltLeft" => key = "code:56".to_string(),
            other => key = other.to_lowercase(),
        }
    }
    format!("{}, {}", modifiers.join(" "), key)
}

/// Modifier-only accelerators fire on release, so they need `bindr`, not `bind`.
fn is_modifier_only(accel: &str) -> bool {
    matches!(
        accel,
        "ShiftRight" | "ShiftLeft" | "ControlRight" | "ControlLeft" | "AltRight" | "AltLeft"
    )
}

/// Bind the configured shortcuts through `hyprctl`.
fn bind_shortcuts() {
    use crate::shortcuts::manager::shortcut_ids;

    let cfg = match crate::config::get_config() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Hyprland binds: failed to load config: {e}");
            return;
        }
    };

    let fifo = fifo_path();
    let mut binds: Vec<(&str, String)> = vec![(
        shortcut_ids::TOGGLE_RECORDING,
        cfg.shortcuts.toggle_recording.clone(),
    )];
    if let Some(alt) = cfg.shortcuts.toggle_recording_alt.clone() {
        binds.push((shortcut_ids::TOGGLE_RECORDING_ALT, alt));
    }
    if let Some(copy) = cfg.shortcuts.copy_last.clone() {
        binds.push((shortcut_ids::COPY_LAST_TRANSCRIPTION, copy));
    }

    for (id, accel) in binds {
        if accel.is_empty() {
            continue;
        }
        let bind = accelerator_to_hyprland_bind(&accel);
        let bind_type = if is_modifier_only(&accel) {
            "bindr"
        } else {
            "bind"
        };

        // Clear any stale binds (both variants) before rebinding.
        for unbind in ["unbind", "unbindr"] {
            let _ = Command::new("hyprctl")
                .args(["keyword", unbind, &bind])
                .output();
        }

        let dispatch = format!("{bind}, exec, echo {id} > {fifo}");
        match Command::new("hyprctl")
            .args(["keyword", bind_type, &dispatch])
            .output()
        {
            Ok(o) if o.status.success() => {
                tracing::info!("Hyprland bind '{id}' -> {bind} ({bind_type})");
            }
            Ok(o) => tracing::error!(
                "hyprctl {bind_type} failed for '{id}': {}",
                String::from_utf8_lossy(&o.stderr)
            ),
            Err(e) => tracing::error!("hyprctl {bind_type} failed for '{id}': {e}"),
        }
    }
}

/// Create the FIFO and route each id written to it through the dispatcher.
fn start_fifo_listener<R: Runtime>(app: AppHandle<R>) {
    use std::fs;
    use std::io::{BufRead, BufReader};

    let path = fifo_path();
    let _ = fs::remove_file(&path);
    match Command::new("mkfifo").arg(&path).status() {
        Ok(s) if s.success() => {}
        Ok(s) => {
            tracing::error!("mkfifo failed with status {s}");
            return;
        }
        Err(e) => {
            tracing::error!("Failed to run mkfifo: {e}");
            return;
        }
    }
    tracing::info!("Hyprland shortcut FIFO listening at {path}");

    std::thread::spawn(move || {
        loop {
            // Opening blocks until a writer (the hyprctl `exec`) opens the FIFO.
            let file = match fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("Failed to open FIFO for reading: {e}");
                    return;
                }
            };
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                let id = line.trim();
                if id.is_empty() {
                    continue;
                }
                crate::shortcuts::manager::dispatch_shortcut_action(&app, id);
            }
            // Writer closed (EOF) — loop back, reopen, wait for the next write.
        }
    });
}
