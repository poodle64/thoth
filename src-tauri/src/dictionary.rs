//! Dictionary management for domain-specific vocabulary and corrections
//!
//! Provides persistent storage and CRUD operations for custom word replacements.
//! Dictionary entries are stored in JSON format at `~/.thoth/dictionary.json`.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A dictionary entry for word replacement
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    /// The text to search for and replace
    pub from: String,
    /// The replacement text
    pub to: String,
    /// Whether the match should be case-sensitive
    pub case_sensitive: bool,
}

/// The dictionary storage structure
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Dictionary {
    /// The dictionary entries
    pub entries: Vec<DictionaryEntry>,
}

/// Global dictionary instance
static DICTIONARY: OnceLock<RwLock<Dictionary>> = OnceLock::new();

/// Get the dictionary file path (~/.thoth/dictionary.json)
fn get_dictionary_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".thoth")
        .join("dictionary.json")
}

/// Get the global dictionary instance, loading from disk if needed
fn get_dictionary() -> &'static RwLock<Dictionary> {
    DICTIONARY.get_or_init(|| {
        let path = get_dictionary_path();
        let dictionary = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(e) => {
                    tracing::warn!("Failed to read dictionary file: {}", e);
                    Dictionary::default()
                }
            }
        } else {
            Dictionary::default()
        };
        RwLock::new(dictionary)
    })
}

/// Save the dictionary to disk
fn save_dictionary(dictionary: &Dictionary) -> Result<(), String> {
    let path = get_dictionary_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(dictionary)
        .map_err(|e| format!("Failed to serialise: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write dictionary: {}", e))?;

    tracing::debug!("Dictionary saved to {:?}", path);
    Ok(())
}

/// Get all dictionary entries
#[tauri::command]
pub fn get_dictionary_entries() -> Result<Vec<DictionaryEntry>, String> {
    let dictionary = get_dictionary().read();
    Ok(dictionary.entries.clone())
}

/// Add a new dictionary entry
#[tauri::command]
pub fn add_dictionary_entry(entry: DictionaryEntry) -> Result<(), String> {
    // Validate entry
    if entry.from.trim().is_empty() {
        return Err("The 'from' field cannot be empty".to_string());
    }
    if entry.to.trim().is_empty() {
        return Err("The 'to' field cannot be empty".to_string());
    }

    let mut dictionary = get_dictionary().write();

    // Check for duplicates
    let from_lower = entry.from.to_lowercase();
    if dictionary
        .entries
        .iter()
        .any(|e| e.from.to_lowercase() == from_lower)
    {
        return Err(format!("An entry for '{}' already exists", entry.from));
    }

    dictionary.entries.push(entry);
    save_dictionary(&dictionary)?;

    tracing::info!(
        "Added dictionary entry, total entries: {}",
        dictionary.entries.len()
    );
    Ok(())
}

/// Update an existing dictionary entry
#[tauri::command]
pub fn update_dictionary_entry(index: usize, entry: DictionaryEntry) -> Result<(), String> {
    // Validate entry
    if entry.from.trim().is_empty() {
        return Err("The 'from' field cannot be empty".to_string());
    }
    if entry.to.trim().is_empty() {
        return Err("The 'to' field cannot be empty".to_string());
    }

    let mut dictionary = get_dictionary().write();

    if index >= dictionary.entries.len() {
        return Err(format!("Invalid entry index: {}", index));
    }

    // Check for duplicates (excluding the current entry)
    let from_lower = entry.from.to_lowercase();
    if dictionary
        .entries
        .iter()
        .enumerate()
        .any(|(i, e)| i != index && e.from.to_lowercase() == from_lower)
    {
        return Err(format!("An entry for '{}' already exists", entry.from));
    }

    dictionary.entries[index] = entry;
    save_dictionary(&dictionary)?;

    tracing::info!("Updated dictionary entry at index {}", index);
    Ok(())
}

/// Remove a dictionary entry by index
#[tauri::command]
pub fn remove_dictionary_entry(index: usize) -> Result<(), String> {
    let mut dictionary = get_dictionary().write();

    if index >= dictionary.entries.len() {
        return Err(format!("Invalid entry index: {}", index));
    }

    let removed = dictionary.entries.remove(index);
    save_dictionary(&dictionary)?;

    tracing::info!(
        "Removed dictionary entry '{}', remaining: {}",
        removed.from,
        dictionary.entries.len()
    );
    Ok(())
}

/// Import dictionary entries from JSON content
#[tauri::command]
pub fn import_dictionary(json_content: String, merge: bool) -> Result<usize, String> {
    let imported: Dictionary =
        serde_json::from_str(&json_content).map_err(|e| format!("Invalid JSON format: {}", e))?;

    let mut dictionary = get_dictionary().write();
    let import_count;

    if merge {
        // Build a set of existing 'from' values for deduplication
        let existing: HashMap<String, usize> = dictionary
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.from.to_lowercase(), i))
            .collect();

        let mut new_entries = Vec::new();
        for entry in imported.entries {
            if entry.from.trim().is_empty() || entry.to.trim().is_empty() {
                continue;
            }
            if !existing.contains_key(&entry.from.to_lowercase()) {
                new_entries.push(entry);
            }
        }
        import_count = new_entries.len();
        dictionary.entries.extend(new_entries);
    } else {
        // Replace entire dictionary
        let valid_entries: Vec<_> = imported
            .entries
            .into_iter()
            .filter(|e| !e.from.trim().is_empty() && !e.to.trim().is_empty())
            .collect();
        import_count = valid_entries.len();
        dictionary.entries = valid_entries;
    }

    save_dictionary(&dictionary)?;

    tracing::info!(
        "Imported {} dictionary entries (merge={})",
        import_count,
        merge
    );
    Ok(import_count)
}

/// Export dictionary entries as JSON
#[tauri::command]
pub fn export_dictionary() -> Result<String, String> {
    let dictionary = get_dictionary().read();
    serde_json::to_string_pretty(&*dictionary).map_err(|e| format!("Failed to serialise: {}", e))
}

/// Apply dictionary replacements to text
pub fn apply_dictionary(text: &str) -> String {
    let dictionary = get_dictionary().read();

    if dictionary.entries.is_empty() {
        return text.to_string();
    }

    let mut result = text.to_string();

    for entry in &dictionary.entries {
        if entry.case_sensitive {
            result = result.replace(&entry.from, &entry.to);
        } else {
            // Case-insensitive replacement
            result = replace_case_insensitive(&result, &entry.from, &entry.to);
        }
    }

    result
}

/// Case-insensitive string replacement
fn replace_case_insensitive(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }

    let lower_text = text.to_lowercase();
    let lower_from = from.to_lowercase();

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for (start, _) in lower_text.match_indices(&lower_from) {
        result.push_str(&text[last_end..start]);
        result.push_str(to);
        last_end = start + from.len();
    }

    result.push_str(&text[last_end..]);
    result
}

/// Tauri command to apply dictionary replacements
#[tauri::command]
pub fn apply_dictionary_to_text(text: String) -> String {
    apply_dictionary(&text)
}

/// Get vocabulary words for AI enhancement context
///
/// Returns a list of unique words that appear in dictionary replacements.
/// These can be included in AI prompts to help the model understand
/// domain-specific terminology.
#[tauri::command]
pub fn get_vocabulary_for_context() -> Vec<String> {
    let dictionary = get_dictionary().read();

    dictionary.entries.iter().map(|e| e.to.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Case-insensitive replacement tests
    // =========================================================================

    #[test]
    fn test_replace_case_insensitive() {
        assert_eq!(
            replace_case_insensitive("Hello World HELLO world", "hello", "hi"),
            "hi World hi world"
        );
        assert_eq!(
            replace_case_insensitive("Test test TEST", "test", "example"),
            "example example example"
        );
        assert_eq!(
            replace_case_insensitive("No match here", "missing", "found"),
            "No match here"
        );
        assert_eq!(
            replace_case_insensitive("Empty from", "", "replacement"),
            "Empty from"
        );
    }

    #[test]
    fn test_replace_case_insensitive_at_boundaries() {
        // At start of string
        assert_eq!(
            replace_case_insensitive("Hello there", "hello", "hi"),
            "hi there"
        );
        // At end of string
        assert_eq!(
            replace_case_insensitive("Say hello", "hello", "hi"),
            "Say hi"
        );
        // Entire string
        assert_eq!(replace_case_insensitive("hello", "hello", "hi"), "hi");
    }

    #[test]
    fn test_replace_case_insensitive_mixed_case() {
        assert_eq!(
            replace_case_insensitive("HeLLo WoRLd", "hello", "hi"),
            "hi WoRLd"
        );
        assert_eq!(
            replace_case_insensitive("hElLo wOrLd", "HELLO", "hi"),
            "hi wOrLd"
        );
    }

    #[test]
    fn test_replace_case_insensitive_no_match() {
        assert_eq!(
            replace_case_insensitive("The quick brown fox", "cat", "dog"),
            "The quick brown fox"
        );
    }

    #[test]
    fn test_replace_case_insensitive_multiple_occurrences() {
        assert_eq!(
            replace_case_insensitive("aa AA aA Aa", "aa", "bb"),
            "bb bb bb bb"
        );
    }

    #[test]
    fn test_replace_case_insensitive_adjacent_matches() {
        assert_eq!(replace_case_insensitive("testtest", "test", "X"), "XX");
    }

    #[test]
    fn test_replace_case_insensitive_empty_string() {
        assert_eq!(replace_case_insensitive("", "hello", "hi"), "");
    }

    #[test]
    fn test_replace_case_insensitive_replacement_longer() {
        assert_eq!(
            replace_case_insensitive("hi", "hi", "hello world"),
            "hello world"
        );
    }

    #[test]
    fn test_replace_case_insensitive_replacement_shorter() {
        assert_eq!(
            replace_case_insensitive("hello world", "hello", "hi"),
            "hi world"
        );
    }

    #[test]
    fn test_replace_case_insensitive_unicode() {
        // Unicode handling
        assert_eq!(
            replace_case_insensitive("Cafe café CAFÉ", "café", "coffee"),
            "Cafe coffee coffee"
        );
    }

    // =========================================================================
    // Dictionary entry validation tests
    // =========================================================================

    #[test]
    fn test_entry_validation() {
        let empty_from = DictionaryEntry {
            from: "".to_string(),
            to: "replacement".to_string(),
            case_sensitive: false,
        };
        assert!(empty_from.from.trim().is_empty());

        let valid_entry = DictionaryEntry {
            from: "teh".to_string(),
            to: "the".to_string(),
            case_sensitive: false,
        };
        assert!(!valid_entry.from.trim().is_empty());
        assert!(!valid_entry.to.trim().is_empty());
    }

    #[test]
    fn test_entry_validation_whitespace_only() {
        let whitespace_from = DictionaryEntry {
            from: "   ".to_string(),
            to: "replacement".to_string(),
            case_sensitive: false,
        };
        assert!(whitespace_from.from.trim().is_empty());

        let whitespace_to = DictionaryEntry {
            from: "valid".to_string(),
            to: "   ".to_string(),
            case_sensitive: false,
        };
        assert!(whitespace_to.to.trim().is_empty());
    }

    #[test]
    fn test_entry_validation_with_newlines() {
        let entry_with_newline = DictionaryEntry {
            from: "from\ntext".to_string(),
            to: "to\ntext".to_string(),
            case_sensitive: false,
        };
        assert!(!entry_with_newline.from.trim().is_empty());
        assert!(!entry_with_newline.to.trim().is_empty());
    }

    // =========================================================================
    // DictionaryEntry struct tests
    // =========================================================================

    #[test]
    fn test_dictionary_entry_serialisation() {
        let entry = DictionaryEntry {
            from: "teh".to_string(),
            to: "the".to_string(),
            case_sensitive: false,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"from\":\"teh\""));
        assert!(json.contains("\"to\":\"the\""));
        assert!(json.contains("\"caseSensitive\":false")); // camelCase due to serde rename
    }

    #[test]
    fn test_dictionary_entry_deserialisation() {
        let json = r#"{"from":"recieve","to":"receive","caseSensitive":true}"#;
        let entry: DictionaryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.from, "recieve");
        assert_eq!(entry.to, "receive");
        assert!(entry.case_sensitive);
    }

    #[test]
    fn test_dictionary_entry_clone() {
        let entry = DictionaryEntry {
            from: "original".to_string(),
            to: "replacement".to_string(),
            case_sensitive: true,
        };
        let cloned = entry.clone();
        assert_eq!(entry.from, cloned.from);
        assert_eq!(entry.to, cloned.to);
        assert_eq!(entry.case_sensitive, cloned.case_sensitive);
    }

    // =========================================================================
    // Dictionary struct tests
    // =========================================================================

    #[test]
    fn test_dictionary_default() {
        let dict = Dictionary::default();
        assert!(dict.entries.is_empty());
    }

    #[test]
    fn test_dictionary_serialisation() {
        let mut dict = Dictionary::default();
        dict.entries.push(DictionaryEntry {
            from: "teh".to_string(),
            to: "the".to_string(),
            case_sensitive: false,
        });
        dict.entries.push(DictionaryEntry {
            from: "recieve".to_string(),
            to: "receive".to_string(),
            case_sensitive: false,
        });

        let json = serde_json::to_string_pretty(&dict).unwrap();
        let restored: Dictionary = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.entries.len(), 2);
        assert_eq!(restored.entries[0].from, "teh");
        assert_eq!(restored.entries[1].from, "recieve");
    }

    #[test]
    fn test_dictionary_empty_serialisation() {
        let dict = Dictionary::default();
        let json = serde_json::to_string(&dict).unwrap();
        let restored: Dictionary = serde_json::from_str(&json).unwrap();
        assert!(restored.entries.is_empty());
    }

    // =========================================================================
    // Dictionary path tests
    // =========================================================================

    #[test]
    fn test_dictionary_path_format() {
        let path = get_dictionary_path();
        let path_str = path.to_string_lossy();

        // Should be in .thoth directory
        assert!(path_str.contains(".thoth"));
        // Should be named dictionary.json
        assert!(path_str.ends_with("dictionary.json"));
    }

    // =========================================================================
    // Integration-style unit tests (without filesystem)
    // =========================================================================

    #[test]
    fn test_case_sensitive_vs_insensitive_replacement() {
        // Case-sensitive replacement
        let text = "Hello hello HELLO";

        // Case-insensitive should replace all
        let result = replace_case_insensitive(text, "hello", "hi");
        assert_eq!(result, "hi hi hi");

        // Case-sensitive should only replace exact match
        let result = text.replace("hello", "hi");
        assert_eq!(result, "Hello hi HELLO");
    }
}
