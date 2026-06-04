//! Text insertion service for typing transcribed text
//!
//! Provides cross-platform text insertion at cursor position in any application.
//! Supports multiple insertion methods with configurable delays.

use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Method used to insert text into the target application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InsertionMethod {
    /// Type text character by character using keyboard simulation.
    /// Works with most applications but slower for long text.
    #[default]
    Typing,
    /// Paste from clipboard using Cmd+V (macOS) or Ctrl+Shift+V (Linux).
    /// Faster but temporarily modifies clipboard contents.
    Paste,
}

impl InsertionMethod {
    /// Parse insertion method from string.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "paste" | "clipboard" => Self::Paste,
            _ => Self::Typing,
        }
    }
}

/// Configuration for text insertion.
#[derive(Debug, Clone)]
pub struct InsertionConfig {
    /// Method to use for insertion.
    pub method: InsertionMethod,
    /// Delay between keystrokes in milliseconds (for typing method).
    pub keystroke_delay_ms: u64,
    /// Delay before starting insertion in milliseconds.
    pub initial_delay_ms: u64,
}

impl Default for InsertionConfig {
    fn default() -> Self {
        Self {
            method: InsertionMethod::Typing,
            keystroke_delay_ms: 0,
            initial_delay_ms: 50,
        }
    }
}

/// Text insertion service.
pub struct TextInsertService {
    config: InsertionConfig,
}

impl TextInsertService {
    /// Create a new text insertion service with default configuration.
    pub fn new() -> Self {
        Self {
            config: InsertionConfig::default(),
        }
    }

    /// Create a new text insertion service with custom configuration.
    pub fn with_config(config: InsertionConfig) -> Self {
        Self { config }
    }

    /// Insert text at the current cursor position.
    ///
    /// Uses the configured insertion method (typing or paste).
    pub fn insert_text(&self, text: &str) -> Result<(), String> {
        if text.is_empty() {
            debug!("Empty text provided, nothing to insert");
            return Ok(());
        }

        // Apply initial delay to allow focus to settle
        if self.config.initial_delay_ms > 0 {
            debug!(
                "Waiting {}ms before insertion",
                self.config.initial_delay_ms
            );
            thread::sleep(Duration::from_millis(self.config.initial_delay_ms));
        }

        match self.config.method {
            InsertionMethod::Typing => self.insert_by_typing(text),
            InsertionMethod::Paste => self.insert_by_paste(text),
        }
    }

    /// Insert text by simulating keyboard typing.
    fn insert_by_typing(&self, text: &str) -> Result<(), String> {
        debug!("Inserting {} characters by typing", text.len());

        #[cfg(target_os = "macos")]
        {
            self.insert_by_typing_macos(text)
        }

        #[cfg(target_os = "linux")]
        {
            self.insert_by_typing_linux(text)
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err("Text insertion not supported on this platform".to_string())
        }
    }

    /// Insert text by pasting from clipboard.
    ///
    /// Note: This function assumes the text is already in the clipboard.
    /// Clipboard preservation and restoration is handled by the clipboard module.
    fn insert_by_paste(&self, text: &str) -> Result<(), String> {
        debug!("Inserting {} characters by paste", text.len());

        // Set clipboard to new text
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;

        clipboard
            .set_text(text.to_string())
            .map_err(|e| format!("Failed to set clipboard: {}", e))?;

        // Small delay to ensure clipboard is ready
        thread::sleep(Duration::from_millis(10));

        // Perform paste
        #[cfg(target_os = "macos")]
        {
            self.paste_macos()?;
        }

        #[cfg(target_os = "linux")]
        {
            self.paste_linux()?;
        }

        // Note: Clipboard restoration is handled by the clipboard module
        // via paste_transcription -> restore_clipboard flow with configurable delay

        Ok(())
    }

    // ========================================================================
    // macOS-specific implementations
    // ========================================================================

    #[cfg(target_os = "macos")]
    fn insert_by_typing_macos(&self, text: &str) -> Result<(), String> {
        use enigo::{Enigo, Keyboard, Settings};

        let mut enigo = match Enigo::new(&Settings::default()) {
            Ok(e) => e,
            Err(e) => {
                warn!(
                    "Failed to initialise enigo: {}, falling back to AppleScript",
                    e
                );
                return self.type_text_applescript(text);
            }
        };

        // For text with special characters, multi-byte Unicode, or keystroke delay,
        // type character by character
        if self.config.keystroke_delay_ms > 0 || !text.is_ascii() {
            self.type_chars_with_enigo(&mut enigo, text)?;
        } else if let Err(e) = enigo.text(text) {
            warn!(
                "Enigo text insertion failed: {}, falling back to AppleScript",
                e
            );
            return self.type_text_applescript(text);
        }

        info!("Successfully inserted {} characters via enigo", text.len());
        Ok(())
    }

    /// Type text character by character via enigo, falling back to AppleScript per character.
    #[cfg(target_os = "macos")]
    fn type_chars_with_enigo(&self, enigo: &mut enigo::Enigo, text: &str) -> Result<(), String> {
        use enigo::Keyboard;

        for c in text.chars() {
            if let Err(e) = enigo.text(&c.to_string()) {
                warn!("Failed to type character '{}': {}", c, e);
                self.type_char_applescript(c)?;
            }
            if self.config.keystroke_delay_ms > 0 {
                thread::sleep(Duration::from_millis(self.config.keystroke_delay_ms));
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn paste_macos(&self) -> Result<(), String> {
        // Synthesise Cmd+V with a Core Graphics event, the standard way macOS
        // dictation/paste tools inject a paste. Replaces the previous approach of
        // shelling out to `osascript`, which: (1) required a SECOND TCC grant
        // (Automation, to drive System Events) on top of Accessibility; (2) forked
        // an interpreter on the hot paste path. `CGEventPost` is callable from any
        // thread (it does not touch the main-thread-only AppKit input machinery),
        // so it also resolves the off-main-thread enigo crash that drove the
        // osascript workaround — with only the Accessibility permission.
        post_paste_cgevent()
    }

    #[cfg(target_os = "macos")]
    fn type_text_applescript(&self, text: &str) -> Result<(), String> {
        use std::process::Command;

        // Escape special characters for AppleScript
        let escaped = escape_for_applescript(text);

        let script = format!(
            "tell application \"System Events\" to keystroke \"{}\"",
            escaped
        );

        let output = Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|e| format!("Failed to execute AppleScript: {}", e))?;

        if output.status.success() {
            info!(
                "Successfully inserted {} characters via AppleScript",
                text.len()
            );
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("AppleScript keystroke failed: {}", stderr))
        }
    }

    #[cfg(target_os = "macos")]
    fn type_char_applescript(&self, c: char) -> Result<(), String> {
        self.type_text_applescript(&c.to_string())
    }

    // ========================================================================
    // Linux-specific implementations
    // ========================================================================

    #[cfg(target_os = "linux")]
    fn insert_by_typing_linux(&self, text: &str) -> Result<(), String> {
        // Try wtype first (native Wayland support)
        if Self::try_type_with_wtype(text, self.config.keystroke_delay_ms) {
            info!("Successfully inserted {} characters via wtype", text.len());
            return Ok(());
        }

        // Fall back to enigo (X11/XWayland). On native Wayland this is where the
        // user feels a missing `wtype`: enigo drives XWayland and on GNOME
        // triggers the "Allow Remote Interaction" prompt. Logged at warn (not
        // debug) so the cause is visible; the startup advisory toast
        // (`emit_linux_typing_advisory`) tells the user how to fix it.
        warn!("wtype unavailable or failed; falling back to enigo for typing (Wayland users: install wtype or grant Remote Interaction)");
        Self::type_with_enigo(text, self.config.keystroke_delay_ms)
    }

    #[cfg(target_os = "linux")]
    fn try_type_with_wtype(text: &str, keystroke_delay_ms: u64) -> bool {
        use std::process::Command;

        // wtype is a native Wayland tool (requires virtual-keyboard protocol support)
        if keystroke_delay_ms > 0 {
            // Type character by character with delay
            for c in text.chars() {
                if c.is_control() {
                    continue;
                }
                match Command::new("wtype").arg(c.to_string()).status() {
                    Ok(status) if status.success() => {}
                    _ => return false,
                }
                thread::sleep(Duration::from_millis(keystroke_delay_ms));
            }
            true
        } else {
            // Type all at once
            Command::new("wtype")
                .arg(text)
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
    }

    #[cfg(target_os = "linux")]
    fn type_with_enigo(text: &str, keystroke_delay_ms: u64) -> Result<(), String> {
        use enigo::{Enigo, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialise enigo: {}", e))?;

        if keystroke_delay_ms > 0 {
            for c in text.chars() {
                if let Err(e) = enigo.text(&c.to_string()) {
                    return Err(format!("Failed to type character '{}': {}", c, e));
                }
                thread::sleep(Duration::from_millis(keystroke_delay_ms));
            }
        } else {
            enigo
                .text(text)
                .map_err(|e| format!("Failed to type text: {}", e))?;
        }

        info!("Successfully inserted {} characters via enigo", text.len());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn paste_linux(&self) -> Result<(), String> {
        // Try wtype first (native Wayland support, no modifier key issues)
        if Self::try_paste_with_wtype() {
            debug!("Pasted via wtype (native Wayland)");
            return Ok(());
        }

        // Fall back to enigo (X11/XWayland). See insert_by_typing_linux for why
        // this is the Wayland pain point; warn so the cause is visible.
        warn!("wtype unavailable or failed; falling back to enigo for paste (Wayland users: install wtype or grant Remote Interaction)");
        Self::paste_with_enigo()
    }

    #[cfg(target_os = "linux")]
    fn try_paste_with_wtype() -> bool {
        use std::process::Command;

        // wtype is a native Wayland tool that avoids modifier key sync issues
        // Note: Only works on compositors supporting the virtual-keyboard protocol (Sway, Hyprland, etc.)
        // Falls back to enigo on GNOME which triggers the "Allow Remote Interaction" dialog.
        //
        // Use Ctrl+Shift+V, not Ctrl+V: terminal emulators (GNOME Terminal, Konsole,
        // Alacritty, kitty, foot, xterm) reserve Ctrl+V for other functions and only
        // accept Ctrl+Shift+V as paste, so a plain Ctrl+V pasted nothing in a
        // terminal. Ctrl+Shift+V also pastes correctly in mainstream GUI apps
        // (browsers, editors, GTK/Qt text fields), so it is the single safe binding.
        match Command::new("wtype")
            .args([
                "-M", "ctrl", "-M", "shift", "v", "-m", "shift", "-m", "ctrl",
            ])
            .status()
        {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    #[cfg(target_os = "linux")]
    fn paste_with_enigo() -> Result<(), String> {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialise enigo: {}", e))?;

        // Synthesise Ctrl+Shift+V (not Ctrl+V): terminal emulators only accept the
        // shifted form as paste, and it also works in mainstream GUI apps. Hold
        // both modifiers around the V click, then release in reverse press order
        // (Shift, then Control), and always release both even if the click errors
        // so we never leave a modifier stuck down.
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| format!("Failed to press Control: {}", e))?;
        if let Err(e) = enigo.key(Key::Shift, Direction::Press) {
            // Release Control before bailing so it isn't left held.
            let _ = enigo.key(Key::Control, Direction::Release);
            return Err(format!("Failed to press Shift: {}", e));
        }

        let click_result = enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("Failed to press V: {}", e));

        if let Err(e) = enigo.key(Key::Shift, Direction::Release) {
            tracing::error!("Failed to release Shift key: {}", e);
        }
        if let Err(e) = enigo.key(Key::Control, Direction::Release) {
            tracing::error!("Failed to release Control key: {}", e);
        }

        click_result?;
        debug!("Pasted via enigo (Ctrl+Shift+V)");
        Ok(())
    }
}

impl Default for TextInsertService {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the `wtype` binary is available on `PATH` (Linux only).
///
/// `wtype` is the native Wayland virtual-keyboard tool and the preferred typing
/// backend; it is not installed by default on most desktops. Absence is not
/// fatal (enigo via XWayland is the fallback) but on GNOME Wayland the fallback
/// triggers a permission prompt, so it is worth telling the user.
///
/// Detection is by `PATH` lookup rather than executing `wtype`: `wtype` has no
/// `--version`/`--help` flag (it would interpret the argument as text to type),
/// so running it to probe would mis-report and could emit a keystroke.
#[cfg(target_os = "linux")]
pub fn wtype_available() -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join("wtype").is_file())
}

/// On Linux/Wayland without `wtype`, emit a one-time advisory so the user knows
/// why text insertion may prompt for permission and how to make it seamless.
/// Called once at startup; no-op on X11, macOS, or when `wtype` is present.
///
/// The frontend listens for `text-insertion-advisory` and shows a toast. The
/// insertion path itself has no `AppHandle`, so the advice is surfaced here at
/// startup rather than on every insertion.
#[cfg(target_os = "linux")]
pub fn emit_linux_typing_advisory(app: &tauri::AppHandle) {
    use tauri::Emitter;

    if crate::shortcuts::get_display_server() != crate::shortcuts::DisplayServer::Wayland {
        return;
    }
    if wtype_available() {
        return;
    }

    let message = "For seamless text insertion on Wayland, install `wtype`. Without it, Thoth \
                   falls back to XWayland, which on GNOME asks for the \"Allow Remote \
                   Interaction\" permission each session.";
    tracing::info!("{message}");
    if let Err(e) = app.emit("text-insertion-advisory", message) {
        tracing::error!("Failed to emit text-insertion-advisory event: {e}");
    }
}

/// Synthesise a Cmd+V keystroke via Core Graphics to paste the clipboard.
///
/// Posts a key-down (V with the Command flag) followed by a key-up to the HID
/// event tap. Uses an event source in `HIDSystemState` so the synthetic event
/// behaves like real hardware input. Requires only the Accessibility permission
/// and is safe to call from any thread.
#[cfg(target_os = "macos")]
fn post_paste_cgevent() -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    /// ANSI virtual key code for the V key.
    const KEY_V: u16 = 0x09;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create CGEventSource for paste".to_string())?;

    let key_down = CGEvent::new_keyboard_event(source.clone(), KEY_V, true)
        .map_err(|_| "Failed to create key-down event for paste".to_string())?;
    key_down.set_flags(CGEventFlags::CGEventFlagCommand);
    key_down.post(CGEventTapLocation::HID);

    let key_up = CGEvent::new_keyboard_event(source, KEY_V, false)
        .map_err(|_| "Failed to create key-up event for paste".to_string())?;
    key_up.set_flags(CGEventFlags::CGEventFlagCommand);
    key_up.post(CGEventTapLocation::HID);

    debug!("Pasted via CGEvent Cmd+V");
    Ok(())
}

/// Escape special characters for AppleScript string.
///
/// Escapes backslashes, double quotes, and all control characters
/// to prevent AppleScript injection.
#[cfg(target_os = "macos")]
fn escape_for_applescript(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {} // Strip other control characters
            c => escaped.push(c),
        }
    }
    escaped
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Insert text at the current cursor position using the typing method.
///
/// This simulates keyboard input to type the text character by character.
/// Works with most applications but may be slower for long text.
///
/// # Arguments
///
/// * `text` - The text to insert
/// * `keystroke_delay_ms` - Optional delay between keystrokes in milliseconds
/// * `initial_delay_ms` - Optional delay before starting insertion
///
/// # Returns
///
/// `Ok(())` on success, or an error message on failure.
#[tauri::command]
pub fn insert_text_by_typing(
    text: String,
    keystroke_delay_ms: Option<u64>,
    initial_delay_ms: Option<u64>,
) -> Result<(), String> {
    let config = InsertionConfig {
        method: InsertionMethod::Typing,
        keystroke_delay_ms: keystroke_delay_ms.unwrap_or(0),
        initial_delay_ms: initial_delay_ms.unwrap_or(50),
    };

    let service = TextInsertService::with_config(config);
    service.insert_text(&text)
}

/// Insert text at the current cursor position using clipboard paste.
///
/// This copies the text to clipboard and simulates Cmd+V (macOS) or Ctrl+Shift+V (Linux).
/// Faster than typing but temporarily modifies clipboard contents.
/// The original clipboard content is restored after pasting.
///
/// # Arguments
///
/// * `text` - The text to insert
/// * `initial_delay_ms` - Optional delay before starting insertion
///
/// # Returns
///
/// `Ok(())` on success, or an error message on failure.
#[tauri::command]
pub fn insert_text_by_paste(text: String, initial_delay_ms: Option<u64>) -> Result<(), String> {
    let config = InsertionConfig {
        method: InsertionMethod::Paste,
        keystroke_delay_ms: 0,
        initial_delay_ms: initial_delay_ms.unwrap_or(50),
    };

    let service = TextInsertService::with_config(config);
    service.insert_text(&text)
}

/// Insert text at the current cursor position.
///
/// This is a convenience command that uses the default insertion method (typing).
/// For more control, use `insert_text_by_typing` or `insert_text_by_paste`.
///
/// # Arguments
///
/// * `text` - The text to insert
/// * `method` - Optional insertion method ("typing" or "paste", defaults to "typing")
///
/// # Returns
///
/// `Ok(())` on success, or an error message on failure.
#[tauri::command]
pub fn insert_text(text: String, method: Option<String>) -> Result<(), String> {
    let insertion_method = method
        .as_deref()
        .map(InsertionMethod::parse)
        .unwrap_or_default();

    let config = InsertionConfig {
        method: insertion_method,
        ..Default::default()
    };

    let service = TextInsertService::with_config(config);
    service.insert_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insertion_method_parse() {
        assert_eq!(InsertionMethod::parse("typing"), InsertionMethod::Typing);
        assert_eq!(InsertionMethod::parse("paste"), InsertionMethod::Paste);
        assert_eq!(InsertionMethod::parse("clipboard"), InsertionMethod::Paste);
        assert_eq!(InsertionMethod::parse("PASTE"), InsertionMethod::Paste);
        assert_eq!(InsertionMethod::parse("unknown"), InsertionMethod::Typing);
    }

    #[test]
    fn test_insertion_config_default() {
        let config = InsertionConfig::default();
        assert_eq!(config.method, InsertionMethod::Typing);
        assert_eq!(config.keystroke_delay_ms, 0);
        assert_eq!(config.initial_delay_ms, 50);
    }

    #[test]
    fn test_text_insert_service_creation() {
        let service = TextInsertService::new();
        assert_eq!(service.config.method, InsertionMethod::Typing);
    }

    #[test]
    fn test_text_insert_service_with_config() {
        let config = InsertionConfig {
            method: InsertionMethod::Paste,
            keystroke_delay_ms: 10,
            initial_delay_ms: 100,
        };
        let service = TextInsertService::with_config(config);
        assert_eq!(service.config.method, InsertionMethod::Paste);
        assert_eq!(service.config.keystroke_delay_ms, 10);
        assert_eq!(service.config.initial_delay_ms, 100);
    }

    #[test]
    fn test_empty_text_insertion() {
        let service = TextInsertService::new();
        let result = service.insert_text("");
        assert!(result.is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_escape_for_applescript() {
        assert_eq!(escape_for_applescript("hello"), "hello");
        assert_eq!(escape_for_applescript("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_for_applescript("path\\to"), "path\\\\to");
    }
}
