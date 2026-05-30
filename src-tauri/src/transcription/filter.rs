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
    /// Remove hesitation sounds (um, uh, er, ah)
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
    /// Convert US spellings to Australian/British equivalents
    #[serde(default)]
    pub australian_spelling: bool,
    /// Convert spoken number words to digits ("twenty three" → "23")
    #[serde(default)]
    pub spoken_numbers_to_digits: bool,
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
            australian_spelling: false,
            spoken_numbers_to_digits: false,
        }
    }
}

/// Common filler words and sounds to remove
static FILLER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    // Pattern matches hesitation sounds as whole words (case-insensitive).
    // Only unambiguous non-words: um, uh, er, ah.
    // "like" and "you know" are excluded — they have legitimate grammatical
    // uses that a regex cannot distinguish from filler.
    Regex::new(r"(?i)\b(u+[hm]+|e+r+|a+h+)\b").unwrap()
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

        // ITN and AU spelling run after fillers so they see clean input,
        // but before punctuation/whitespace cleanup which tidies any artefacts.
        if self.options.spoken_numbers_to_digits {
            result = spoken_numbers_to_digits(&result);
        }

        if self.options.australian_spelling {
            result = apply_australian_spelling(&result);
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

/// Mapping of US spellings to Australian/British equivalents.
///
/// Entries are grouped by suffix family. Each tuple is (US word, AU word);
/// the replacement preserves leading capitalisation at apply time.
/// Inflected forms are listed explicitly — word-boundary regex cannot handle
/// partial-stem substitution safely enough for a curated list.
static AU_SPELLING_PAIRS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    // (US pattern [word-boundary, case-insensitive], AU replacement [lowercase])
    let pairs: &[(&str, &str)] = &[
        // -or → -our
        (r"\bcolor\b", "colour"),
        (r"\bcolors\b", "colours"),
        (r"\bcolored\b", "coloured"),
        (r"\bcoloring\b", "colouring"),
        (r"\bfavor\b", "favour"),
        (r"\bfavors\b", "favours"),
        (r"\bfavored\b", "favoured"),
        (r"\bfavoring\b", "favouring"),
        (r"\bfavorite\b", "favourite"),
        (r"\bfavorites\b", "favourites"),
        (r"\bhonor\b", "honour"),
        (r"\bhonors\b", "honours"),
        (r"\bhonored\b", "honoured"),
        (r"\bhonoring\b", "honouring"),
        (r"\bhumor\b", "humour"),
        (r"\bhumors\b", "humours"),
        (r"\bhumored\b", "humoured"),
        (r"\bhumoring\b", "humouring"),
        (r"\blabor\b", "labour"),
        (r"\blabors\b", "labours"),
        (r"\blabored\b", "laboured"),
        (r"\blaboring\b", "labouring"),
        (r"\bneighbor\b", "neighbour"),
        (r"\bneighbors\b", "neighbours"),
        (r"\bneighborhood\b", "neighbourhood"),
        (r"\bneighborhoods\b", "neighbourhoods"),
        (r"\bneighboring\b", "neighbouring"),
        (r"\brumor\b", "rumour"),
        (r"\brumors\b", "rumours"),
        (r"\brumored\b", "rumoured"),
        (r"\bsavior\b", "saviour"),
        (r"\bsaviors\b", "saviours"),
        (r"\bflavor\b", "flavour"),
        (r"\bflavors\b", "flavours"),
        (r"\bflavored\b", "flavoured"),
        (r"\bflavoring\b", "flavouring"),
        (r"\bvapor\b", "vapour"),
        (r"\bvapors\b", "vapours"),
        // -er → -re (unit meanings; "meter" as instrument stays, only the unit sense changes)
        // Map "metre" as the unit spelling for these common cases.
        (r"\bcenter\b", "centre"),
        (r"\bcenters\b", "centres"),
        (r"\bcentered\b", "centred"),
        (r"\bcentering\b", "centring"),
        (r"\btheatre\b", "theatre"), // already AU; include so no double-map
        (r"\btheater\b", "theatre"),
        (r"\btheaters\b", "theatres"),
        (r"\blitre\b", "litre"), // already AU
        (r"\bliter\b", "litre"),
        (r"\bliters\b", "litres"),
        (r"\bfibre\b", "fibre"), // already AU
        (r"\bfiber\b", "fibre"),
        (r"\bfibers\b", "fibres"),
        (r"\bmaneuver\b", "manoeuvre"),
        (r"\bmaneuvers\b", "manoeuvres"),
        (r"\bmanoeuvre\b", "manoeuvre"), // already AU
        // -ize → -ise family (most common ASR outputs)
        (r"\borganize\b", "organise"),
        (r"\borganizes\b", "organises"),
        (r"\borganized\b", "organised"),
        (r"\borganizing\b", "organising"),
        (r"\borganization\b", "organisation"),
        (r"\borganizations\b", "organisations"),
        (r"\brecognize\b", "recognise"),
        (r"\brecognizes\b", "recognises"),
        (r"\brecognized\b", "recognised"),
        (r"\brecognizing\b", "recognising"),
        (r"\brecognition\b", "recognition"), // unchanged — keep to avoid false maps
        (r"\banalyze\b", "analyse"),
        (r"\banalyzes\b", "analyses"),
        (r"\banalyzed\b", "analysed"),
        (r"\banalyzing\b", "analysing"),
        (r"\bsymbolize\b", "symbolise"),
        (r"\bsymbolizes\b", "symbolises"),
        (r"\bsymbolized\b", "symbolised"),
        (r"\bcategorize\b", "categorise"),
        (r"\bcategorizes\b", "categorises"),
        (r"\bcategorized\b", "categorised"),
        (r"\bcategorizing\b", "categorising"),
        (r"\bprioritize\b", "prioritise"),
        (r"\bprioritizes\b", "prioritises"),
        (r"\bprioritized\b", "prioritised"),
        (r"\bprioritizing\b", "prioritising"),
        (r"\bspecialize\b", "specialise"),
        (r"\bspecializes\b", "specialises"),
        (r"\bspecialized\b", "specialised"),
        (r"\bspecializing\b", "specialising"),
        (r"\bspecialization\b", "specialisation"),
        (r"\butilize\b", "utilise"),
        (r"\butilizes\b", "utilises"),
        (r"\butilized\b", "utilised"),
        (r"\butilizing\b", "utilising"),
        (r"\bminimize\b", "minimise"),
        (r"\bminimizes\b", "minimises"),
        (r"\bminimized\b", "minimised"),
        (r"\bminimizing\b", "minimising"),
        (r"\bmaximize\b", "maximise"),
        (r"\bmaximizes\b", "maximises"),
        (r"\bmaximized\b", "maximised"),
        (r"\bmaximizing\b", "maximising"),
        (r"\bstandardize\b", "standardise"),
        (r"\bstandardizes\b", "standardises"),
        (r"\bstandardized\b", "standardised"),
        (r"\bstandardizing\b", "standardising"),
        // -ense → -ence
        (r"\bdefense\b", "defence"),
        (r"\bdefenses\b", "defences"),
        (r"\boffense\b", "offence"),
        (r"\boffenses\b", "offences"),
        // Double-consonant spelling differences (UK/AU doubles; US single)
        (r"\btraveling\b", "travelling"),
        (r"\btraveled\b", "travelled"),
        (r"\btraveler\b", "traveller"),
        (r"\btravelers\b", "travellers"),
        (r"\bcanceled\b", "cancelled"),
        (r"\bcanceling\b", "cancelling"),
        (r"\bcancellation\b", "cancellation"), // unchanged — both spellings exist; AU prefers double-l
        (r"\bmodeling\b", "modelling"),
        (r"\bmodeled\b", "modelled"),
        (r"\bmodeler\b", "modeller"),
        (r"\blabeled\b", "labelled"),
        (r"\blabeling\b", "labelling"),
        (r"\blabeler\b", "labeller"),
        (r"\bfulfill\b", "fulfil"),
        (r"\bfulfills\b", "fulfils"),
        (r"\bfulfilled\b", "fulfilled"), // unchanged — same
        (r"\benroll\b", "enrol"),
        (r"\benrolls\b", "enrols"),
        (r"\benrolled\b", "enrolled"), // unchanged
        (r"\bskillful\b", "skilful"),
        (r"\bskillfully\b", "skilfully"),
        // -og → -ogue
        (r"\bcatalog\b", "catalogue"),
        (r"\bcatalogs\b", "catalogues"),
        (r"\bdialog\b", "dialogue"),
        (r"\bdialogs\b", "dialogues"),
        (r"\bmonolog\b", "monologue"),
        (r"\banalog\b", "analogue"),
        (r"\banalogous\b", "analogous"), // unchanged
        // Miscellaneous
        (r"\bgray\b", "grey"),
        (r"\bgrays\b", "greys"),
        (r"\bgrayed\b", "greyed"),
        (r"\bplow\b", "plough"),
        (r"\bplows\b", "ploughs"),
        (r"\bplowed\b", "ploughed"),
        (r"\bmold\b", "mould"),
        (r"\bmolds\b", "moulds"),
        (r"\bmolded\b", "moulded"),
        (r"\bbehavior\b", "behaviour"),
        (r"\bbehaviors\b", "behaviours"),
        (r"\bbehavioral\b", "behavioural"),
    ];

    pairs
        .iter()
        .map(|(pattern, replacement)| {
            // (?i) makes the match case-insensitive; we restore case in the replacer.
            let re = Regex::new(&format!("(?i){}", pattern)).expect("AU spelling regex is valid");
            (re, *replacement)
        })
        .collect()
});

/// Apply Australian/British spelling substitutions.
///
/// Preserves leading capitalisation: if the matched word starts with an
/// uppercase letter the replacement is also capitalised.
pub fn apply_australian_spelling(text: &str) -> String {
    let mut result = text.to_string();
    for (re, replacement) in AU_SPELLING_PAIRS.iter() {
        result = re
            .replace_all(&result, |caps: &regex::Captures| {
                let matched = caps.get(0).map_or("", |m| m.as_str());
                // Preserve leading capital if the source word was capitalised.
                if matched.chars().next().is_some_and(|c| c.is_uppercase()) {
                    let mut capitalised = replacement.to_string();
                    if let Some(first) = capitalised.get_mut(0..1) {
                        first.make_ascii_uppercase();
                    }
                    capitalised
                } else {
                    replacement.to_string()
                }
            })
            .to_string();
    }
    result
}

// ── Spoken-number → digit inverse text normalisation ──────────────────────

/// Value of each recognised number word.
fn word_to_value(word: &str) -> Option<u64> {
    match word.to_ascii_lowercase().as_str() {
        "zero" => Some(0),
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "eleven" => Some(11),
        "twelve" => Some(12),
        "thirteen" => Some(13),
        "fourteen" => Some(14),
        "fifteen" => Some(15),
        "sixteen" => Some(16),
        "seventeen" => Some(17),
        "eighteen" => Some(18),
        "nineteen" => Some(19),
        "twenty" => Some(20),
        "thirty" => Some(30),
        "forty" => Some(40),
        "fifty" => Some(50),
        "sixty" => Some(60),
        "seventy" => Some(70),
        "eighty" => Some(80),
        "ninety" => Some(90),
        _ => None,
    }
}

/// Multiplier words — these scale an accumulated value rather than adding.
fn word_to_multiplier(word: &str) -> Option<u64> {
    match word.to_ascii_lowercase().as_str() {
        "hundred" => Some(100),
        "thousand" => Some(1_000),
        "million" => Some(1_000_000),
        "billion" => Some(1_000_000_000),
        _ => None,
    }
}

/// True if the token is a number word, multiplier, or the connective "and".
fn is_number_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower == "and" || word_to_value(&lower).is_some() || word_to_multiplier(&lower).is_some()
}

/// Parse a contiguous slice of number tokens into a single integer.
///
/// Algorithm mirrors how English number words compose:
///   - Accumulator tracks the in-progress sub-total.
///   - "hundred" multiplies the accumulator by 100.
///   - "thousand"/"million"/"billion" flush the accumulator into a high-order
///     bucket and reset it, so "two thousand and five" → 2005.
///   - "and" is consumed as a no-op connective (British style).
///   - Ones/teens/tens add to the accumulator.
fn tokens_to_number(tokens: &[&str]) -> Option<u64> {
    let mut total: u64 = 0;
    let mut current: u64 = 0;
    let mut had_value = false;

    for token in tokens {
        let lower = token.to_ascii_lowercase();

        if lower == "and" {
            // Connective — valid only between number words; skip.
            continue;
        }

        if let Some(val) = word_to_value(&lower) {
            current += val;
            had_value = true;
        } else if let Some(mult) = word_to_multiplier(&lower) {
            if mult == 100 {
                // "hundred" multiplies the current accumulator only.
                // If current is 0 (bare "hundred"), treat as 1 × 100.
                current = if current == 0 { 100 } else { current * 100 };
                had_value = true;
            } else {
                // "thousand", "million", "billion": flush current into total,
                // scaled by the multiplier.
                let segment = if current == 0 { 1 } else { current };
                total += segment * mult;
                current = 0;
                had_value = true;
            }
        }
    }

    if !had_value {
        return None;
    }

    Some(total + current)
}

/// Convert spoken number words to digit strings within arbitrary text.
///
/// Tokenises on whitespace, identifies runs of number words (including the
/// connective "and"), parses each run to an integer, and splices the digit
/// string back in place of the run. Non-number tokens and punctuation are
/// preserved verbatim.
///
/// Trade-off: lone "one" is converted to "1" in all positions since this is
/// a dictation tool and digit output is almost always the intent. Detecting
/// pronoun vs numeral use is not feasible with a simple rule-based approach.
pub fn spoken_numbers_to_digits(text: &str) -> String {
    // Tokenise into (preceding_whitespace, word) pairs so reconstruction is
    // simply `preceding_ws + word` for each token. This keeps the whitespace
    // model straightforward: the space *before* a token belongs to that token,
    // so when a number run is replaced with a digit string we emit the digit
    // string using the first run-token's preceding whitespace, and the next
    // token carries its own preceding whitespace unmodified.
    let mut tokens: Vec<(&str, &str)> = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < text.len() {
        // Consume preceding whitespace.
        let ws_start = pos;
        while pos < text.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        let preceding_ws = &text[ws_start..pos];

        if pos >= text.len() {
            // Trailing-only whitespace with no following word — emit as a
            // token with an empty word so it is included in the output.
            if !preceding_ws.is_empty() {
                tokens.push((preceding_ws, ""));
            }
            break;
        }

        // Consume a non-whitespace word.
        let word_start = pos;
        while pos < text.len() && bytes[pos] != b' ' && bytes[pos] != b'\t' {
            pos += 1;
        }
        let word = &text[word_start..pos];

        tokens.push((preceding_ws, word));
    }

    if tokens.is_empty() {
        return text.to_string();
    }

    /// Strip leading/trailing punctuation from a word to expose its alphabetic core.
    fn alpha_core(word: &str) -> &str {
        let start = word
            .char_indices()
            .find(|(_, c)| c.is_alphabetic())
            .map(|(i, _)| i)
            .unwrap_or(word.len());
        let end = word
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_alphabetic())
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        if start < end {
            &word[start..end]
        } else {
            ""
        }
    }

    let n = tokens.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < n {
        let (preceding_ws, word) = tokens[i];

        let core = alpha_core(word);

        if !core.is_empty() && is_number_token(core) {
            // Identify the full run of consecutive number tokens.
            // "and" is included only when it is followed by another number word,
            // preventing a trailing "and" from being swallowed.
            let run_start = i;
            let mut run_end = i;

            let mut j = i;
            while j < n {
                let (_, w) = tokens[j];
                let c = alpha_core(w);
                if c.is_empty() || !is_number_token(c) {
                    break;
                }
                if c.eq_ignore_ascii_case("and") {
                    let next_is_number = j + 1 < n && {
                        let (_, nw) = tokens[j + 1];
                        let nc = alpha_core(nw);
                        !nc.is_empty() && is_number_token(nc) && !nc.eq_ignore_ascii_case("and")
                    };
                    if !next_is_number {
                        break;
                    }
                }
                run_end = j;
                j += 1;
            }

            let run_cores: Vec<&str> = (run_start..=run_end)
                .map(|k| alpha_core(tokens[k].1))
                .collect();

            if let Some(number) = tokens_to_number(&run_cores) {
                // Emit: preceding whitespace of the first run token, then any
                // punctuation that prefixes the first word, then the digit string,
                // then any punctuation that suffixes the last word.
                // The next token's preceding_ws is emitted when that token is processed.
                let first_word = tokens[run_start].1;
                let last_word = tokens[run_end].1;

                out.push_str(preceding_ws);

                let first_core = alpha_core(first_word);
                if let Some(core_start) = first_word.find(first_core) {
                    out.push_str(&first_word[..core_start]);
                }

                out.push_str(&number.to_string());

                let last_core = alpha_core(last_word);
                if let Some(core_start) = last_word.find(last_core) {
                    let core_end = core_start + last_core.len();
                    out.push_str(&last_word[core_end..]);
                }

                i = run_end + 1;
                continue;
            }
        }

        // Not a number token (or parse yielded nothing) — emit verbatim.
        out.push_str(preceding_ws);
        out.push_str(word);
        i += 1;
    }

    out
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
    fn test_preserve_like() {
        // "like" is not removed — it has legitimate grammatical uses
        assert_eq!(
            remove_filler_words("I was like thinking"),
            "I was like thinking"
        );
        assert_eq!(
            remove_filler_words("It's like so good"),
            "It's like so good"
        );
        assert_eq!(remove_filler_words("I like coffee"), "I like coffee");
    }

    #[test]
    fn test_preserve_you_know() {
        // "you know" is not removed — it has legitimate grammatical uses
        assert_eq!(
            remove_filler_words("I was, you know, thinking"),
            "I was, you know, thinking"
        );
        assert_eq!(
            remove_filler_words("You know what I mean"),
            "You know what I mean"
        );
    }

    #[test]
    fn test_remove_multiple_fillers() {
        assert_eq!(
            remove_filler_words("Um, I was, uh, like thinking, you know"),
            ", I was, , like thinking, you know"
        );
    }

    #[test]
    fn test_case_insensitive_fillers() {
        assert_eq!(remove_filler_words("UM hello"), " hello");
        assert_eq!(remove_filler_words("I UH think"), "I  think");
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
            australian_spelling: false,
            spoken_numbers_to_digits: false,
        });

        let input = "um, I was like  thinking...what do you think ??";
        let result = filter.filter(input);

        // "um" removed; "like" preserved (not a hesitation sound); spaces normalised;
        // punctuation cleaned; sentence case applied
        assert_eq!(result, ", I was like thinking. What do you think?");
    }

    #[test]
    fn test_filter_with_no_options() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: false,
            normalise_whitespace: false,
            cleanup_punctuation: false,
            sentence_case: false,
            apply_dictionary: false,
            australian_spelling: false,
            spoken_numbers_to_digits: false,
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
            australian_spelling: false,
            spoken_numbers_to_digits: false,
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
            australian_spelling: false,
            spoken_numbers_to_digits: false,
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
            australian_spelling: false,
            spoken_numbers_to_digits: false,
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
            australian_spelling: false,
            spoken_numbers_to_digits: false,
        });

        let input = "um so like I was thinking you know about the project...and uh I think we should like move forward with it what do you think ??";
        let result = filter.filter(input);

        // "um" and "uh" removed; "like" and "you know" preserved; punctuation cleaned; sentence case applied
        assert_eq!(
            result,
            "So like I was thinking you know about the project. And I think we should like move forward with it what do you think?"
        );
    }

    // ── Australian spelling tests ──────────────────────────────────────────

    #[test]
    fn test_au_spelling_color_to_colour() {
        assert_eq!(apply_australian_spelling("color"), "colour");
        assert_eq!(apply_australian_spelling("colors"), "colours");
        assert_eq!(apply_australian_spelling("colored"), "coloured");
    }

    #[test]
    fn test_au_spelling_preserves_leading_capital() {
        assert_eq!(apply_australian_spelling("Organize"), "Organise");
        assert_eq!(apply_australian_spelling("organize"), "organise");
        assert_eq!(apply_australian_spelling("Color"), "Colour");
    }

    #[test]
    fn test_au_spelling_discolored() {
        // "discolored" is not in the map — should pass through unchanged.
        // Only exact listed forms are replaced; partial-stem substitution is not attempted.
        assert_eq!(apply_australian_spelling("discolored"), "discolored");
    }

    #[test]
    fn test_au_spelling_non_target_word_unchanged() {
        assert_eq!(apply_australian_spelling("hello world"), "hello world");
        assert_eq!(apply_australian_spelling("testing"), "testing");
    }

    #[test]
    fn test_au_spelling_mixed_sentence() {
        let input = "I like the color of that theater";
        let output = apply_australian_spelling(input);
        assert_eq!(output, "I like the colour of that theatre");
    }

    #[test]
    fn test_au_spelling_organization() {
        assert_eq!(apply_australian_spelling("organization"), "organisation");
        assert_eq!(apply_australian_spelling("organizations"), "organisations");
    }

    // ── Spoken-number ITN tests ────────────────────────────────────────────

    #[test]
    fn test_itn_twenty_three() {
        assert_eq!(spoken_numbers_to_digits("twenty three"), "23");
    }

    #[test]
    fn test_itn_one_hundred_and_fifty() {
        assert_eq!(spoken_numbers_to_digits("one hundred and fifty"), "150");
    }

    #[test]
    fn test_itn_two_thousand_and_twenty_four() {
        assert_eq!(
            spoken_numbers_to_digits("two thousand and twenty four"),
            "2024"
        );
    }

    #[test]
    fn test_itn_lone_digit_word() {
        assert_eq!(spoken_numbers_to_digits("five"), "5");
        assert_eq!(spoken_numbers_to_digits("zero"), "0");
    }

    #[test]
    fn test_itn_no_numbers_unchanged() {
        let s = "I went to the shops and bought some milk";
        assert_eq!(spoken_numbers_to_digits(s), s);
    }

    #[test]
    fn test_itn_mixed_sentence() {
        // "5" is already a digit — only the word run "twenty minutes" is converted.
        assert_eq!(
            spoken_numbers_to_digits("I ran 5 km in twenty minutes"),
            "I ran 5 km in 20 minutes"
        );
    }

    #[test]
    fn test_itn_three_million() {
        assert_eq!(spoken_numbers_to_digits("three million"), "3000000");
    }

    #[test]
    fn test_itn_in_sentence() {
        assert_eq!(
            spoken_numbers_to_digits("There were forty two people at the event"),
            "There were 42 people at the event"
        );
    }

    // ── Integration tests for new flags via OutputFilter ──────────────────

    #[test]
    fn test_filter_australian_spelling_only() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: false,
            normalise_whitespace: false,
            cleanup_punctuation: false,
            sentence_case: false,
            apply_dictionary: false,
            australian_spelling: true,
            spoken_numbers_to_digits: false,
        });
        assert_eq!(
            filter.filter("I love the color and flavor"),
            "I love the colour and flavour"
        );
    }

    #[test]
    fn test_filter_spoken_numbers_only() {
        let filter = OutputFilter::new(FilterOptions {
            remove_fillers: false,
            normalise_whitespace: false,
            cleanup_punctuation: false,
            sentence_case: false,
            apply_dictionary: false,
            australian_spelling: false,
            spoken_numbers_to_digits: true,
        });
        assert_eq!(
            filter.filter("I have twenty three items"),
            "I have 23 items"
        );
    }

    #[test]
    fn test_filter_new_flags_default_off() {
        let options = FilterOptions::default();
        assert!(!options.australian_spelling);
        assert!(!options.spoken_numbers_to_digits);
    }
}
