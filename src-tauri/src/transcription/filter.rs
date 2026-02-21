//! Transcription output filtering
//!
//! Provides text cleaning and normalisation for transcription output.
//! Removes filler words, normalises whitespace, cleans up punctuation,
//! and applies dictionary word replacements.

use crate::dictionary;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Options for filtering transcription output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FilterOptions {
    /// Remove common filler words (um, uh, er, ah, like, you know)
    pub remove_fillers: bool,
    /// Normalise whitespace (collapse multiple spaces, trim)
    pub normalise_whitespace: bool,
    /// Clean up punctuation (remove duplicate punctuation, fix spacing)
    pub cleanup_punctuation: bool,
    /// Convert to sentence case (capitalise first letter of sentences)
    pub sentence_case: bool,
    /// Apply dictionary word replacements
    #[serde(default = "default_apply_dictionary")]
    pub apply_dictionary: bool,
}

fn default_apply_dictionary() -> bool {
    true
}

impl Default for FilterOptions {
    fn default() -> Self {
        Self {
            remove_fillers: true,
            normalise_whitespace: true,
            cleanup_punctuation: true,
            sentence_case: false,
            apply_dictionary: true,
        }
    }
}

/// Common filler words and sounds to remove
static FILLER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern matches filler words as whole words (case-insensitive)
    // Includes: um, uh, er, ah, like (when used as filler), you know, y'know
    Regex::new(r"(?i)\b(u+[hm]+|e+r+|a+h+|like,?\s+|you know,?\s*|y'know,?\s*)\b").unwrap()
});

/// Multiple whitespace pattern
static MULTI_SPACE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {2,}").unwrap());

/// Duplicate period pattern
static DUPLICATE_PERIOD_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.{2,}").unwrap());

/// Duplicate exclamation pattern
static DUPLICATE_EXCLAIM_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!{2,}").unwrap());

/// Duplicate question pattern
static DUPLICATE_QUESTION_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\?{2,}").unwrap());

/// Space before punctuation pattern
static SPACE_BEFORE_PUNCT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+([.!?,;:])").unwrap());

/// Missing space after punctuation pattern
static MISSING_SPACE_AFTER_PUNCT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([.!?,;:])([A-Za-z])").unwrap());

/// Sentence start pattern (for capitalisation)
static SENTENCE_START_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[.!?]\s+)([a-z])").unwrap());

/// Output filter for transcription text
#[derive(Debug, Default)]
pub struct OutputFilter {
    options: FilterOptions,
}

impl OutputFilter {
    /// Create a new output filter with the given options
    pub fn new(options: FilterOptions) -> Self {
        Self { options }
    }

    /// Create a new output filter with default options
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Filter the given text according to the configured options
    pub fn filter(&self, text: &str) -> String {
        let mut result = text.to_string();

        // Apply dictionary replacements first (before other processing)
        if self.options.apply_dictionary {
            result = dictionary::apply_dictionary(&result);
        }

        if self.options.remove_fillers {
            result = remove_filler_words(&result);
        }

        if self.options.cleanup_punctuation {
            result = cleanup_punctuation(&result);
        }

        if self.options.normalise_whitespace {
            result = normalise_whitespace(&result);
        }

        if self.options.sentence_case {
            result = apply_sentence_case(&result);
        }

        result
    }
}

/// Remove common filler words and sounds from text
pub fn remove_filler_words(text: &str) -> String {
    FILLER_PATTERN.replace_all(text, "").to_string()
}

/// Normalise whitespace by collapsing multiple spaces and trimming
pub fn normalise_whitespace(text: &str) -> String {
    let result = MULTI_SPACE_PATTERN.replace_all(text, " ");
    result.trim().to_string()
}

/// Clean up punctuation issues
pub fn cleanup_punctuation(text: &str) -> String {
    // Remove duplicate punctuation (... -> ., !!! -> !, ??? -> ?)
    let result = DUPLICATE_PERIOD_PATTERN.replace_all(text, ".");
    let result = DUPLICATE_EXCLAIM_PATTERN.replace_all(&result, "!");
    let result = DUPLICATE_QUESTION_PATTERN.replace_all(&result, "?");

    // Remove spaces before punctuation
    let result = SPACE_BEFORE_PUNCT_PATTERN.replace_all(&result, "$1");

    // Add space after punctuation if missing (before a letter)
    MISSING_SPACE_AFTER_PUNCT_PATTERN
        .replace_all(&result, "$1 $2")
        .to_string()
}

/// Approximate word count threshold before paragraph breaks are inserted.
const PARAGRAPH_WORD_THRESHOLD: usize = 50;

/// Format long text into paragraphs by inserting double-newline breaks at
/// sentence boundaries approximately every ~50 words.
///
/// Short texts (fewer than ~50 words) are returned unchanged.
pub fn format_paragraphs(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < PARAGRAPH_WORD_THRESHOLD {
        return text.to_string();
    }

    // Rebuild the text, inserting paragraph breaks at sentence-ending
    // punctuation nearest to each ~50-word boundary.
    let mut result = String::with_capacity(text.len() + 32);
    let mut word_count: usize = 0;
    let mut looking_for_break = false;

    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            if looking_for_break && ends_sentence(words[i - 1]) {
                result.push_str("\n\n");
                looking_for_break = false;
                word_count = 0;
            } else {
                result.push(' ');
            }
        }

        result.push_str(word);
        word_count += 1;

        if word_count >= PARAGRAPH_WORD_THRESHOLD && !looking_for_break {
            looking_for_break = true;
        }
    }

    result
}

/// Check whether a word ends with sentence-terminating punctuation.
fn ends_sentence(word: &str) -> bool {
    matches!(word.as_bytes().last(), Some(b'.' | b'?' | b'!'))
}

/// Apply sentence case (capitalise first letter of each sentence)
pub fn apply_sentence_case(text: &str) -> String {
    SENTENCE_START_PATTERN
        .replace_all(text, |caps: &regex::Captures| {
            let prefix = caps.get(1).map_or("", |m| m.as_str());
            let letter = caps.get(2).map_or("", |m| m.as_str());
            format!("{}{}", prefix, letter.to_uppercase())
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Filler word removal tests

    #[test]
    fn test_remove_um() {
        assert_eq!(remove_filler_words("I um think so"), "I  think so");
        assert_eq!(remove_filler_words("Um hello"), " hello");
        assert_eq!(remove_filler_words("hello um"), "hello ");
    }

    #[test]
    fn test_remove_uh() {
        assert_eq!(remove_filler_words("I uh need help"), "I  need help");
        assert_eq!(remove_filler_words("Uh what"), " what");
    }

    #[test]
    fn test_remove_er() {
        assert_eq!(remove_filler_words("I er don't know"), "I  don't know");
        assert_eq!(remove_filler_words("Well er yes"), "Well  yes");
    }

    #[test]
    fn test_remove_ah() {
        assert_eq!(remove_filler_words("Ah I see"), " I see");
        assert_eq!(remove_filler_words("So ah yes"), "So  yes");
    }

    #[test]
    fn test_remove_like_filler() {
        assert_eq!(remove_filler_words("I was like thinking"), "I was thinking");
        assert_eq!(remove_filler_words("It's like so good"), "It's so good");
    }

    #[test]
    fn test_remove_you_know() {
        assert_eq!(
            remove_filler_words("I was, you know, thinking"),
            "I was, thinking"
        );
        assert_eq!(remove_filler_words("You know what I mean"), "what I mean");
    }

    #[test]
    fn test_remove_y_know() {
        assert_eq!(remove_filler_words("I was, y'know, busy"), "I was, busy");
    }

    #[test]
    fn test_remove_multiple_fillers() {
        assert_eq!(
            remove_filler_words("Um, I was, uh, like thinking, you know"),
            ", I was, , thinking, "
        );
    }

    #[test]
    fn test_case_insensitive_fillers() {
        assert_eq!(remove_filler_words("UM hello"), " hello");
        assert_eq!(remove_filler_words("I UH think"), "I  think");
        assert_eq!(remove_filler_words("Like cool"), "cool");
    }

    // Whitespace normalisation tests

    #[test]
    fn test_collapse_multiple_spaces() {
        assert_eq!(normalise_whitespace("hello  world"), "hello world");
        assert_eq!(normalise_whitespace("hello    world"), "hello world");
    }

    #[test]
    fn test_trim_whitespace() {
        assert_eq!(normalise_whitespace("  hello  "), "hello");
        assert_eq!(normalise_whitespace("   hello world   "), "hello world");
    }

    #[test]
    fn test_preserve_single_spaces() {
        assert_eq!(normalise_whitespace("hello world"), "hello world");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(normalise_whitespace(""), "");
        assert_eq!(normalise_whitespace("   "), "");
    }

    // Punctuation cleanup tests

    #[test]
    fn test_remove_duplicate_periods() {
        assert_eq!(cleanup_punctuation("Hello..."), "Hello.");
        assert_eq!(cleanup_punctuation("What.."), "What.");
    }

    #[test]
    fn test_remove_duplicate_exclamations() {
        assert_eq!(cleanup_punctuation("Hello!!!"), "Hello!");
        assert_eq!(cleanup_punctuation("Wow!!"), "Wow!");
    }

    #[test]
    fn test_remove_duplicate_questions() {
        assert_eq!(cleanup_punctuation("What???"), "What?");
        assert_eq!(cleanup_punctuation("Really??"), "Really?");
    }

    #[test]
    fn test_remove_space_before_punctuation() {
        assert_eq!(cleanup_punctuation("Hello ."), "Hello.");
        assert_eq!(cleanup_punctuation("What ?"), "What?");
        assert_eq!(cleanup_punctuation("Wow !"), "Wow!");
    }

    #[test]
    fn test_add_space_after_punctuation() {
        assert_eq!(cleanup_punctuation("Hello.World"), "Hello. World");
        assert_eq!(cleanup_punctuation("What?Really"), "What? Really");
    }

    #[test]
    fn test_punctuation_combined() {
        assert_eq!(cleanup_punctuation("Hello ...World"), "Hello. World");
        assert_eq!(cleanup_punctuation("What ??Really"), "What? Really");
    }

    // Sentence case tests

    #[test]
    fn test_capitalise_first_letter() {
        assert_eq!(apply_sentence_case("hello world"), "Hello world");
    }

    #[test]
    fn test_capitalise_after_period() {
        assert_eq!(apply_sentence_case("hello. world"), "Hello. World");
    }

    #[test]
    fn test_capitalise_after_question() {
        assert_eq!(apply_sentence_case("what? yes"), "What? Yes");
    }

    #[test]
    fn test_capitalise_after_exclamation() {
        assert_eq!(apply_sentence_case("wow! amazing"), "Wow! Amazing");
    }

    #[test]
    fn test_preserve_existing_capitals() {
        assert_eq!(apply_sentence_case("Hello World"), "Hello World");
    }

    #[test]
    fn test_multiple_sentences() {
        assert_eq!(
            apply_sentence_case("hello. how are you? fine! good."),
            "Hello. How are you? Fine! Good."
        );
    }

    // OutputFilter integration tests

    #[test]
    fn test_filter_with_all_options() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: true,
            normalise_whitespace: true,
            cleanup_punctuation: true,
            sentence_case: true,
            apply_dictionary: false, // Disable for test isolation
        });

        let input = "um, I was like  thinking...what do you think ??";
        let result = filter.filter(input);

        // Should remove fillers, normalise spaces, clean punctuation, apply sentence case
        assert_eq!(result, ", I was thinking. What do you think?");
    }

    #[test]
    fn test_filter_with_no_options() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: false,
            normalise_whitespace: false,
            cleanup_punctuation: false,
            sentence_case: false,
            apply_dictionary: false,
        });

        let input = "um  hello...";
        let result = filter.filter(input);

        // Should not modify the text
        assert_eq!(result, "um  hello...");
    }

    #[test]
    fn test_filter_defaults() {
        let _filter = OutputFilter::with_defaults();
        let options = FilterOptions::default();

        assert!(options.remove_fillers);
        assert!(options.normalise_whitespace);
        assert!(options.cleanup_punctuation);
        assert!(!options.sentence_case);
        assert!(options.apply_dictionary);
    }

    #[test]
    fn test_filter_only_fillers() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: true,
            normalise_whitespace: false,
            cleanup_punctuation: false,
            sentence_case: false,
            apply_dictionary: false,
        });

        let input = "I um think so";
        assert_eq!(filter.filter(input), "I  think so");
    }

    #[test]
    fn test_filter_only_whitespace() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: false,
            normalise_whitespace: true,
            cleanup_punctuation: false,
            sentence_case: false,
            apply_dictionary: false,
        });

        let input = "  hello   world  ";
        assert_eq!(filter.filter(input), "hello world");
    }

    #[test]
    fn test_filter_empty_string() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: true,
            normalise_whitespace: true,
            cleanup_punctuation: true,
            sentence_case: false,
            apply_dictionary: false,
        });
        assert_eq!(filter.filter(""), "");
    }

    // Paragraph formatting tests

    #[test]
    fn test_format_paragraphs_short_text_unchanged() {
        let text = "This is a short sentence. It has fewer than fifty words.";
        assert_eq!(format_paragraphs(text), text);
    }

    #[test]
    fn test_format_paragraphs_empty_string() {
        assert_eq!(format_paragraphs(""), "");
    }

    #[test]
    fn test_format_paragraphs_exactly_at_threshold() {
        // Build a text with exactly 49 words (below threshold)
        let words: Vec<&str> = (0..49).map(|_| "word").collect();
        let text = words.join(" ");
        assert_eq!(format_paragraphs(&text), text);
    }

    #[test]
    fn test_format_paragraphs_inserts_break_at_sentence_boundary() {
        // Build text: 50+ words with a sentence ending around word 50
        let mut parts = Vec::new();
        // First ~52 words ending in a period
        for i in 0..52 {
            if i == 51 {
                parts.push("end.");
            } else {
                parts.push("word");
            }
        }
        // More words after
        for _ in 0..10 {
            parts.push("more");
        }
        let text = parts.join(" ");
        let result = format_paragraphs(&text);

        assert!(
            result.contains("\n\n"),
            "Should contain paragraph break, got: {result}"
        );
        // The break should come after "end."
        let break_pos = result.find("\n\n").unwrap();
        let before_break = &result[..break_pos];
        assert!(
            before_break.ends_with("end."),
            "Break should come after sentence-ending punctuation, before: '{before_break}'"
        );
    }

    #[test]
    fn test_format_paragraphs_no_sentence_boundary_no_break() {
        // 60+ words with NO sentence-ending punctuation at all
        let words: Vec<&str> = (0..70).map(|_| "word").collect();
        let text = words.join(" ");
        let result = format_paragraphs(&text);
        // No sentence boundary → no break inserted
        assert!(
            !result.contains("\n\n"),
            "Should not insert break without sentence boundary"
        );
    }

    #[test]
    fn test_format_paragraphs_multiple_breaks() {
        // ~150 words with sentence boundaries at ~50 and ~100
        let mut parts = Vec::new();
        for i in 0..150 {
            if i == 51 || i == 102 {
                parts.push("stop.");
            } else {
                parts.push("word");
            }
        }
        let text = parts.join(" ");
        let result = format_paragraphs(&text);

        let break_count = result.matches("\n\n").count();
        assert!(
            break_count >= 2,
            "Should have at least 2 paragraph breaks for ~150 words, got {break_count}"
        );
    }

    #[test]
    fn test_format_paragraphs_question_and_exclamation() {
        // Verify ? and ! also trigger paragraph breaks
        let mut parts = Vec::new();
        for i in 0..110 {
            if i == 51 {
                parts.push("right?");
            } else if i == 103 {
                parts.push("great!");
            } else {
                parts.push("word");
            }
        }
        let text = parts.join(" ");
        let result = format_paragraphs(&text);

        assert!(result.contains("\n\n"), "Should break at ? or ! boundaries");
    }

    #[test]
    fn test_real_world_transcription() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: true,
            normalise_whitespace: true,
            cleanup_punctuation: true,
            sentence_case: true,
            apply_dictionary: false, // Disable for test isolation
        });

        let input = "um so like I was thinking you know about the project...and uh I think we should like move forward with it what do you think ??";
        let result = filter.filter(input);

        assert_eq!(
            result,
            "So I was thinking about the project. And I think we should move forward with it what do you think?"
        );
    }
}
