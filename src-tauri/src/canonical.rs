//! Canonical-term registry — deterministic phonetic/fuzzy snapping.
//!
//! Stores a small registry of "canonical" terms (e.g. "portcullis", "LiteLLM").
//! When a transcription contains a phonetically or orthographically similar
//! sequence of words, it is snapped to the registered canonical form.
//!
//! This is a layer above the flat dictionary:
//! - The dictionary handles exact / whole-word find-replace.
//! - This module handles acoustic/spelling variant clustering so you register
//!   a term ONCE and all variants resolve to it automatically.
//!
//! Storage: `~/.thoth/canonical_terms.json` (pretty JSON, camelCase).
//! On first run the file is absent; the module seeds from the dictionary so
//! existing behaviour is preserved exactly (AliasOnly policy = same as today).

use crate::error::Error;
use parking_lot::RwLock;
use rphonetic::{DoubleMetaphone, Encoder};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// How aggressively unknown variants are snapped to a canonical term.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SnapPolicy {
    /// Snap only when the candidate exactly matches the term or an explicit alias
    /// (case-insensitive).  Behaviour-preserving default — identical to the old
    /// flat dictionary.
    #[default]
    AliasOnly,
    /// Snap when BOTH the Double-Metaphone keys match AND the normalised
    /// Damerau–Levenshtein distance >= threshold (default 0.55).  The AND
    /// gate prevents phonetic collisions on short 4-char codes (e.g. FLTR
    /// matches both "folder" and "Vaultwarden") from firing without also
    /// requiring meaningful string similarity.
    Phonetic,
    /// Same AND gate as Phonetic (Double-Metaphone key match AND normalised
    /// Damerau–Levenshtein distance >= threshold), but with a higher default
    /// threshold (0.85) for terms that share a phonetic code with common words.
    Conservative,
}

/// A canonical term with its variants and matching policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalTerm {
    /// The canonical spelling to snap to.
    pub term: String,
    /// Explicit spelling variants (matched exactly, case-insensitive).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Matching policy (default: AliasOnly).
    #[serde(default)]
    pub policy: SnapPolicy,
    /// Maximum number of consecutive tokens to consider as one candidate
    /// (window size for the n-gram sweep).  Default: 3.
    #[serde(default = "default_max_words")]
    pub max_words: u8,
    /// Per-term override for the edit-distance threshold.  When absent, the
    /// policy defaults apply (0.55 for Phonetic, 0.85 for Conservative).
    #[serde(default)]
    pub threshold: Option<f64>,
}

fn default_max_words() -> u8 {
    3
}

/// The on-disk registry.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalRegistry {
    pub terms: Vec<CanonicalTerm>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

static REGISTRY: OnceLock<RwLock<CanonicalRegistry>> = OnceLock::new();

fn get_registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".thoth")
        .join("canonical_terms.json")
}

fn load_or_seed() -> CanonicalRegistry {
    let path = get_registry_path();
    if path.exists() {
        match fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(r) => return r,
                Err(e) => tracing::warn!("Failed to parse canonical_terms.json: {}", e),
            },
            Err(e) => tracing::warn!("Failed to read canonical_terms.json: {}", e),
        }
    }

    // Seed from dictionary: group by `to` (case-insensitive), emit AliasOnly terms.
    let dict_entries = crate::dictionary::get_dictionary_entries().unwrap_or_default();
    if dict_entries.is_empty() {
        return CanonicalRegistry::default();
    }

    // Collect: canonical (lowercased key) -> (canonical display value, aliases)
    let mut map: std::collections::HashMap<String, (String, Vec<String>)> =
        std::collections::HashMap::new();

    for entry in &dict_entries {
        let key = entry.to.to_lowercase();
        let slot = map
            .entry(key)
            .or_insert_with(|| (entry.to.clone(), Vec::new()));
        let from_lc = entry.from.to_lowercase();
        // Avoid adding the term itself as an alias.
        if from_lc != slot.0.to_lowercase() && !slot.1.iter().any(|a| a.to_lowercase() == from_lc) {
            slot.1.push(entry.from.clone());
        }
    }

    let mut terms: Vec<CanonicalTerm> = map
        .into_values()
        .map(|(term, aliases)| CanonicalTerm {
            term,
            aliases,
            policy: SnapPolicy::AliasOnly,
            max_words: default_max_words(),
            threshold: None,
        })
        .collect();

    // Stable order for deterministic serialisation.
    terms.sort_by_key(|t| t.term.to_lowercase());

    let registry = CanonicalRegistry { terms };
    // Persist so subsequent runs don't reseed every time.
    if let Err(e) = save_registry(&registry) {
        tracing::warn!("Failed to persist seeded canonical registry: {}", e);
    }
    registry
}

fn get_registry() -> &'static RwLock<CanonicalRegistry> {
    REGISTRY.get_or_init(|| RwLock::new(load_or_seed()))
}

fn save_registry(registry: &CanonicalRegistry) -> Result<(), String> {
    let path = get_registry_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    let content = serde_json::to_string_pretty(registry)
        .map_err(|e| format!("Failed to serialise: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("Failed to write canonical terms: {}", e))?;
    tracing::debug!("Canonical registry saved to {:?}", path);
    Ok(())
}

// ---------------------------------------------------------------------------
// Phonetic helpers
// ---------------------------------------------------------------------------

static DM: OnceLock<DoubleMetaphone> = OnceLock::new();

fn dm() -> &'static DoubleMetaphone {
    DM.get_or_init(DoubleMetaphone::default)
}

/// True when the two strings share at least one Double-Metaphone key.
fn phonetic_match(a: &str, b: &str) -> bool {
    dm().is_encoded_equals(a, b)
}

// ---------------------------------------------------------------------------
// Tokeniser
// ---------------------------------------------------------------------------

/// A span in the original string: either an alphanumeric run (a word) or a
/// gap (whitespace / punctuation preserved verbatim).
#[derive(Debug)]
enum Span<'a> {
    Word(&'a str),
    Gap(&'a str),
}

/// Split `text` into alternating Word/Gap spans (starts with a gap if text
/// begins with non-alnum; each contiguous alnum run is one Word).
fn tokenise(text: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i].is_ascii_alphanumeric() {
            // Find end of alnum run (may include Unicode letters via char check below)
            let start = i;
            while i < len
                && text[i..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric())
            {
                i += text[i..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::Word(&text[start..i]));
        } else {
            let start = i;
            while i < len
                && !text[i..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric())
            {
                i += text[i..].chars().next().map_or(1, |c| c.len_utf8());
            }
            spans.push(Span::Gap(&text[start..i]));
        }
    }
    spans
}

/// Collect the indices of `Span::Word` entries within `spans`.
fn word_indices(spans: &[Span<'_>]) -> Vec<usize> {
    spans
        .iter()
        .enumerate()
        .filter_map(|(i, s)| matches!(s, Span::Word(_)).then_some(i))
        .collect()
}

// ---------------------------------------------------------------------------
// Casing
// ---------------------------------------------------------------------------

/// Restore the leading-capitalisation style of `original` onto `replacement`.
///
/// - ALL-CAPS original → replacement uppercased
/// - Capitalised original → replacement capitalised
/// - all-lower original → replacement unchanged
fn restore_case(original: &str, replacement: &str) -> String {
    let all_upper =
        original.chars().all(|c| !c.is_lowercase()) && original.chars().any(|c| c.is_uppercase());
    if all_upper {
        return replacement.to_uppercase();
    }
    if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut out = replacement.to_string();
        if let Some(first) = out.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        return out;
    }
    replacement.to_string()
}

/// Decide the output form of `canonical_term` when it matched `matched_span`.
///
/// If the term itself carries mixed or upper casing (e.g. "LiteLLM",
/// "Vaultwarden") it is inserted verbatim — the author of the term chose that
/// form.  Otherwise the matched span's leading-capitalisation style is restored.
fn output_form(canonical_term: &str, matched_span: &str) -> String {
    let term_is_cased = canonical_term != canonical_term.to_lowercase();
    if term_is_cased {
        canonical_term.to_string()
    } else {
        restore_case(matched_span, canonical_term)
    }
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// True when `candidate` should snap to `ct`.
///
/// For multi-word candidates, fuzzy policies compare the candidate against
/// **aliases of the same word count** (e.g. "port colours" ~ alias "port cullis")
/// rather than against the single-word term directly.  This prevents stop-word
/// inflation: DoubleMetaphone encodes "portcolours is" the same as "portcolours",
/// so phrase-level fuzzy must anchor on same-shape comparators.
fn matches_term(candidate: &str, ct: &CanonicalTerm) -> bool {
    let cand_lc = candidate.to_lowercase();
    let term_lc = ct.term.to_lowercase();

    // Exact short-circuit: applies for ALL policies.
    if cand_lc == term_lc || ct.aliases.iter().any(|a| a.to_lowercase() == cand_lc) {
        return true;
    }

    match ct.policy {
        SnapPolicy::AliasOnly => false,
        SnapPolicy::Phonetic => {
            let threshold = ct.threshold.unwrap_or(0.55);
            fuzzy_matches_any(&cand_lc, &term_lc, &ct.aliases, threshold)
        }
        SnapPolicy::Conservative => {
            let threshold = ct.threshold.unwrap_or(0.85);
            fuzzy_matches_any(&cand_lc, &term_lc, &ct.aliases, threshold)
        }
    }
}

/// Check whether `cand_lc` fuzzy-matches `term_lc` or any same-word-count alias.
///
/// Single-word candidates: AND gate for both Phonetic and Conservative —
/// phonetic key match AND ndl >= threshold must both hold.  The Phonetic
/// default threshold (0.55) is lower than Conservative (0.85) to catch
/// genuine acoustic variants while still blocking collisions on coarse
/// 4-char Double-Metaphone codes (e.g. FLTR matches both "folder" and
/// "Vaultwarden"; NDL 0.27 blocks the false positive).
///
/// Multi-word candidates: DoubleMetaphone drops stop words so phrase-level
/// phonetic keys are too coarse for an OR gate (e.g. "portcolours is" encodes
/// identically to "portcolours").  Multi-word matching therefore always uses
/// AND gate with a floor of `max(threshold, 0.60)` — the floor distinguishes
/// genuine acoustic variants ("port colours" / "port cullis" NDL ≈ 0.67) from
/// incidental phonetic collisions ("portcolours is" / "port cullis" NDL ≈ 0.50),
/// and the caller's threshold governs when it is above the floor (so Conservative
/// 0.85 is honoured on phrases as well as single-word candidates).
fn fuzzy_matches_any(cand_lc: &str, term_lc: &str, aliases: &[String], threshold: f64) -> bool {
    let cand_word_count = cand_lc.split_whitespace().count();
    let multi_word = cand_word_count > 1;

    // Collect same-word-count references (term or aliases).
    let mut refs: Vec<String> = Vec::new();
    if term_lc.split_whitespace().count() == cand_word_count {
        refs.push(term_lc.to_string());
    }
    for alias in aliases {
        let alias_lc = alias.to_lowercase();
        if alias_lc.split_whitespace().count() == cand_word_count {
            refs.push(alias_lc);
        }
    }

    if refs.is_empty() {
        return false;
    }

    refs.iter().any(|reference| {
        let phon = phonetic_match(cand_lc, reference.as_str());
        let ndl = strsim::normalized_damerau_levenshtein(cand_lc, reference.as_str());
        if multi_word {
            // AND gate for multi-word: phonetic key match anchors the comparison,
            // NDL >= max(threshold, 0.60) screens out stop-word collisions.
            // The 0.60 floor applies when the caller's threshold is lower; when
            // the caller passes a higher threshold (e.g. Conservative 0.85) it
            // still governs (so Conservative is stricter even on phrases).
            phon && ndl >= threshold.max(0.60_f64)
        } else {
            // Both Phonetic and Conservative use AND gate for single-word candidates.
            // Phonetic uses a lower default threshold (0.55) to allow genuine acoustic
            // variants through while blocking coarse phonetic-code collisions.
            phon && ndl >= threshold
        }
    })
}

/// How many tokens (words) does `term` contain?
fn term_word_count(term: &str) -> usize {
    term.split_whitespace().count().max(1)
}

/// True when a window of `window_size` tokens is a valid candidate for `ct`.
///
/// A window is compatible when:
/// 1. The canonical term itself has exactly `window_size` words (exact/fuzzy 1:1).
/// 2. Any registered alias has exactly `window_size` words — covers explicit
///    multi-word aliases ("port cullis") and fuzzy variants of same-shape aliases
///    ("port colours" ~ "port cullis").
fn window_compatible(ct: &CanonicalTerm, window_size: usize) -> bool {
    // Rule 1: same word count as the canonical term.
    if term_word_count(&ct.term) == window_size {
        return true;
    }
    // Rule 2: an explicit alias has this word count.
    if ct.aliases.iter().any(|a| term_word_count(a) == window_size) {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// Main entry-point
// ---------------------------------------------------------------------------

/// Apply canonical-term snapping to `text`.
///
/// Scans the text with a sliding n-gram window (largest window first) and
/// replaces matching spans with the canonical form.
pub fn apply_canonical(text: &str) -> String {
    let registry = get_registry();
    let guard = registry.read();

    if guard.terms.is_empty() {
        return text.to_string();
    }

    let spans = tokenise(text);
    let word_idx = word_indices(&spans);
    let n_words = word_idx.len();

    if n_words == 0 {
        return text.to_string();
    }

    // Maximum window size across all terms (capped at the largest registered max_words).
    let global_max_window = guard
        .terms
        .iter()
        .map(|t| t.max_words as usize)
        .max()
        .unwrap_or(1);

    // `consumed[i]` = true when the i-th word has already been snapped.
    let mut consumed = vec![false; n_words];
    // Output: reuse span content or replace with snap.
    let mut replacements: Vec<Option<String>> = vec![None; spans.len()];

    'outer: for wi in 0..n_words {
        if consumed[wi] {
            continue;
        }

        // Try windows from largest to smallest (longest match wins).
        let max_window = global_max_window.min(n_words - wi);

        for window_size in (1..=max_window).rev() {
            // Check that none of these words have been consumed already.
            if (wi..wi + window_size).any(|w| consumed[w]) {
                continue;
            }

            // Build the candidate string from words[wi .. wi+window_size],
            // joined by single spaces (normalised for matching).
            let candidate_words: Vec<&str> = (wi..wi + window_size)
                .map(|w| match &spans[word_idx[w]] {
                    Span::Word(s) => *s,
                    Span::Gap(_) => "",
                })
                .collect();
            let candidate = candidate_words.join(" ");
            let cand_lc = candidate.to_lowercase();

            // Length guard: skip very short candidates.
            if cand_lc.len() < 4 {
                continue;
            }

            // Find the first matching term whose word-count is compatible with this window.
            let snap = guard
                .terms
                .iter()
                .find(|ct| window_compatible(ct, window_size) && matches_term(&candidate, ct));

            if let Some(ct) = snap {
                // The candidate matched this term.  Build the matched-span string
                // (original chars, preserving capitalisation of the first word).
                let first_word = candidate_words[0];
                let out = output_form(&ct.term, first_word);

                // Mark the span of the first word in `spans` for replacement.
                replacements[word_idx[wi]] = Some(out);

                // Mark all words in the window as consumed; for words 2..N, mark their
                // span AND the preceding gap span for deletion (set to empty string).
                for w in wi..wi + window_size {
                    consumed[w] = true;
                    if w > wi {
                        // Suppress the gap between word wi and word w, and the word itself.
                        let gap_span_idx = word_idx[w] - 1;
                        replacements[gap_span_idx] = Some(String::new());
                        replacements[word_idx[w]] = Some(String::new());
                    }
                }

                continue 'outer;
            }
        }
    }

    // Reconstruct the output string.
    let mut out = String::with_capacity(text.len());
    for (i, span) in spans.iter().enumerate() {
        match &replacements[i] {
            Some(s) => out.push_str(s),
            None => match span {
                Span::Word(s) | Span::Gap(s) => out.push_str(s),
            },
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Return all registered canonical terms.
#[tauri::command]
pub fn get_canonical_terms() -> Result<Vec<CanonicalTerm>, Error> {
    Ok(get_registry().read().terms.clone())
}

/// Add a new canonical term.
#[tauri::command]
pub fn add_canonical_term(term: CanonicalTerm) -> Result<(), Error> {
    if term.term.trim().is_empty() {
        return Err("Term cannot be empty".to_string().into());
    }
    let mut registry = get_registry().write();
    let term_lc = term.term.to_lowercase();
    if registry
        .terms
        .iter()
        .any(|t| t.term.to_lowercase() == term_lc)
    {
        return Err(format!("A canonical term for '{}' already exists", term.term).into());
    }
    registry.terms.push(term);
    save_registry(&registry).map_err(Into::into)
}

/// Update an existing canonical term by index.
#[tauri::command]
pub fn update_canonical_term(index: usize, term: CanonicalTerm) -> Result<(), Error> {
    if term.term.trim().is_empty() {
        return Err("Term cannot be empty".to_string().into());
    }
    let mut registry = get_registry().write();
    if index >= registry.terms.len() {
        return Err(format!("Invalid index: {}", index).into());
    }
    let term_lc = term.term.to_lowercase();
    if registry
        .terms
        .iter()
        .enumerate()
        .any(|(i, t)| i != index && t.term.to_lowercase() == term_lc)
    {
        return Err(format!("A canonical term for '{}' already exists", term.term).into());
    }
    registry.terms[index] = term;
    save_registry(&registry).map_err(Into::into)
}

/// Remove a canonical term by index.
#[tauri::command]
pub fn remove_canonical_term(index: usize) -> Result<(), Error> {
    let mut registry = get_registry().write();
    if index >= registry.terms.len() {
        return Err(format!("Invalid index: {}", index).into());
    }
    registry.terms.remove(index);
    save_registry(&registry).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Helpers to build an in-memory registry without touching the filesystem
    // -------------------------------------------------------------------------

    fn run_with_registry(terms: Vec<CanonicalTerm>, text: &str) -> String {
        // Build spans and run the matching logic directly, bypassing global state.
        let spans = tokenise(text);
        let word_idx = word_indices(&spans);
        let n_words = word_idx.len();

        if n_words == 0 || terms.is_empty() {
            return text.to_string();
        }

        let global_max_window = terms
            .iter()
            .map(|t| t.max_words as usize)
            .max()
            .unwrap_or(1);
        let mut consumed = vec![false; n_words];
        let mut replacements: Vec<Option<String>> = vec![None; spans.len()];

        'outer: for wi in 0..n_words {
            if consumed[wi] {
                continue;
            }
            let max_window = global_max_window.min(n_words - wi);
            for window_size in (1..=max_window).rev() {
                if (wi..wi + window_size).any(|w| consumed[w]) {
                    continue;
                }
                let candidate_words: Vec<&str> = (wi..wi + window_size)
                    .map(|w| match &spans[word_idx[w]] {
                        Span::Word(s) => *s,
                        Span::Gap(_) => "",
                    })
                    .collect();
                let candidate = candidate_words.join(" ");
                if candidate.to_lowercase().len() < 4 {
                    continue;
                }
                let snap = terms
                    .iter()
                    .find(|ct| window_compatible(ct, window_size) && matches_term(&candidate, ct));
                if let Some(ct) = snap {
                    let first_word = candidate_words[0];
                    let out = output_form(&ct.term, first_word);
                    replacements[word_idx[wi]] = Some(out);
                    for w in wi..wi + window_size {
                        consumed[w] = true;
                        if w > wi {
                            let gap_span_idx = word_idx[w] - 1;
                            replacements[gap_span_idx] = Some(String::new());
                            replacements[word_idx[w]] = Some(String::new());
                        }
                    }
                    continue 'outer;
                }
            }
        }

        let mut out = String::with_capacity(text.len());
        for (i, span) in spans.iter().enumerate() {
            match &replacements[i] {
                Some(s) => out.push_str(s),
                None => match span {
                    Span::Word(s) | Span::Gap(s) => out.push_str(s),
                },
            }
        }
        out
    }

    fn portcullis_term(policy: SnapPolicy) -> CanonicalTerm {
        CanonicalTerm {
            term: "portcullis".to_string(),
            aliases: vec![
                "port cullis".to_string(),
                "portculis".to_string(),
                "portcolours".to_string(),
            ],
            policy,
            max_words: 3,
            threshold: None,
        }
    }

    fn litellm_term(policy: SnapPolicy) -> CanonicalTerm {
        CanonicalTerm {
            term: "LiteLLM".to_string(),
            aliases: vec!["lite llm".to_string()],
            policy,
            max_words: 2,
            threshold: None,
        }
    }

    // -------------------------------------------------------------------------
    // Phonetic variants
    // -------------------------------------------------------------------------

    #[test]
    fn test_phonetic_portcullis_alias() {
        // Explicit aliases snap via AliasOnly.
        let terms = vec![portcullis_term(SnapPolicy::AliasOnly)];
        assert_eq!(
            run_with_registry(terms.clone(), "lower the port cullis"),
            "lower the portcullis"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "lower the portculis"),
            "lower the portcullis"
        );
    }

    #[test]
    fn test_phonetic_portcullis_phonetic_policy() {
        let terms = vec![portcullis_term(SnapPolicy::Phonetic)];
        // "port colours" is not in the alias list — relies on phonetic gate.
        // portcullis and portcolours share phonetic key "PRTK".
        assert_eq!(
            run_with_registry(terms.clone(), "lower the port colours"),
            "lower the portcullis"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "lower the port collars"),
            "lower the portcullis"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "portcolours is strong"),
            "portcullis is strong"
        );
    }

    #[test]
    fn test_litellm_verbatim_casing() {
        // LiteLLM has intrinsic casing; must be inserted verbatim regardless of input case.
        let terms = vec![litellm_term(SnapPolicy::AliasOnly)];
        // Exact alias match.
        assert_eq!(
            run_with_registry(terms.clone(), "use lite llm for this"),
            "use LiteLLM for this"
        );
        // Direct exact match on term (case-insensitive).
        assert_eq!(
            run_with_registry(terms.clone(), "use litellm for this"),
            "use LiteLLM for this"
        );
    }

    #[test]
    fn test_litellm_phonetic_policy() {
        let terms = vec![litellm_term(SnapPolicy::Phonetic)];
        // "light LLM" is phonetically similar.
        assert_eq!(
            run_with_registry(terms.clone(), "run light LLM locally"),
            "run LiteLLM locally"
        );
    }

    // -------------------------------------------------------------------------
    // N-gram / longest-match
    // -------------------------------------------------------------------------

    #[test]
    fn test_ngram_two_token_beats_one_token() {
        // Register a 1-word term for "port" and a 2-word term for "port colours" -> portcullis.
        let terms = vec![
            CanonicalTerm {
                term: "port".to_string(),
                aliases: vec![],
                policy: SnapPolicy::AliasOnly,
                max_words: 1,
                threshold: None,
            },
            CanonicalTerm {
                term: "portcullis".to_string(),
                aliases: vec!["port colours".to_string()],
                policy: SnapPolicy::AliasOnly,
                max_words: 2,
                threshold: None,
            },
        ];
        // The 2-gram "port colours" should win over the 1-gram "port".
        assert_eq!(
            run_with_registry(terms, "lower the port colours"),
            "lower the portcullis"
        );
    }

    // -------------------------------------------------------------------------
    // False-positive guards
    // -------------------------------------------------------------------------

    #[test]
    fn test_conservative_immich_leaves_image_alone() {
        let terms = vec![CanonicalTerm {
            term: "immich".to_string(),
            aliases: vec![],
            policy: SnapPolicy::Conservative,
            max_words: 1,
            threshold: None,
        }];
        // "image" is not phonetically similar to "immich".
        assert_eq!(
            run_with_registry(terms.clone(), "attach the image"),
            "attach the image"
        );
        assert_eq!(run_with_registry(terms.clone(), "image"), "image");
    }

    #[test]
    fn test_alias_only_immich_leaves_image_alone() {
        let terms = vec![CanonicalTerm {
            term: "immich".to_string(),
            aliases: vec![],
            policy: SnapPolicy::AliasOnly,
            max_words: 1,
            threshold: None,
        }];
        assert_eq!(
            run_with_registry(terms.clone(), "view the image gallery"),
            "view the image gallery"
        );
    }

    #[test]
    fn test_phonetic_portcullis_does_not_snap_standalone_colours() {
        // "colours" alone must NOT snap to "portcullis" even under Phonetic policy —
        // the phonetic key for "colours" is "KLRS", which doesn't match "PRTK".
        let terms = vec![portcullis_term(SnapPolicy::Phonetic)];
        assert_eq!(
            run_with_registry(terms, "the colours of autumn"),
            "the colours of autumn"
        );
    }

    // -------------------------------------------------------------------------
    // Phonetic AND-gate regression (folder/filter false-positive)
    // -------------------------------------------------------------------------

    fn vaultwarden_term() -> CanonicalTerm {
        CanonicalTerm {
            term: "Vaultwarden".to_string(),
            aliases: vec![],
            policy: SnapPolicy::Phonetic,
            max_words: 1,
            threshold: None,
        }
    }

    #[test]
    fn test_phonetic_folder_does_not_snap_to_vaultwarden() {
        // "folder" and "Vaultwarden" share Double-Metaphone code FLTR (4-char truncation),
        // but NDL("folder", "vaultwarden") ≈ 0.27, which is below the 0.55 threshold.
        // The AND gate must block this false positive.
        let terms = vec![vaultwarden_term()];
        assert_eq!(
            run_with_registry(terms.clone(), "open the folder"),
            "open the folder",
            "folder must NOT snap to Vaultwarden"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "folder"),
            "folder",
            "standalone folder must NOT snap to Vaultwarden"
        );
    }

    #[test]
    fn test_phonetic_filter_does_not_snap_to_vaultwarden() {
        // "filter" also reduces to FLTR — same coarse-code false-positive class.
        let terms = vec![vaultwarden_term()];
        assert_eq!(
            run_with_registry(terms.clone(), "apply the filter"),
            "apply the filter",
            "filter must NOT snap to Vaultwarden"
        );
    }

    #[test]
    fn test_phonetic_genuine_variants_still_snap() {
        // Verify that the stricter AND gate does not block legitimate mishearings.
        // "portcolours" → portcullis: phonetic match (both PRTK) + NDL ≈ 0.64 >= 0.55.
        let portcullis = portcullis_term(SnapPolicy::Phonetic);
        assert_eq!(
            run_with_registry(vec![portcullis], "lower the portcolours"),
            "lower the portcullis",
            "portcolours should still snap to portcullis (NDL >= 0.55)"
        );

        // "port colours" → portcullis via the two-word alias path (multi-word AND gate).
        let portcullis2 = portcullis_term(SnapPolicy::Phonetic);
        assert_eq!(
            run_with_registry(vec![portcullis2], "lower the port colours"),
            "lower the portcullis",
            "port colours should still snap to portcullis"
        );
    }

    // -------------------------------------------------------------------------
    // Threshold boundary
    // -------------------------------------------------------------------------

    #[test]
    fn test_threshold_boundary() {
        // Conservative (AND gate): phonetic key must match AND ndl >= threshold.
        // "portculis" (one l): phonetic key "PRTK" matches AND ndl ~0.9 → snaps.
        // "portklezm": phonetic key "PRTK" matches but ndl ~0.5 < 0.85 → blocked.
        let conservative_terms = vec![CanonicalTerm {
            term: "portcullis".to_string(),
            aliases: vec![],
            policy: SnapPolicy::Conservative,
            max_words: 1,
            threshold: Some(0.85),
        }];
        assert_eq!(
            run_with_registry(conservative_terms.clone(), "portculis"),
            "portcullis",
            "portculis should snap (phonetic match + ndl ~0.9 >= 0.85)"
        );
        assert_eq!(
            run_with_registry(conservative_terms.clone(), "portklezm"),
            "portklezm",
            "portklezm should not snap (phonetic match but ndl ~0.5 < 0.85)"
        );

        // Phonetic (AND gate, explicit threshold 0.85): phonetic key matches AND
        // ndl ~0.9 >= 0.85, so "portculis" snaps.
        let phonetic_terms = vec![CanonicalTerm {
            term: "portcullis".to_string(),
            aliases: vec![],
            policy: SnapPolicy::Phonetic,
            max_words: 1,
            threshold: Some(0.85),
        }];
        assert_eq!(
            run_with_registry(phonetic_terms, "portculis"),
            "portcullis",
            "portculis should snap under phonetic policy (phon match + ndl ~0.9 >= 0.85)"
        );
    }

    #[test]
    fn test_per_term_threshold_override() {
        // Conservative AND gate + threshold 0.99: "portculis" phonetically matches
        // but NDL ~0.9 < 0.99 fails the AND gate, so it should not snap.
        let strict_terms = [CanonicalTerm {
            term: "portcullis".to_string(),
            aliases: vec![],
            policy: SnapPolicy::Conservative,
            max_words: 1,
            threshold: Some(0.99),
        }];
        assert_eq!(
            run_with_registry(strict_terms.to_vec(), "portculis"),
            "portculis",
            "Conservative + threshold 0.99: portculis should not snap"
        );
    }

    // -------------------------------------------------------------------------
    // Seed-from-dictionary
    // -------------------------------------------------------------------------

    #[test]
    fn test_seed_deduplication() {
        // Simulate dictionary entries with duplicate `to` values.
        let dict_entries = vec![
            crate::dictionary::DictionaryEntry {
                from: "port cullis".to_string(),
                to: "portcullis".to_string(),
                case_sensitive: false,
            },
            crate::dictionary::DictionaryEntry {
                from: "portcolours".to_string(),
                to: "portcullis".to_string(),
                case_sensitive: false,
            },
            crate::dictionary::DictionaryEntry {
                from: "portcolors".to_string(),
                to: "portcullis".to_string(),
                case_sensitive: false,
            },
        ];

        // Manually replicate the seeding logic.
        let mut map: std::collections::HashMap<String, (String, Vec<String>)> =
            std::collections::HashMap::new();
        for entry in &dict_entries {
            let key = entry.to.to_lowercase();
            let slot = map
                .entry(key)
                .or_insert_with(|| (entry.to.clone(), Vec::new()));
            let from_lc = entry.from.to_lowercase();
            if from_lc != slot.0.to_lowercase()
                && !slot.1.iter().any(|a| a.to_lowercase() == from_lc)
            {
                slot.1.push(entry.from.clone());
            }
        }

        assert_eq!(
            map.len(),
            1,
            "Three entries with same `to` must collapse to one term"
        );
        let (term, aliases) = map.values().next().unwrap();
        assert_eq!(term, "portcullis");
        assert_eq!(aliases.len(), 3);
    }

    #[test]
    fn test_seed_policy_defaults_to_alias_only() {
        let term = CanonicalTerm {
            term: "portcullis".to_string(),
            aliases: vec![],
            policy: SnapPolicy::default(),
            max_words: default_max_words(),
            threshold: None,
        };
        assert_eq!(term.policy, SnapPolicy::AliasOnly);
    }

    // -------------------------------------------------------------------------
    // Serialisation round-trip
    // -------------------------------------------------------------------------

    #[test]
    fn test_snap_policy_serialisation() {
        for (policy, expected_json) in [
            (SnapPolicy::AliasOnly, "\"aliasOnly\""),
            (SnapPolicy::Phonetic, "\"phonetic\""),
            (SnapPolicy::Conservative, "\"conservative\""),
        ] {
            let json = serde_json::to_string(&policy).unwrap();
            assert_eq!(json, expected_json, "policy {:?} wrong JSON", policy);
            let back: SnapPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(back, policy);
        }
    }

    #[test]
    fn test_canonical_term_round_trip() {
        let term = CanonicalTerm {
            term: "LiteLLM".to_string(),
            aliases: vec!["lite llm".to_string(), "light llm".to_string()],
            policy: SnapPolicy::Phonetic,
            max_words: 2,
            threshold: Some(0.72),
        };
        let json = serde_json::to_string_pretty(&term).unwrap();
        let back: CanonicalTerm = serde_json::from_str(&json).unwrap();
        assert_eq!(back.term, term.term);
        assert_eq!(back.aliases, term.aliases);
        assert_eq!(back.policy, term.policy);
        assert_eq!(back.max_words, term.max_words);
        assert_eq!(back.threshold, term.threshold);
    }

    // -------------------------------------------------------------------------
    // Casing
    // -------------------------------------------------------------------------

    #[test]
    fn test_verbatim_casing_for_cased_terms() {
        let terms = vec![litellm_term(SnapPolicy::AliasOnly)];
        // Regardless of how the alias is cased in the transcript, canonical form is verbatim.
        assert_eq!(
            run_with_registry(terms.clone(), "LITE LLM works"),
            "LiteLLM works"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "Lite Llm works"),
            "LiteLLM works"
        );
    }

    #[test]
    fn test_restore_case_for_lowercase_terms() {
        let terms = vec![portcullis_term(SnapPolicy::AliasOnly)];
        // The term is all-lowercase; casing should follow the matched span.
        assert_eq!(
            run_with_registry(terms.clone(), "PORTCULIS in the gate"),
            "PORTCULLIS in the gate"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "Port cullis stands"),
            "Portcullis stands"
        );
    }

    // -------------------------------------------------------------------------
    // Lossless reconstruction
    // -------------------------------------------------------------------------

    #[test]
    fn test_no_match_returns_identical() {
        let terms = vec![portcullis_term(SnapPolicy::Phonetic)];
        let text = "the quick brown fox jumps, over the lazy dog.";
        let result = run_with_registry(terms, text);
        assert_eq!(result, text, "no-match text must be byte-identical");
    }

    #[test]
    fn test_punctuation_preserved_around_snap() {
        let terms = vec![portcullis_term(SnapPolicy::AliasOnly)];
        assert_eq!(
            run_with_registry(terms.clone(), "lower the port cullis!"),
            "lower the portcullis!"
        );
        assert_eq!(
            run_with_registry(terms.clone(), "(port cullis)"),
            "(portcullis)"
        );
    }

    #[test]
    fn test_length_guard_skips_short_candidates() {
        // 3-char word "tic" should not attempt matching (len < 4 guard).
        let terms = vec![CanonicalTerm {
            term: "tic".to_string(), // 3 chars
            aliases: vec![],
            policy: SnapPolicy::Phonetic,
            max_words: 1,
            threshold: None,
        }];
        let text = "a tic here";
        let result = run_with_registry(terms, text);
        assert_eq!(result, text);
    }
}
