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
    /// Paste from clipboard using Cmd+V (macOS) or Ctrl+V (Linux/Windows).
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

        // Try enigo first (preferred method)
        match Enigo::new(&Settings::default()) {
            Ok(mut enigo) => {
                // For text with special characters or multi-byte Unicode,
                // use character-by-character insertion
                if self.config.keystroke_delay_ms > 0 || !text.is_ascii() {
                    for c in text.chars() {
                        if let Err(e) = enigo.text(&c.to_string()) {
                            warn!("Failed to type character '{}': {}", c, e);
                            // Fall back to AppleScript for this character
                            self.type_char_applescript(c)?;
                        }
                        if self.config.keystroke_delay_ms > 0 {
                            thread::sleep(Duration::from_millis(self.config.keystroke_delay_ms));
                        }
                    }
                } else {
                    // For plain ASCII text without delay, type all at once
                    if let Err(e) = enigo.text(text) {
                        warn!(
                            "Enigo text insertion failed: {}, falling back to AppleScript",
                            e
                        );
                        return self.type_text_applescript(text);
                    }
                }
                info!("Successfully inserted {} characters via enigo", text.len());
                Ok(())
            }
            Err(e) => {
                warn!(
                    "Failed to initialise enigo: {}, falling back to AppleScript",
                    e
                );
                self.type_text_applescript(text)
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn paste_macos(&self) -> Result<(), String> {
        // Use AppleScript for paste because enigo requires main thread access on macOS.
        // When called from async context (tokio worker thread), enigo crashes with
        // "dispatch_assert_queue_fail" because macOS input APIs require main thread.
        self.paste_applescript()
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

    #[cfg(target_os = "macos")]
    fn paste_applescript(&self) -> Result<(), String> {
        use std::process::Command;

        let script = "tell application \"System Events\" to keystroke \"v\" using command down";

        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .map_err(|e| format!("Failed to execute AppleScript: {}", e))?;

        if output.status.success() {
            debug!("Pasted via AppleScript");
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("AppleScript paste failed: {}", stderr))
        }
    }

    // ========================================================================
    // Linux-specific implementations
    // ========================================================================

    #[cfg(target_os = "linux")]
    fn insert_by_typing_linux(&self, text: &str) -> Result<(), String> {
        use enigo::{Enigo, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialise enigo: {}", e))?;

        if self.config.keystroke_delay_ms > 0 {
            for c in text.chars() {
                if let Err(e) = enigo.text(&c.to_string()) {
                    return Err(format!("Failed to type character '{}': {}", c, e));
                }
                thread::sleep(Duration::from_millis(self.config.keystroke_delay_ms));
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
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};

        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialise enigo: {}", e))?;

        // Press Ctrl+V
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| format!("Failed to press Control: {}", e))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("Failed to press V: {}", e))?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| format!("Failed to release Control: {}", e))?;

        debug!("Pasted via enigo");
        Ok(())
    }
}

impl Default for TextInsertService {
    fn default() -> Self {
        Self::new()
    }
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
/// This copies the text to clipboard and simulates Cmd+V (macOS) or Ctrl+V (Linux).
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
