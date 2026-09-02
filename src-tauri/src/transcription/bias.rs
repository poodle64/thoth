//! Decode-time vocabulary bias for the Whisper backend (#106).
//!
//! Dictionary and canonical-term correction runs *after* Whisper has produced
//! text, so it can only repair a word the model already got close enough to
//! fuzzy-match. Feeding the same terms to the decoder as an initial prompt lets
//! the model pick the right word while the acoustics are still ambiguous, which
//! is the case post-hoc correction cannot reach.

use std::collections::HashSet;

/// Whisper truncates the initial prompt to half the text context — 224 tokens
/// for every model Thoth ships — and a long prompt is what makes whisper.cpp
/// start emitting prompt words that were never spoken. 512 characters sits well
/// inside that, and the terms that do not fit are still corrected post-hoc.
const MAX_PROMPT_CHARS: usize = 512;

/// Join the user's own terms into an initial prompt, or `None` when there are
/// none to bias with.
///
/// Case-insensitively deduplicated, first spelling wins, and terms are taken in
/// order until the cap — so a user whose list outgrows the prompt gets a stable
/// prefix rather than a set that reshuffles between dictations.
pub fn build_initial_prompt<I: IntoIterator<Item = String>>(terms: I) -> Option<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut prompt = String::new();

    for term in terms {
        let term = term.trim();
        // A NUL would panic inside whisper-rs's CString conversion, and a
        // dictionary is a file a user can hand-edit.
        if term.is_empty() || term.contains('\0') {
            continue;
        }
        if !seen.insert(term.to_lowercase()) {
            continue;
        }
        let separator = if prompt.is_empty() { "" } else { ", " };
        if prompt.chars().count() + separator.len() + term.chars().count() > MAX_PROMPT_CHARS {
            break;
        }
        prompt.push_str(separator);
        prompt.push_str(term);
    }

    (!prompt.is_empty()).then_some(prompt)
}

/// The user's dictionary replacements and canonical terms as a decode bias, or
/// `None` when they have none or have turned the bias off.
pub fn vocabulary_bias() -> Option<String> {
    let enabled = crate::config::get_config()
        .map(|c| c.transcription.vocabulary_bias)
        .unwrap_or(true);
    if !enabled {
        return None;
    }

    let canonical = crate::canonical::get_canonical_terms()
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.term);

    build_initial_prompt(
        crate::dictionary::get_vocabulary_for_context()
            .into_iter()
            .chain(canonical),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_terms_means_no_prompt() {
        assert_eq!(build_initial_prompt(Vec::<String>::new()), None);
        assert_eq!(
            build_initial_prompt(vec!["".to_string(), "   ".to_string()]),
            None
        );
    }

    #[test]
    fn terms_are_joined_in_order() {
        assert_eq!(
            build_initial_prompt(vec!["Thoth".to_string(), "Godswood".to_string()]),
            Some("Thoth, Godswood".to_string())
        );
    }

    #[test]
    fn duplicates_are_dropped_case_insensitively() {
        assert_eq!(
            build_initial_prompt(vec![
                "Thoth".to_string(),
                "thoth".to_string(),
                "THOTH".to_string()
            ]),
            Some("Thoth".to_string())
        );
    }

    /// A NUL byte reaches whisper-rs's `CString::new(...).expect(...)`, which
    /// panics — on the transcription path, from a file the user can edit.
    #[test]
    fn a_term_with_a_nul_byte_is_dropped() {
        let prompt = build_initial_prompt(vec!["safe".to_string(), "bad\0term".to_string()]);
        assert_eq!(prompt, Some("safe".to_string()));
    }

    #[test]
    fn the_prompt_is_capped_and_keeps_a_stable_prefix() {
        let terms: Vec<String> = (0..200).map(|i| format!("term{i:04}")).collect();
        let prompt = build_initial_prompt(terms.clone()).expect("some terms fit");
        assert!(
            prompt.chars().count() <= MAX_PROMPT_CHARS,
            "prompt was {} chars",
            prompt.chars().count()
        );
        assert!(prompt.starts_with("term0000, term0001, "));
        // The cap must drop the tail, not the head: the same list must produce
        // the same prompt every time.
        assert_eq!(build_initial_prompt(terms), Some(prompt));
    }
}
