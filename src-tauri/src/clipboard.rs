//! Clipboard integration module
//!
//! Provides smart clipboard operations including auto-copy on transcription
//! completion, clipboard history, and configurable formatting options.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tracing::{debug, error, info};

/// Maximum number of items to keep in clipboard history
const MAX_HISTORY_SIZE: usize = 50;

/// Clipboard format options for copying transcriptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardFormat {
    /// Plain text format (default)
    #[default]
    PlainText,
    /// Rich text with basic formatting preserved
    RichText,
    /// Markdown format
    Markdown,
}

/// Clipboard settings for auto-copy behaviour
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSettings {
    /// Whether to automatically copy transcription on completion
    pub auto_copy_enabled: bool,
    /// Format to use when copying
    pub format: ClipboardFormat,
    /// Whether to show notification on copy
    pub show_notification: bool,
    /// Whether to preserve original clipboard content (restore after paste)
    pub preserve_clipboard: bool,
    /// Delay in milliseconds before restoring clipboard (default 1000ms)
    pub restore_delay_ms: u64,
    /// Whether to track clipboard history
    pub history_enabled: bool,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            auto_copy_enabled: false,
            format: ClipboardFormat::PlainText,
            show_notification: false,
            preserve_clipboard: true,
            restore_delay_ms: 1000,
            history_enabled: true,
        }
    }
}

/// An entry in the clipboard history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistoryEntry {
    /// Unique identifier for the entry
    pub id: String,
    /// The text content
    pub text: String,
    /// Timestamp when the entry was created (ISO 8601)
    pub timestamp: String,
    /// Optional source description (e.g., "transcription", "manual copy")
    pub source: String,
}

/// Manages clipboard state and history
pub struct ClipboardManager {
    /// Current settings
    settings: ClipboardSettings,
    /// Clipboard history (most recent first)
    history: VecDeque<ClipboardHistoryEntry>,
    /// Preserved clipboard content for restore after paste
    preserved_content: Option<String>,
}

impl ClipboardManager {
    /// Create a new clipboard manager with default settings.
    pub fn new() -> Self {
        Self {
            settings: ClipboardSettings::default(),
            history: VecDeque::with_capacity(MAX_HISTORY_SIZE),
            preserved_content: None,
        }
    }

    /// Get current settings.
    pub fn settings(&self) -> &ClipboardSettings {
        &self.settings
    }

    /// Update settings.
    pub fn update_settings(&mut self, settings: ClipboardSettings) {
        debug!("Updating clipboard settings: {:?}", settings);
        self.settings = settings;

        // Clear history if disabled
        if !self.settings.history_enabled {
            self.history.clear();
        }
    }

    /// Add an entry to the clipboard history.
    pub fn add_to_history(&mut self, text: String, source: &str) {
        if !self.settings.history_enabled {
            return;
        }

        let entry = ClipboardHistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            text,
            timestamp: chrono::Utc::now().to_rfc3339(),
            source: source.to_string(),
        };

        debug!(
            "Adding to clipboard history: {} chars from {}",
            entry.text.len(),
            source
        );

        // Add to front (most recent first)
        self.history.push_front(entry);

        // Trim to max size
        while self.history.len() > MAX_HISTORY_SIZE {
            self.history.pop_back();
        }
    }

    /// Get clipboard history.
    pub fn get_history(&self) -> Vec<ClipboardHistoryEntry> {
        self.history.iter().cloned().collect()
    }

    /// Clear clipboard history.
    pub fn clear_history(&mut self) {
        debug!("Clearing clipboard history");
        self.history.clear();
    }

    /// Remove a specific entry from history.
    pub fn remove_from_history(&mut self, id: &str) -> bool {
        if let Some(pos) = self.history.iter().position(|e| e.id == id) {
            self.history.remove(pos);
            debug!("Removed entry {} from clipboard history", id);
            true
        } else {
            false
        }
    }

    /// Preserve the current clipboard content for later restoration.
    pub fn preserve_content(&mut self, content: String) {
        debug!("Preserving clipboard content: {} chars", content.len());
        self.preserved_content = Some(content);
    }

    /// Get and clear the preserved clipboard content.
    pub fn take_preserved_content(&mut self) -> Option<String> {
        self.preserved_content.take()
    }
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Global clipboard manager instance
static CLIPBOARD_MANAGER: OnceLock<Mutex<ClipboardManager>> = OnceLock::new();

fn get_manager() -> &'static Mutex<ClipboardManager> {
    CLIPBOARD_MANAGER.get_or_init(|| Mutex::new(ClipboardManager::new()))
}

/// Copy text to the system clipboard.
///
/// Copies the provided text to the clipboard and optionally adds it to
/// clipboard history.
#[tauri::command]
pub async fn copy_to_clipboard(
    app: AppHandle,
    text: String,
    source: Option<String>,
) -> Result<(), String> {
    if text.is_empty() {
        return Err("Cannot copy empty text to clipboard".to_string());
    }

    debug!("Copying {} chars to clipboard", text.len());

    app.clipboard().write_text(&text).map_err(|e| {
        error!("Failed to copy to clipboard: {}", e);
        format!("Failed to copy to clipboard: {}", e)
    })?;

    // Add to history
    let source = source.unwrap_or_else(|| "manual".to_string());
    let mut manager = get_manager().lock();
    manager.add_to_history(text, &source);

    info!("Text copied to clipboard from source: {}", source);
    Ok(())
}

/// Read text from the system clipboard.
///
/// Returns the current text content of the clipboard, or an empty string
/// if the clipboard is empty or does not contain text.
#[tauri::command]
pub async fn read_clipboard(app: AppHandle) -> Result<String, String> {
    debug!("Reading from clipboard");

    app.clipboard().read_text().map_err(|e| {
        error!("Failed to read clipboard: {}", e);
        format!("Failed to read clipboard: {}", e)
    })
}

/// Clear the system clipboard.
#[tauri::command]
pub async fn clear_clipboard(app: AppHandle) -> Result<(), String> {
    debug!("Clearing clipboard");

    app.clipboard().clear().map_err(|e| {
        error!("Failed to clear clipboard: {}", e);
        format!("Failed to clear clipboard: {}", e)
    })
}

/// Copy transcription to clipboard with auto-copy settings applied.
///
/// This is the main entry point for copying transcription results. It checks
/// the current settings and applies formatting as configured.
#[tauri::command]
pub async fn copy_transcription(
    app: AppHandle,
    text: String,
    enhanced: bool,
) -> Result<bool, String> {
    let manager = get_manager().lock();
    let settings = manager.settings().clone();
    drop(manager);

    if !settings.auto_copy_enabled {
        debug!("Auto-copy disabled, skipping clipboard copy");
        return Ok(false);
    }

    // Preserve current clipboard content if configured
    if settings.preserve_clipboard {
        if let Ok(current) = app.clipboard().read_text() {
            let mut manager = get_manager().lock();
            manager.preserve_content(current);
        }
    }

    // Format the text according to settings
    let formatted_text = match settings.format {
        ClipboardFormat::PlainText => text.clone(),
        ClipboardFormat::RichText => text.clone(), // Rich text handled by clipboard plugin
        ClipboardFormat::Markdown => {
            // Wrap in code block if it looks like it might benefit
            if text.contains('\n') {
                format!("```\n{}\n```", text)
            } else {
                text.clone()
            }
        }
    };

    // Copy to clipboard
    app.clipboard().write_text(&formatted_text).map_err(|e| {
        error!("Failed to copy transcription: {}", e);
        format!("Failed to copy transcription: {}", e)
    })?;

    // Add to history
    let source = if enhanced {
        "enhanced_transcription"
    } else {
        "transcription"
    };
    let mut manager = get_manager().lock();
    manager.add_to_history(text, source);

    info!(
        "Transcription copied to clipboard (enhanced: {}, format: {:?})",
        enhanced, settings.format
    );

    Ok(true)
}

/// Get current clipboard settings.
#[tauri::command]
pub fn get_clipboard_settings() -> ClipboardSettings {
    let manager = get_manager().lock();
    manager.settings().clone()
}

/// Update clipboard settings.
#[tauri::command]
pub fn set_clipboard_settings(settings: ClipboardSettings) -> Result<(), String> {
    let mut manager = get_manager().lock();
    manager.update_settings(settings);
    Ok(())
}

/// Get clipboard history.
#[tauri::command]
pub fn get_clipboard_history() -> Vec<ClipboardHistoryEntry> {
    let manager = get_manager().lock();
    manager.get_history()
}

/// Clear clipboard history.
#[tauri::command]
pub fn clear_clipboard_history() {
    let mut manager = get_manager().lock();
    manager.clear_history();
}

/// Remove a specific entry from clipboard history.
#[tauri::command]
pub fn remove_clipboard_history_entry(id: String) -> bool {
    let mut manager = get_manager().lock();
    manager.remove_from_history(&id)
}

/// Copy an entry from clipboard history to the clipboard.
#[tauri::command]
pub async fn copy_from_history(app: AppHandle, id: String) -> Result<(), String> {
    let manager = get_manager().lock();
    let entry = manager
        .get_history()
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| "History entry not found".to_string())?;
    drop(manager);

    app.clipboard().write_text(&entry.text).map_err(|e| {
        error!("Failed to copy from history: {}", e);
        format!("Failed to copy from history: {}", e)
    })?;

    info!("Copied history entry {} to clipboard", id);
    Ok(())
}

/// Restore the preserved clipboard content.
///
/// Call this after pasting to restore the user's original clipboard content.
#[tauri::command]
pub async fn restore_clipboard(app: AppHandle) -> Result<bool, String> {
    let mut manager = get_manager().lock();
    if let Some(content) = manager.take_preserved_content() {
        drop(manager);

        app.clipboard().write_text(&content).map_err(|e| {
            error!("Failed to restore clipboard: {}", e);
            format!("Failed to restore clipboard: {}", e)
        })?;

        debug!("Restored preserved clipboard content");
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Get the current restore delay setting in milliseconds.
#[tauri::command]
pub fn get_restore_delay() -> u64 {
    let manager = get_manager().lock();
    manager.settings().restore_delay_ms
}

/// Paste text at cursor with automatic clipboard restoration.
///
/// This command implements the complete paste-with-restore flow:
/// 1. Save current clipboard contents (if preserve_clipboard enabled)
/// 2. Copy transcription to clipboard
/// 3. Paste at cursor position
/// 4. Schedule clipboard restoration after configured delay
///
/// Returns the delay in milliseconds for the frontend to schedule restoration,
/// or 0 if restoration is not enabled.
#[tauri::command]
pub async fn paste_transcription(
    app: AppHandle,
    text: String,
    enhanced: bool,
) -> Result<u64, String> {
    if text.is_empty() {
        return Err("Cannot paste empty text".to_string());
    }

    let manager = get_manager().lock();
    let settings = manager.settings().clone();
    drop(manager);

    // Save current clipboard if preservation is enabled
    if settings.preserve_clipboard {
        if let Ok(current) = app.clipboard().read_text() {
            let mut manager = get_manager().lock();
            manager.preserve_content(current);
        }
    }

    // Format the text according to settings
    let formatted_text = match settings.format {
        ClipboardFormat::PlainText => text.clone(),
        ClipboardFormat::RichText => text.clone(),
        ClipboardFormat::Markdown => {
            if text.contains('\n') {
                format!("```\n{}\n```", text)
            } else {
                text.clone()
            }
        }
    };

    // Copy to clipboard
    app.clipboard().write_text(&formatted_text).map_err(|e| {
        error!("Failed to copy transcription for paste: {}", e);
        format!("Failed to copy transcription: {}", e)
    })?;

    // Add to history
    let source = if enhanced {
        "enhanced_transcription"
    } else {
        "transcription"
    };
    {
        let mut manager = get_manager().lock();
        manager.add_to_history(text, source);
    }

    // Perform paste using text_insert module
    crate::text_insert::insert_text_by_paste(formatted_text, Some(50)).map_err(|e| {
        error!("Failed to paste transcription: {}", e);
        format!("Failed to paste: {}", e)
    })?;

    info!(
        "Transcription pasted (enhanced: {}, preserve: {})",
        enhanced, settings.preserve_clipboard
    );

    // Return delay for frontend to schedule restoration
    if settings.preserve_clipboard {
        Ok(settings.restore_delay_ms)
    } else {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_manager_creation() {
        let manager = ClipboardManager::new();
        assert!(!manager.settings().auto_copy_enabled);
        assert!(manager.get_history().is_empty());
    }

    #[test]
    fn test_clipboard_settings_default() {
        let settings = ClipboardSettings::default();
        assert!(!settings.auto_copy_enabled);
        assert_eq!(settings.format, ClipboardFormat::PlainText);
        assert!(!settings.show_notification);
        assert!(settings.preserve_clipboard);
        assert_eq!(settings.restore_delay_ms, 1000);
        assert!(settings.history_enabled);
    }

    #[test]
    fn test_add_to_history() {
        let mut manager = ClipboardManager::new();
        manager.add_to_history("Test text".to_string(), "test");

        let history = manager.get_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text, "Test text");
        assert_eq!(history[0].source, "test");
    }

    #[test]
    fn test_history_max_size() {
        let mut manager = ClipboardManager::new();

        // Add more than MAX_HISTORY_SIZE entries
        for i in 0..MAX_HISTORY_SIZE + 10 {
            manager.add_to_history(format!("Text {}", i), "test");
        }

        let history = manager.get_history();
        assert_eq!(history.len(), MAX_HISTORY_SIZE);
        // Most recent should be first
        assert_eq!(history[0].text, format!("Text {}", MAX_HISTORY_SIZE + 9));
    }

    #[test]
    fn test_history_disabled() {
        let mut manager = ClipboardManager::new();
        let mut settings = manager.settings().clone();
        settings.history_enabled = false;
        manager.update_settings(settings);

        manager.add_to_history("Test text".to_string(), "test");

        let history = manager.get_history();
        assert!(history.is_empty());
    }

    #[test]
    fn test_remove_from_history() {
        let mut manager = ClipboardManager::new();
        manager.add_to_history("Test 1".to_string(), "test");
        manager.add_to_history("Test 2".to_string(), "test");

        let history = manager.get_history();
        let id = history[0].id.clone();

        assert!(manager.remove_from_history(&id));
        assert_eq!(manager.get_history().len(), 1);
        assert!(!manager.remove_from_history(&id)); // Already removed
    }

    #[test]
    fn test_clear_history() {
        let mut manager = ClipboardManager::new();
        manager.add_to_history("Test 1".to_string(), "test");
        manager.add_to_history("Test 2".to_string(), "test");

        manager.clear_history();
        assert!(manager.get_history().is_empty());
    }

    #[test]
    fn test_preserve_content() {
        let mut manager = ClipboardManager::new();

        manager.preserve_content("Original content".to_string());
        assert!(manager.preserved_content.is_some());

        let content = manager.take_preserved_content();
        assert_eq!(content, Some("Original content".to_string()));
        assert!(manager.preserved_content.is_none());
    }

    #[test]
    fn test_clipboard_format_serialisation() {
        let plain = ClipboardFormat::PlainText;
        let serialised = serde_json::to_string(&plain).unwrap();
        assert_eq!(serialised, r#""plain_text""#);

        let deserialised: ClipboardFormat = serde_json::from_str(&serialised).unwrap();
        assert_eq!(deserialised, ClipboardFormat::PlainText);
    }
}
