//! Drives escape detection against the history on this machine.
//!
//! `suggest_aliases` is only worth anything if, pointed at real dictation, it
//! surfaces the mis-hearings a person would have found by reading. This runs the
//! production entry point — this machine's own registry, this machine's own
//! transcriptions — and prints what it found, ranked.
//!
//! Read-only: it SELECTs from the history and reads the registry.
//!
//! `cargo test --test canonical_suggest -- --ignored --nocapture`

#[test]
#[ignore = "reads the transcription history on this machine"]
fn escaping_spellings_are_found_in_real_history() {
    let suggestions = thoth_lib::canonical::suggest_aliases_from_history(20_000, 2)
        .expect("history and registry are readable");

    println!("{} candidate spellings\n", suggestions.len());
    for s in &suggestions {
        println!(
            "{:>5}x  {:<20} -> {:<16} (term seen {:>5}x, similarity {:.2})",
            s.occurrences, s.spelling, s.term, s.term_occurrences, s.similarity
        );
        println!("        {}", s.context);
    }

    // Whatever this machine's history holds, these must hold of every row.
    let registered: std::collections::HashSet<String> = thoth_lib::canonical::get_canonical_terms()
        .expect("registry readable")
        .into_iter()
        .flat_map(|t| {
            std::iter::once(t.term.to_lowercase())
                .chain(t.aliases.into_iter().map(|a| a.to_lowercase()))
        })
        .collect();

    for s in &suggestions {
        assert!(
            !registered.contains(&s.spelling),
            "{:?} is already registered, so proposing it is noise",
            s.spelling
        );
        assert!(
            s.occurrences <= s.term_occurrences,
            "{:?} occurs more often than {:?} itself, so it is a word, not a \
             mis-hearing of one",
            s.spelling,
            s.term
        );
        assert!(
            !s.context.is_empty(),
            "{:?} came back without the context a reviewer needs to judge it",
            s.spelling
        );
    }

    let ranked: Vec<usize> = suggestions.iter().map(|s| s.occurrences).collect();
    let mut sorted = ranked.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(
        ranked, sorted,
        "the list must lead with the strongest evidence"
    );
}
