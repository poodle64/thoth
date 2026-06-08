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

        // Collapse decoder-loop repetition before anything else, so the dedup
        // sees the raw transducer output (including duplicated words at chunk
        // overlaps). Linux/Parakeet only — the macOS path never loops this way,
        // and we don't want to risk collapsing legitimate repetition there.
        #[cfg(target_os = "linux")]
        {
            result = remove_repeated_phrases(&result);
        }

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

/// Remove repeated phrases caused by transducer decoder loops.
///
/// Sherpa-ONNX's NeMo transducer (Parakeet) — like Whisper's autoregressive
/// decoder — can get stuck repeating the last few sentences on longer audio,
/// and the chunked Linux transcription path can duplicate a word or two where
/// adjacent chunks overlap. This collapses both:
///
/// 1. runs of consecutive identical sentences, and
/// 2. sub-sentence n-gram repetition (e.g. "to matter to matter to matter").
///
/// Linux/Parakeet only — see the call site in [`OutputFilter::filter`].
#[cfg(target_os = "linux")]
pub fn remove_repeated_phrases(text: &str) -> String {
    if text.len() < 20 {
        return text.to_string();
    }

    // Phase 1: sentence-level dedup — split on sentence boundaries and
    // collapse consecutive identical sentences.
    let sentence_result = dedup_consecutive_sentences(text);

    // Phase 2: sub-sentence n-gram dedup — catches "to matter to matter to matter"
    // style repetition that doesn't align on sentence boundaries.
    dedup_inline_ngrams(&sentence_result)
}

/// Split text into sentences, collapse consecutive duplicates.
#[cfg(target_os = "linux")]
fn dedup_consecutive_sentences(text: &str) -> String {
    static SENTENCE_BOUNDARY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"([.!?])\s+").unwrap());

    // Split while preserving the terminal punctuation on each sentence.
    let mut sentences: Vec<String> = Vec::new();
    let mut last_end = 0;
    for m in SENTENCE_BOUNDARY.find_iter(text) {
        let punct_end = m.start() + 1; // include the punctuation char
        sentences.push(text[last_end..punct_end].to_string());
        last_end = m.end();
    }
    if last_end < text.len() {
        sentences.push(text[last_end..].to_string());
    }

    if sentences.len() < 2 {
        return text.to_string();
    }

    let mut result = Vec::with_capacity(sentences.len());
    let mut prev_normalised = String::new();

    for sentence in &sentences {
        let normalised = sentence.trim().to_lowercase();
        if normalised == prev_normalised && !normalised.is_empty() {
            continue;
        }
        prev_normalised = normalised;
        result.push(sentence.as_str());
    }

    if result.len() < sentences.len() {
        tracing::debug!(
            "Repetition filter: collapsed {} repeated sentences",
            sentences.len() - result.len()
        );
    }

    result.join(" ")
}

/// Detect and remove inline n-gram repetition.
///
/// Finds a repeating word-sequence (2–12 words) that appears 3+ times
/// consecutively and collapses it to a single occurrence.
#[cfg(target_os = "linux")]
fn dedup_inline_ngrams(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 9 {
        return text.to_string();
    }

    let mut result_words = words.clone();
    let mut changed = false;

    for n in 2..=12 {
        if result_words.len() < n * 3 {
            break;
        }
        let mut i = 0;
        let mut new_words: Vec<&str> = Vec::with_capacity(result_words.len());
        while i < result_words.len() {
            if i + n * 3 <= result_words.len() {
                let chunk: Vec<String> = result_words[i..i + n]
                    .iter()
                    .map(|w| w.to_lowercase())
                    .collect();
                let mut reps = 1usize;
                while i + n * (reps + 1) <= result_words.len() {
                    let next: Vec<String> = result_words[i + n * reps..i + n * (reps + 1)]
                        .iter()
                        .map(|w| w.to_lowercase())
                        .collect();
                    if next == chunk {
                        reps += 1;
                    } else {
                        break;
                    }
                }
                if reps >= 3 {
                    new_words.extend_from_slice(&result_words[i..i + n]);
                    i += n * reps;
                    changed = true;
                    continue;
                }
            }
            new_words.push(result_words[i]);
            i += 1;
        }
        result_words = new_words;
    }

    if changed {
        tracing::debug!("Repetition filter: removed inline n-gram repetitions");
        result_words.join(" ")
    } else {
        text.to_string()
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

// ── Australian/British spelling normalisation ─────────────────────────────
//
// US → AU spelling is a whole-word lookup against a map generated from VARCON
// (the English Speller Database), the canonical dialect-variant dataset that
// also generates the en_AU Hunspell dictionary shipped in browsers and office
// suites. The generated table lives in `au_spelling_map.rs`
// (`scripts/generate_au_spelling.py` rebuilds it from `data/varcon/varcon.txt`).
//
// A lookup approach is what every mature tool uses: the -ize/-ise split has too
// many false friends (size, capsize, seize, prize) and the -our/-re families
// too many non-members (doctor, motor, water) for suffix rules to be safe. The
// generated map already excludes homograph hazards and the rare/archaic tail.
//
// Word splitting keeps everything that is not a run of ASCII letters verbatim,
// so punctuation, digits and whitespace are untouched. Case is restored after.

use super::au_spelling_map::AU_SPELLING_MAP;

/// Look up the Australian spelling for a lowercase ASCII word, if one differs.
fn lookup_au_word(lower: &str) -> Option<&'static str> {
    AU_SPELLING_MAP
        .binary_search_by(|(us, _)| (*us).cmp(lower))
        .ok()
        .map(|idx| AU_SPELLING_MAP[idx].1)
}

/// Restore the casing of `original` onto `converted`.
///
/// Handles the three cases dictation produces: all-lower, Capitalised, and
/// ALL-CAPS. Mixed/other casings fall back to matching the leading capital.
fn restore_case(original: &str, converted: &str) -> String {
    let all_upper =
        original.chars().all(|c| !c.is_lowercase()) && original.chars().any(|c| c.is_uppercase());
    if all_upper {
        return converted.to_uppercase();
    }
    if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut out = converted.to_string();
        if let Some(first) = out.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        return out;
    }
    converted.to_string()
}

/// Apply Australian/British spelling normalisation across arbitrary text.
///
/// Splits on runs of ASCII letters (everything else — spaces, digits,
/// punctuation — is emitted verbatim), looks each word up in the VARCON-derived
/// map, and restores the original word's capitalisation on any replacement.
pub fn apply_australian_spelling(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word_start: Option<usize> = None;

    let flush = |out: &mut String, word: &str| {
        let lower = word.to_ascii_lowercase();
        match lookup_au_word(&lower) {
            Some(au) => out.push_str(&restore_case(word, au)),
            None => out.push_str(word), // unchanged — preserve original casing exactly
        }
    };

    for (idx, ch) in text.char_indices() {
        if ch.is_ascii_alphabetic() {
            if word_start.is_none() {
                word_start = Some(idx);
            }
        } else if let Some(start) = word_start.take() {
            flush(&mut out, &text[start..idx]);
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    if let Some(start) = word_start {
        flush(&mut out, &text[start..]);
    }

    out
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

/// The single digit (0-9) a word denotes when read out digit-by-digit, if any.
///
/// Covers the spoken-digit vocabulary: "zero".."nine" plus "oh" (the spoken
/// form of 0 in codes and phone numbers, e.g. "four oh four"). "oh" maps to a
/// digit ONLY here — it is deliberately NOT a general number token, so the
/// interjection in "oh, I see" is never treated as a number.
fn word_to_single_digit(word: &str) -> Option<u8> {
    match word.to_ascii_lowercase().as_str() {
        "zero" | "oh" => Some(0),
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        _ => None,
    }
}

/// Determiners that, immediately before a lone "one", mark it as the pronoun
/// ("no one", "any one", "every one", "each one") rather than the numeral 1.
fn is_oneness_determiner(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "no" | "any" | "every" | "each" | "some"
    )
}

/// Read a run of number-word cores as a digit-by-digit sequence, if it is one.
///
/// A run qualifies only when it is **two or more** bare single-digit words and
/// nothing else — no teens, tens, magnitudes ("hundred"/"thousand"), or "and".
/// Such a run is how people dictate PINs, codes and phone numbers: "one two
/// three" means the string "123", not the sum 1+2+3. Any run containing a
/// larger number word is a cardinal phrase ("twenty three" → 23, "two hundred"
/// → 200) and is handled by `tokens_to_number` instead.
///
/// Returns the concatenated digit string (e.g. "123"), or `None` if the run is
/// not a pure multi-digit sequence.
fn digit_sequence(cores: &[&str]) -> Option<String> {
    if cores.len() < 2 {
        return None;
    }
    let mut digits = String::with_capacity(cores.len());
    for core in cores {
        let d = word_to_single_digit(core)?;
        digits.push((b'0' + d) as char);
    }
    Some(digits)
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

    // Expand hyphenated number compounds into individual sub-tokens so the
    // run-scanner below treats them the same as space-separated words.
    //
    // A token is expanded only when every hyphen-separated piece (after
    // stripping leading/trailing punctuation from the whole token) is itself a
    // recognised number word. This is conservative by design:
    //   "twenty-three"    → ["twenty", "three"]   ✓ (all pieces are number words)
    //   "well-known"      → kept intact            ✗ ("well" is not a number word)
    //   "twenty-something"→ kept intact            ✗ ("something" is not a number word)
    //   "x-ray"           → kept intact            ✗ ("x" and "ray" are not number words)
    // The leading punctuation of the first piece and trailing punctuation of the
    // last piece are preserved; the hyphen itself is discarded.
    //
    // Two-phase approach to satisfy the borrow checker:
    //   Phase 1 — classify each token; tokens that expand produce owned Strings
    //             stored in `owned_pieces`; everything else records its original
    //             slice indices for phase 2.
    //   Phase 2 — build the final `&str` slice pairs from either the original
    //             `text`-backed slices or the owned Strings.
    //
    // An enum avoids any mutation of `owned_pieces` while borrowing from it.
    enum TokenKind<'t> {
        // Original whitespace-delimited token — borrow directly from `text`.
        Original((&'t str, &'t str)),
        // Hyphenated number compound — index range into `owned_pieces`; the
        // first sub-token carries the original preceding whitespace.
        Expanded {
            ws: &'t str,
            range: std::ops::Range<usize>,
        },
    }

    let mut owned_pieces: Vec<String> = Vec::new();
    let mut kinds: Vec<TokenKind> = Vec::with_capacity(tokens.len());

    for (ws, word) in &tokens {
        let (ws, word) = (*ws, *word);

        // Fast path: no hyphen means nothing to expand.
        if !word.contains('-') {
            kinds.push(TokenKind::Original((ws, word)));
            continue;
        }

        let core = alpha_core(word);
        if core.is_empty() || !core.contains('-') {
            kinds.push(TokenKind::Original((ws, word)));
            continue;
        }

        // Split the alphabetic core on hyphens and check every piece.
        let pieces: Vec<&str> = core.split('-').collect();
        let all_number_words = pieces.iter().all(|p| {
            !p.is_empty() && (word_to_value(p).is_some() || word_to_multiplier(p).is_some())
        });

        if !all_number_words {
            kinds.push(TokenKind::Original((ws, word)));
            continue;
        }

        // Locate where the core sits inside the original word so we can
        // extract any leading/trailing punctuation envelope.
        let core_start = word.find(core).unwrap_or(0);
        let core_end = core_start + core.len();
        let leading_punct = &word[..core_start];
        let trailing_punct = &word[core_end..];

        let n_pieces = pieces.len();
        let base_idx = owned_pieces.len();

        for (idx, piece) in pieces.iter().enumerate() {
            // Build a synthetic token with punctuation only on the first/last piece.
            let synthetic = if idx == 0 && idx == n_pieces - 1 {
                // Single piece — cannot happen since we checked for '-' above.
                format!("{leading_punct}{piece}{trailing_punct}")
            } else if idx == 0 {
                format!("{leading_punct}{piece}")
            } else if idx == n_pieces - 1 {
                format!("{piece}{trailing_punct}")
            } else {
                piece.to_string()
            };
            owned_pieces.push(synthetic);
        }

        kinds.push(TokenKind::Expanded {
            ws,
            range: base_idx..owned_pieces.len(),
        });
    }

    // Phase 2: owned_pieces is fully built; borrow from it freely.
    let mut expanded_tokens: Vec<(&str, &str)> = Vec::with_capacity(kinds.len());
    for kind in &kinds {
        match kind {
            TokenKind::Original((ws, word)) => {
                expanded_tokens.push((ws, word));
            }
            TokenKind::Expanded { ws, range } => {
                for (idx, owned) in owned_pieces[range.clone()].iter().enumerate() {
                    let preceding = if idx == 0 { *ws } else { "" };
                    expanded_tokens.push((preceding, owned.as_str()));
                }
            }
        }
    }

    let tokens = expanded_tokens;

    let n = tokens.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < n {
        let (preceding_ws, word) = tokens[i];

        let core = alpha_core(word);

        // A token starts a number run if it is a number token, or a spoken digit
        // such as "oh" (which is not a general number token but does belong in a
        // digit sequence like "four oh four").
        let starts_run =
            !core.is_empty() && (is_number_token(core) || word_to_single_digit(core).is_some());

        if starts_run {
            // Identify the full run of consecutive number tokens.
            // "and" is included only when it is followed by another number word,
            // preventing a trailing "and" from being swallowed.
            let run_start = i;
            let mut run_end = i;

            let mut j = i;
            while j < n {
                let (_, w) = tokens[j];
                let c = alpha_core(w);
                let in_run =
                    !c.is_empty() && (is_number_token(c) || word_to_single_digit(c).is_some());
                if !in_run {
                    break;
                }
                if c.eq_ignore_ascii_case("and") {
                    let next_is_number = j + 1 < n && {
                        let (_, nw) = tokens[j + 1];
                        let nc = alpha_core(nw);
                        !nc.is_empty()
                            && (is_number_token(nc) || word_to_single_digit(nc).is_some())
                            && !nc.eq_ignore_ascii_case("and")
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

            // Guard: a lone "one" is the pronoun, not the numeral, after a
            // determiner like "no"/"any"/"every"/"each" ("no one", "any one").
            // Converting it to "1" ("no 1") is wrong. Only suppress the lone
            // single-word "one"; "one hundred", "one two three", or "one" after
            // a non-determiner ("I need one") still convert.
            let is_lone_pronoun_one = run_cores.len() == 1
                && run_cores[0].eq_ignore_ascii_case("one")
                && run_start > 0
                && is_oneness_determiner(alpha_core(tokens[run_start - 1].1));

            // A pure run of two or more single digits is read digit-by-digit
            // ("one two three" → "123"); everything else is a cardinal number
            // ("twenty three" → 23, "two hundred" → 200). digit_sequence returns
            // None for the cardinal case, falling through to tokens_to_number.
            let parsed: Option<String> = if is_lone_pronoun_one {
                None
            } else {
                digit_sequence(&run_cores)
                    .or_else(|| tokens_to_number(&run_cores).map(|n| n.to_string()))
            };

            if let Some(number) = parsed {
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

                out.push_str(&number);

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
        parts.extend(std::iter::repeat_n("more", 10));
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
        // VARCON carries prefixed forms, so "discolored" → "discoloured".
        assert_eq!(apply_australian_spelling("discolored"), "discoloured");
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

    #[test]
    fn test_au_spelling_ize_rule_covers_unlisted_words() {
        // The whole -ize family converts by rule, including words not in any explicit list.
        assert_eq!(apply_australian_spelling("realize"), "realise");
        assert_eq!(apply_australian_spelling("realized"), "realised");
        assert_eq!(apply_australian_spelling("realizing"), "realising");
        assert_eq!(apply_australian_spelling("realization"), "realisation");
        assert_eq!(
            apply_australian_spelling("institutionalize"),
            "institutionalise"
        );
        assert_eq!(
            apply_australian_spelling("institutionalized"),
            "institutionalised"
        );
        assert_eq!(apply_australian_spelling("modernize"), "modernise");
        assert_eq!(apply_australian_spelling("hospitalize"), "hospitalise");
        assert_eq!(apply_australian_spelling("itemize"), "itemise");
    }

    #[test]
    fn test_au_spelling_yze_rule() {
        assert_eq!(apply_australian_spelling("analyze"), "analyse");
        assert_eq!(apply_australian_spelling("analyzed"), "analysed");
        assert_eq!(apply_australian_spelling("paralyze"), "paralyse");
        assert_eq!(apply_australian_spelling("catalyzing"), "catalysing");
    }

    #[test]
    fn test_au_spelling_ize_false_friends_unchanged() {
        // Words where -ize/-ise letters are part of the stem must NOT be touched.
        for w in [
            "size", "sized", "sizing", "resize", "downsize", "capsize", "prize", "prized", "maize",
            "seize", "seized",
        ] {
            assert_eq!(apply_australian_spelling(w), w, "{w} should be unchanged");
        }
    }

    #[test]
    fn test_au_spelling_our_does_not_overreach() {
        // -or words that are NOT -our words must pass through untouched — the
        // failure mode a blanket -or→-our rule would cause.
        for w in [
            "doctor", "motor", "actor", "error", "mirror", "factor", "tractor", "author", "razor",
            "mentor", "vendor",
        ] {
            assert_eq!(apply_australian_spelling(w), w, "{w} should be unchanged");
        }
    }

    #[test]
    fn test_au_spelling_our_family_inflections() {
        assert_eq!(apply_australian_spelling("favorite"), "favourite");
        assert_eq!(apply_australian_spelling("behavior"), "behaviour");
        assert_eq!(apply_australian_spelling("neighbors"), "neighbours");
        assert_eq!(apply_australian_spelling("labored"), "laboured");
    }

    #[test]
    fn test_au_spelling_all_caps_preserved() {
        assert_eq!(apply_australian_spelling("COLOR"), "COLOUR");
        assert_eq!(apply_australian_spelling("REALIZE"), "REALISE");
    }

    #[test]
    fn test_au_spelling_punctuation_and_digits_untouched() {
        assert_eq!(
            apply_australian_spelling("color, flavor; honor!"),
            "colour, flavour; honour!"
        );
        assert_eq!(
            apply_australian_spelling("organize 5 colors"),
            "organise 5 colours"
        );
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

    // ── Digit-sequence ITN tests ──────────────────────────────────────────
    // A run of bare single-digit words is read digit-by-digit (PIN/code/phone), not summed.

    #[test]
    fn test_itn_digit_sequence_basic() {
        assert_eq!(spoken_numbers_to_digits("one two three"), "123");
        assert_eq!(spoken_numbers_to_digits("four five"), "45");
        assert_eq!(
            spoken_numbers_to_digits("nine eight seven six five"),
            "98765"
        );
    }

    #[test]
    fn test_itn_digit_sequence_with_zero_and_oh() {
        // "oh" is the spoken zero in codes ("four oh four" → 404).
        assert_eq!(spoken_numbers_to_digits("four oh four"), "404");
        assert_eq!(spoken_numbers_to_digits("one zero one"), "101");
    }

    #[test]
    fn test_itn_lone_oh_is_not_a_number() {
        // A single "oh" (or the interjection) must NOT become "0": digit
        // sequences require two or more single-digit words.
        assert_eq!(spoken_numbers_to_digits("oh"), "oh");
        assert_eq!(spoken_numbers_to_digits("oh well"), "oh well");
    }

    #[test]
    fn test_itn_cardinal_not_treated_as_digit_sequence() {
        // Any run containing a teen/ten/magnitude is cardinal, not concatenated.
        assert_eq!(spoken_numbers_to_digits("twenty three"), "23");
        assert_eq!(spoken_numbers_to_digits("two hundred"), "200");
        assert_eq!(spoken_numbers_to_digits("one hundred and two"), "102");
    }

    #[test]
    fn test_itn_digit_sequence_in_sentence() {
        assert_eq!(
            spoken_numbers_to_digits("my code is one two three four"),
            "my code is 1234"
        );
    }

    #[test]
    fn test_itn_fifteen_hundred_and_three() {
        // "fifteen hundred" = 15 × 100; "and three" adds 3 → 1503.
        assert_eq!(
            spoken_numbers_to_digits("fifteen hundred and three"),
            "1503"
        );
    }

    #[test]
    fn test_itn_eight_hundred_and_ninety_six() {
        assert_eq!(
            spoken_numbers_to_digits("eight hundred and ninety six"),
            "896"
        );
    }

    #[test]
    fn test_itn_lone_one_after_determiner_is_pronoun() {
        // "one" as a pronoun after a determiner must NOT become "1".
        assert_eq!(spoken_numbers_to_digits("no one"), "no one");
        assert_eq!(spoken_numbers_to_digits("no one came"), "no one came");
        assert_eq!(
            spoken_numbers_to_digits("any one of them"),
            "any one of them"
        );
        assert_eq!(spoken_numbers_to_digits("every one"), "every one");
        assert_eq!(spoken_numbers_to_digits("each one"), "each one");
    }

    #[test]
    fn test_itn_one_still_converts_when_numeral() {
        // A real numeral "one" still converts: not after a oneness-determiner,
        // or part of a larger number.
        assert_eq!(spoken_numbers_to_digits("I need one"), "I need 1");
        assert_eq!(spoken_numbers_to_digits("one hundred"), "100");
        assert_eq!(spoken_numbers_to_digits("one two three"), "123");
        // "the one" — "the" is not in the oneness set, so it still converts;
        // this is acceptable (rare, and ambiguous either way).
        assert_eq!(
            spoken_numbers_to_digits("give me one please"),
            "give me 1 please"
        );
    }

    // ── Hyphenated number compound tests ──────────────────────────────────
    // Parakeet/FluidAudio emits compounds like "twenty-three" as a single
    // whitespace-delimited token. The expansion step splits them before the
    // number-run scanner so they convert identically to space-separated forms.

    #[test]
    fn test_itn_hyphenated_twenty_three() {
        assert_eq!(spoken_numbers_to_digits("twenty-three"), "23");
    }

    #[test]
    fn test_itn_hyphenated_forty_two_in_sentence() {
        assert_eq!(spoken_numbers_to_digits("forty-two items"), "42 items");
    }

    #[test]
    fn test_itn_hyphenated_ninety_nine() {
        assert_eq!(spoken_numbers_to_digits("ninety-nine"), "99");
    }

    #[test]
    fn test_itn_hyphenated_trailing_punctuation() {
        // Trailing punctuation on the compound token must survive on the digit.
        assert_eq!(
            spoken_numbers_to_digits("I have twenty-three apples."),
            "I have 23 apples."
        );
    }

    #[test]
    fn test_itn_hyphenated_non_number_unchanged() {
        // Neither "well" nor "known" is a number word — token must pass through intact.
        assert_eq!(spoken_numbers_to_digits("well-known"), "well-known");
    }

    #[test]
    fn test_itn_hyphenated_x_ray_unchanged() {
        assert_eq!(spoken_numbers_to_digits("x-ray"), "x-ray");
    }

    #[test]
    fn test_itn_hyphenated_mixed_with_space_separated() {
        // Hyphenated compound and space-separated run must both convert when
        // they appear in separate (non-adjacent) number runs within a sentence.
        assert_eq!(
            spoken_numbers_to_digits("twenty-three items costing one hundred dollars"),
            "23 items costing 100 dollars"
        );
    }

    #[test]
    fn test_itn_hyphenated_twenty_something_unchanged() {
        // "something" is not a number word, so the conservative rule leaves the
        // whole token intact rather than converting only the "twenty" piece.
        // This avoids surprising output like "20-something".
        assert_eq!(
            spoken_numbers_to_digits("twenty-something"),
            "twenty-something"
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

    // Repetition filter (Linux/Parakeet only) ---------------------------------

    #[cfg(target_os = "linux")]
    #[test]
    fn test_dedup_consecutive_sentences() {
        let input = "The meeting is at noon. The meeting is at noon. See you there.";
        assert_eq!(
            remove_repeated_phrases(input),
            "The meeting is at noon. See you there."
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_dedup_inline_ngram_repetition() {
        let input = "it seems to matter to matter to matter to matter in the end";
        assert_eq!(
            remove_repeated_phrases(input),
            "it seems to matter in the end"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_repetition_filter_leaves_clean_text_untouched() {
        let input = "This is a perfectly normal sentence with no repetition at all.";
        assert_eq!(remove_repeated_phrases(input), input);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_repetition_filter_keeps_short_text() {
        // Below the 20-char floor — returned unchanged.
        assert_eq!(remove_repeated_phrases("hi hi hi"), "hi hi hi");
    }
}
