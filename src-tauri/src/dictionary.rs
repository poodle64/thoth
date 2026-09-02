//! Dictionary management for domain-specific vocabulary and corrections
//!
//! Provides persistent storage and CRUD operations for custom word replacements.
//! Dictionary entries are stored in JSON format at `~/.thoth/dictionary.json`.

use crate::error::Error;
use parking_lot::RwLock;
use regex::{Regex, RegexBuilder};
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
///
/// `THOTH_DATA_DIR` overrides the directory when set to a non-empty path, the
/// same override the database honours — so a test can point the dictionary at a
/// throwaway file instead of editing the user's real one.
fn get_dictionary_path() -> PathBuf {
    if let Ok(dir) = std::env::var("THOTH_DATA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("dictionary.json");
        }
    }

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
pub fn get_dictionary_entries() -> Result<Vec<DictionaryEntry>, Error> {
    let dictionary = get_dictionary().read();
    Ok(dictionary.entries.clone())
}

/// Add a new dictionary entry
#[tauri::command]
pub fn add_dictionary_entry(entry: DictionaryEntry) -> Result<(), Error> {
    // Validate entry
    if entry.from.trim().is_empty() {
        return Err("The 'from' field cannot be empty".to_string().into());
    }
    if entry.to.trim().is_empty() {
        return Err("The 'to' field cannot be empty".to_string().into());
    }

    let mut dictionary = get_dictionary().write();

    // Check for duplicates
    let from_lower = entry.from.to_lowercase();
    if dictionary
        .entries
        .iter()
        .any(|e| e.from.to_lowercase() == from_lower)
    {
        return Err(format!("An entry for '{}' already exists", entry.from).into());
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
pub fn update_dictionary_entry(index: usize, entry: DictionaryEntry) -> Result<(), Error> {
    let mut dictionary = get_dictionary().write();
    update_entry_locked(&mut dictionary, index, entry)
}

/// Remove a dictionary entry by index
#[tauri::command]
pub fn remove_dictionary_entry(index: usize) -> Result<(), Error> {
    let mut dictionary = get_dictionary().write();
    remove_entry_locked(&mut dictionary, index)
}

/// Update the single entry whose `from` text is `from_key`, returning the index
/// it landed on.
///
/// The `from` text is the dictionary's natural key — [`add_dictionary_entry`]
/// already refuses a second entry with the same one — so this addresses an entry
/// by its own content rather than by a position the caller had to count out.
/// The stored `from` is kept verbatim (matching is case-insensitive, so the
/// caller's casing must not silently rewrite the entry); to change the `from`
/// text itself, remove the entry and add the new one.
///
/// `case_sensitive` of `None` keeps the entry's existing flag rather than
/// resetting it, so a caller that only means to change `to` cannot flip it by
/// omission.
pub fn update_dictionary_entry_by_from(
    from_key: &str,
    to: String,
    case_sensitive: Option<bool>,
) -> Result<usize, Error> {
    let mut dictionary = get_dictionary().write();
    let index = resolve_unique_from(&dictionary.entries, from_key)?;
    let entry = DictionaryEntry {
        from: dictionary.entries[index].from.clone(),
        to,
        case_sensitive: case_sensitive.unwrap_or(dictionary.entries[index].case_sensitive),
    };
    update_entry_locked(&mut dictionary, index, entry)?;
    Ok(index)
}

/// Remove the single entry whose `from` text is `from_key`, returning the index
/// it was removed from.
///
/// The safe counterpart to [`remove_dictionary_entry`]: an index the caller
/// miscounted deletes a different, working entry silently, whereas a `from` that
/// does not resolve to exactly one entry is refused. Resolution happens under the
/// same write lock as the removal, so no concurrent edit can shift the index in
/// between.
pub fn remove_dictionary_entry_by_from(from_key: &str) -> Result<usize, Error> {
    let mut dictionary = get_dictionary().write();
    let index = resolve_unique_from(&dictionary.entries, from_key)?;
    remove_entry_locked(&mut dictionary, index)?;
    Ok(index)
}

/// Resolve the one entry whose `from` matches `key`, or refuse.
///
/// Matching is case-insensitive on the whole value, identical to the duplicate
/// check in [`add_dictionary_entry`] — one rule for "is this the same entry?".
/// Zero matches and more than one match are both errors: a caller asking for an
/// entry by name gets the entry it named or nothing, never a guess.
fn resolve_unique_from(entries: &[DictionaryEntry], key: &str) -> Result<usize, Error> {
    let needle = key.to_lowercase();
    let matches: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.from.to_lowercase() == needle)
        .map(|(i, _)| i)
        .collect();

    match matches.as_slice() {
        [only] => Ok(*only),
        [] => Err(format!("No dictionary entry has a 'from' of '{}'", key).into()),
        many => Err(format!(
            "'{}' matches {} entries (indices {}); refusing to guess — pass an explicit 'index' to pick one",
            key,
            many.len(),
            many.iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
        .into()),
    }
}

/// Update one entry, with the dictionary already locked for writing.
fn update_entry_locked(
    dictionary: &mut Dictionary,
    index: usize,
    entry: DictionaryEntry,
) -> Result<(), Error> {
    // Validate entry
    if entry.from.trim().is_empty() {
        return Err("The 'from' field cannot be empty".to_string().into());
    }
    if entry.to.trim().is_empty() {
        return Err("The 'to' field cannot be empty".to_string().into());
    }

    if index >= dictionary.entries.len() {
        return Err(format!("Invalid entry index: {}", index).into());
    }

    // Check for duplicates (excluding the current entry)
    let from_lower = entry.from.to_lowercase();
    if dictionary
        .entries
        .iter()
        .enumerate()
        .any(|(i, e)| i != index && e.from.to_lowercase() == from_lower)
    {
        return Err(format!("An entry for '{}' already exists", entry.from).into());
    }

    dictionary.entries[index] = entry;
    save_dictionary(dictionary)?;

    tracing::info!("Updated dictionary entry at index {}", index);
    Ok(())
}

/// Remove one entry, with the dictionary already locked for writing.
fn remove_entry_locked(dictionary: &mut Dictionary, index: usize) -> Result<(), Error> {
    if index >= dictionary.entries.len() {
        return Err(format!("Invalid entry index: {}", index).into());
    }

    let removed = dictionary.entries.remove(index);
    save_dictionary(dictionary)?;

    tracing::info!(
        "Removed dictionary entry '{}', remaining: {}",
        removed.from,
        dictionary.entries.len()
    );
    Ok(())
}

/// Import dictionary entries from JSON content
#[tauri::command]
pub fn import_dictionary(json_content: String, merge: bool) -> Result<usize, Error> {
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
pub fn export_dictionary() -> Result<String, Error> {
    let dictionary = get_dictionary().read();
    serde_json::to_string_pretty(&*dictionary)
        .map_err(|e| format!("Failed to serialise: {}", e))
        .map_err(Into::into)
}

/// Apply dictionary replacements to text
///
/// Matches whole words only: an entry `hook -> look` rewrites "hook" but leaves
/// "webhook" untouched. Multi-word entries (e.g. "machine learning") anchor at
/// the phrase boundaries. Entries that begin or end with a non-word character
/// (punctuation) simply don't anchor on that side, which is the closest sensible
/// behaviour for a `\b`-based boundary.
pub fn apply_dictionary(text: &str) -> String {
    let dictionary = get_dictionary().read();

    if dictionary.entries.is_empty() {
        return text.to_string();
    }

    let mut result = text.to_string();

    for entry in &dictionary.entries {
        result = replace_whole_word(&result, &entry.from, &entry.to, entry.case_sensitive);
    }

    result
}

/// Replace whole-word occurrences of `from` with `to`.
///
/// Anchors the (regex-escaped) needle between `\b` word boundaries so substrings
/// inside larger words are left alone. Falls back to plain substring replacement
/// only if the boundary pattern fails to compile (it never should for escaped
/// input, but we never want a malformed entry to drop the whole transcription).
fn replace_whole_word(text: &str, from: &str, to: &str, case_sensitive: bool) -> String {
    if from.is_empty() {
        return text.to_string();
    }

    let pattern = format!(r"\b{}\b", regex::escape(from));
    let regex = RegexBuilder::new(&pattern)
        .case_insensitive(!case_sensitive)
        .build();

    match regex {
        Ok(re) => whole_word_replace_all(&re, text, to),
        Err(e) => {
            tracing::warn!(
                "Dictionary entry '{}' failed to compile as regex ({e}); falling back to substring replace",
                from
            );
            if case_sensitive {
                text.replace(from, to)
            } else {
                text.to_string()
            }
        }
    }
}

/// Replace all regex matches with a literal replacement (no `$`-capture expansion).
fn whole_word_replace_all(re: &Regex, text: &str, to: &str) -> String {
    re.replace_all(text, regex::NoExpand(to)).into_owned()
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
pub fn get_vocabulary_for_context() -> Vec<String> {
    let dictionary = get_dictionary().read();

    dictionary.entries.iter().map(|e| e.to.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Whole-word replacement tests (#57)
    // =========================================================================

    #[test]
    fn test_whole_word_does_not_match_substring() {
        // The core #57 fix: an entry must not rewrite a substring inside a word.
        assert_eq!(
            replace_whole_word("the webhook fired", "hook", "look", false),
            "the webhook fired"
        );
        assert_eq!(
            replace_whole_word("classification", "class", "klass", false),
            "classification"
        );
    }

    #[test]
    fn test_whole_word_matches_standalone() {
        assert_eq!(
            replace_whole_word("grab the hook", "hook", "look", false),
            "grab the look"
        );
        // Adjacent punctuation still counts as a boundary.
        assert_eq!(
            replace_whole_word("the hook, please", "hook", "look", false),
            "the look, please"
        );
    }

    #[test]
    fn test_whole_word_case_insensitive() {
        assert_eq!(
            replace_whole_word("Hello HELLO hello", "hello", "hi", false),
            "hi hi hi"
        );
    }

    #[test]
    fn test_whole_word_case_sensitive() {
        // Only the exact-case standalone word is rewritten.
        assert_eq!(
            replace_whole_word("Hello hello HELLO", "hello", "hi", true),
            "Hello hi HELLO"
        );
    }

    #[test]
    fn test_whole_word_multi_word_entry() {
        assert_eq!(
            replace_whole_word(
                "I love machine learning a lot",
                "machine learning",
                "ML",
                false
            ),
            "I love ML a lot"
        );
    }

    #[test]
    fn test_whole_word_regex_metacharacters_are_literal() {
        // A user entry containing regex metacharacters must be escaped, not interpreted.
        // `.` is a metacharacter; without escaping it would match any character.
        assert_eq!(
            replace_whole_word("use node.js here", "node.js", "Node", false),
            "use Node here"
        );
        // It must NOT match a different character in the metacharacter slot.
        assert_eq!(
            replace_whole_word("nodexjs", "node.js", "Node", false),
            "nodexjs"
        );
    }

    #[test]
    fn test_whole_word_trailing_punctuation_does_not_anchor() {
        // Documented limitation: an entry ending in a non-word char (e.g. "c++")
        // has no word boundary after it, so `\b…\b` cannot match. This is
        // acceptable graceful degradation rather than silently falling back to
        // substring matching (which would reintroduce the #57 bug).
        assert_eq!(
            replace_whole_word("the c++ code", "c++", "cpp", false),
            "the c++ code"
        );
    }

    #[test]
    fn test_whole_word_replacement_is_literal_not_capture() {
        // A `to` value containing `$1` must be inserted verbatim, not expanded.
        assert_eq!(
            replace_whole_word("see foo here", "foo", "$1 bar", false),
            "see $1 bar here"
        );
    }

    #[test]
    fn test_whole_word_empty_from() {
        assert_eq!(
            replace_whole_word("Empty from", "", "replacement", false),
            "Empty from"
        );
    }

    #[test]
    fn test_whole_word_no_match() {
        assert_eq!(
            replace_whole_word("The quick brown fox", "cat", "dog", false),
            "The quick brown fox"
        );
    }

    #[test]
    fn test_whole_word_unicode() {
        assert_eq!(
            replace_whole_word("a café here", "café", "coffee", false),
            "a coffee here"
        );
    }

    // =========================================================================
    // Addressing an entry by its `from` text
    // =========================================================================

    fn entry(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry {
            from: from.to_string(),
            to: to.to_string(),
            case_sensitive: false,
        }
    }

    #[test]
    fn test_resolve_from_finds_the_named_entry() {
        let entries = vec![
            entry("teh", "the"),
            entry("immich", "Immich"),
            entry("ell", "LLM"),
        ];
        assert_eq!(resolve_unique_from(&entries, "immich").unwrap(), 1);
        // The first and last entries are addressable too — nothing is off-by-one.
        assert_eq!(resolve_unique_from(&entries, "teh").unwrap(), 0);
        assert_eq!(resolve_unique_from(&entries, "ell").unwrap(), 2);
    }

    #[test]
    fn test_resolve_from_is_case_insensitive() {
        // Matching mirrors add_dictionary_entry's duplicate check, so an entry
        // that could not be added twice can always be addressed by name.
        let entries = vec![entry("LiteLLM", "LiteLLM")];
        assert_eq!(resolve_unique_from(&entries, "litellm").unwrap(), 0);
    }

    #[test]
    fn test_resolve_from_matches_whole_value_not_substring() {
        // "hook" must not resolve to "webhook": a substring match would delete
        // exactly the kind of wrong entry this addressing exists to prevent.
        let entries = vec![entry("webhook", "web hook")];
        assert!(resolve_unique_from(&entries, "hook").is_err());
    }

    #[test]
    fn test_resolve_from_refuses_when_absent() {
        let entries = vec![entry("teh", "the")];
        let err = resolve_unique_from(&entries, "nope")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("No dictionary entry"),
            "unhelpful error: {err}"
        );
    }

    #[test]
    fn test_resolve_from_refuses_when_ambiguous() {
        // Duplicates can reach the file via a hand edit or a replace-import, so
        // ambiguity is real. The resolver must refuse, not pick the first.
        let entries = vec![
            entry("teh", "the"),
            entry("ell", "LLM"),
            entry("TEH", "the"),
        ];
        let err = resolve_unique_from(&entries, "teh")
            .unwrap_err()
            .to_string();
        assert!(err.contains("refusing to guess"), "unhelpful error: {err}");
        // The error names both candidates so the caller can pick one by index.
        assert!(
            err.contains('0') && err.contains('2'),
            "indices missing: {err}"
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

    // `#[serial]` because the path now consults `THOTH_DATA_DIR`, which the
    // trash tests in this same binary set and clear; without it this test can
    // observe their throwaway directory instead of the `~/.thoth` default.
    #[test]
    #[serial_test::serial]
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
        let text = "Hello hello HELLO";

        // Case-insensitive should rewrite all three whole words.
        assert_eq!(replace_whole_word(text, "hello", "hi", false), "hi hi hi");

        // Case-sensitive should only rewrite the exact-case word.
        assert_eq!(
            replace_whole_word(text, "hello", "hi", true),
            "Hello hi HELLO"
        );
    }
}
