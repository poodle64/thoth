//! Corpus guard for the canonical snapper: it must never delete a word.
//!
//! The unit tests in `canonical.rs` assert single sentences. This runs the real
//! `apply_canonical` — against the registry actually installed on this machine —
//! over a body of real dictation and looks for the failure that motivated the
//! two-pass sweep: a snap consuming the word that followed the term, leaving its
//! contraction glued to the canonical spelling ("Portcullis. You'll need" →
//! "portcullis'll need").
//!
//! That artefact is unmistakable. "portcullis'm" and "godswood've" are not
//! English; a possessive is, so `'s` is not counted here.
//!
//! Point it at a corpus and run:
//! `THOTH_TEST_CORPUS=/path/to/transcripts.txt \
//!  cargo test --test canonical_corpus -- --ignored --nocapture`
//!
//! The corpus is deliberately NOT in the repo — it is a user's own dictation.
//! Export one from any Thoth history:
//! `sqlite3 ~/.thoth/thoth.db "SELECT text FROM transcriptions" > transcripts.txt`

use std::collections::BTreeMap;

/// Contractions that cannot follow a noun, so finding one welded to a canonical
/// term is proof a word was eaten rather than evidence of a possessive.
const IMPOSSIBLE_SUFFIXES: [&str; 5] = ["'m", "'ll", "'re", "'ve", "'t"];

/// Terms whose canonical spelling appears in the registry on this machine.
fn canonical_terms() -> Vec<String> {
    thoth_lib::canonical::get_canonical_terms()
        .expect("registry readable")
        .into_iter()
        .map(|t| t.term.to_lowercase())
        .collect()
}

/// Count `<term><suffix>` welds in `text`, keyed by the weld itself.
fn welds(text: &str, terms: &[String]) -> BTreeMap<String, usize> {
    let lower = text.to_lowercase();
    let mut found = BTreeMap::new();
    for term in terms {
        for suffix in IMPOSSIBLE_SUFFIXES {
            let needle = format!("{term}{suffix}");
            let n = lower.matches(&needle).count();
            if n > 0 {
                found.insert(needle, n);
            }
        }
    }
    found
}

#[test]
#[ignore = "needs THOTH_TEST_CORPUS pointing at a file of real transcripts"]
fn the_snapper_never_welds_a_contraction_onto_a_term() {
    let path = std::env::var("THOTH_TEST_CORPUS").expect(
        "THOTH_TEST_CORPUS is not set, so no corpus was read and nothing was \
         measured — this test proves nothing without one",
    );
    let corpus = std::fs::read_to_string(&path).expect("corpus is readable");

    let terms = canonical_terms();
    assert!(
        !terms.is_empty(),
        "no canonical terms are registered on this machine, so the sweep has \
         nothing to match and the corpus cannot exercise it"
    );

    // The corpus is already post-processed text, so it holds the correct forms
    // ("Portcullis. You'll"). Re-running the sweep over it is exactly the input
    // shape that used to be corrupted. A corpus written by an affected build
    // carries welds of its own, so the invariant is that the sweep adds none.
    let before = welds(&corpus, &terms);
    let after = welds(&thoth_lib::canonical::apply_canonical(&corpus), &terms);
    println!("welds already in the corpus: {before:?}");
    println!("welds after one more sweep:  {after:?}");

    let added: BTreeMap<_, _> = after
        .iter()
        .filter_map(|(weld, n)| {
            let was = before.get(weld).copied().unwrap_or(0);
            (*n > was).then(|| (weld.clone(), n - was))
        })
        .collect();
    assert!(
        added.is_empty(),
        "the sweep welded contractions onto canonical terms, deleting the word \
         before each: {added:?}"
    );
}
