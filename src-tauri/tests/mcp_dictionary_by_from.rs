//! Regression tests for addressing a dictionary entry by its `from` text.
//!
//! The MCP `dictionary` tool could only `update`/`delete` by positional index,
//! with no index reported by `list`. On a three-hundred-entry dictionary that
//! made deletion unsafe for an agent: an index counted out by eye that lands one
//! row off deletes a DIFFERENT, working entry, silently. So the assertions that
//! matter here are not "the target went" but "the neighbours stayed", and
//! "an ambiguous key is refused rather than resolved to one of its candidates".
//!
//! These drive the real core functions `mcp_server/mod.rs` calls for the
//! by-`from` path:
//!
//!   • `delete` → `dictionary::remove_dictionary_entry_by_from(...)`
//!   • `update` → `dictionary::update_dictionary_entry_by_from(...)`
//!
//! The dictionary lives behind a process-global `OnceLock` loaded once from one
//! path, so this runs in its own integration-test binary with `THOTH_DATA_DIR`
//! pointed at a throwaway directory, and every assertion lives in one test. The
//! user's real `~/.thoth/dictionary.json` is never touched.
//!
//! The fixture is seeded by writing the file rather than by calling
//! `add_dictionary_entry`, because it must contain a DUPLICATE `from` — which
//! `add` rightly refuses. Duplicates still reach a real dictionary by hand edit
//! or by a replace-import carrying two copies, which is what makes "refuse when
//! ambiguous" a live requirement rather than a theoretical one.

use tempfile::TempDir;
use thoth_lib::dictionary::{
    get_dictionary_entries, remove_dictionary_entry_by_from, update_dictionary_entry_by_from,
};

fn from_values() -> Vec<String> {
    get_dictionary_entries()
        .expect("read dictionary")
        .into_iter()
        .map(|e| e.from)
        .collect()
}

fn pairs() -> Vec<(String, String)> {
    get_dictionary_entries()
        .expect("read dictionary")
        .into_iter()
        .map(|e| (e.from, e.to))
        .collect()
}

#[test]
fn by_from_hits_only_the_named_entry_and_refuses_when_ambiguous() {
    let temp = TempDir::new().expect("create temp dir");
    std::fs::write(
        temp.path().join("dictionary.json"),
        r#"{"entries":[
             {"from":"neighbour-a","to":"Neighbour A","caseSensitive":false},
             {"from":"teh","to":"the","caseSensitive":false},
             {"from":"doomed","to":"Doomed","caseSensitive":false},
             {"from":"TEH","to":"THE","caseSensitive":true},
             {"from":"neighbour-d","to":"Neighbour D","caseSensitive":true}
           ]}"#,
    )
    .expect("write seeded dictionary");

    // Point the dictionary at the throwaway dir BEFORE the global is first read.
    // SAFETY: single-threaded test binary; no other threads read this var.
    unsafe { std::env::set_var("THOTH_DATA_DIR", temp.path()) };
    assert_eq!(from_values().len(), 5, "seeded file did not load");

    // ---- an ambiguous key is refused, not resolved to one candidate --------
    let err = remove_dictionary_entry_by_from("teh").expect_err("ambiguous delete must fail");
    let msg = err.to_string();
    assert!(msg.contains("refusing to guess"), "unhelpful error: {msg}");
    // The error names both candidates so the caller can pick one by index.
    assert!(
        msg.contains('1') && msg.contains('3'),
        "candidate indices missing: {msg}"
    );
    // Refusing means refusing: nothing went on the way out.
    assert_eq!(
        from_values().len(),
        5,
        "an ambiguous delete removed an entry"
    );

    // Update is refused on the same terms, and changes nothing.
    update_dictionary_entry_by_from("teh", "whatever".to_string(), None)
        .expect_err("ambiguous update must fail");
    assert_eq!(
        from_values().len(),
        5,
        "an ambiguous update changed the file"
    );

    // ---- a key that matches nothing is refused too --------------------------
    let err = remove_dictionary_entry_by_from("never-existed")
        .expect_err("deleting an absent key must fail");
    assert!(
        err.to_string().contains("No dictionary entry"),
        "unhelpful error: {err}"
    );

    // A near-miss must not resolve either: "neighbour" is a prefix of two
    // entries and the exact `from` of none.
    remove_dictionary_entry_by_from("neighbour").expect_err("a prefix must not resolve");
    assert_eq!(from_values().len(), 5, "a failed delete removed something");

    // ---- delete by `from` ---------------------------------------------------
    // "doomed" sits at index 2, between the two ambiguous entries: an off-by-one
    // index would take one of them instead.
    let removed_at = remove_dictionary_entry_by_from("doomed").expect("delete by from");
    assert_eq!(removed_at, 2, "reported the wrong index");

    // The named entry is gone, and — the actual risk being closed — every other
    // entry survives, in its original order.
    assert_eq!(
        from_values(),
        vec!["neighbour-a", "teh", "TEH", "neighbour-d"],
        "a neighbour was disturbed"
    );

    // The ambiguity report tracks the entries' new positions.
    let msg = remove_dictionary_entry_by_from("teh")
        .expect_err("still ambiguous")
        .to_string();
    assert!(
        msg.contains('1') && msg.contains('2'),
        "stale candidate indices: {msg}"
    );

    // ---- update by `from` changes only what it names ------------------------
    let updated_at = update_dictionary_entry_by_from("neighbour-a", "Rewritten".to_string(), None)
        .expect("update by from");
    assert_eq!(updated_at, 0, "reported the wrong index");
    assert_eq!(
        pairs(),
        vec![
            ("neighbour-a".to_string(), "Rewritten".to_string()),
            ("teh".to_string(), "the".to_string()),
            ("TEH".to_string(), "THE".to_string()),
            ("neighbour-d".to_string(), "Neighbour D".to_string()),
        ],
        "update touched an entry it was not given"
    );

    // Matching is case-insensitive, but the stored `from` is kept verbatim (a
    // caller's casing must not silently rewrite the entry it addressed), and
    // omitting caseSensitive keeps the entry's existing flag rather than
    // resetting it — `neighbour-d` was seeded case-sensitive and must stay so.
    update_dictionary_entry_by_from("NEIGHBOUR-D", "Also rewritten".to_string(), None)
        .expect("update by from, different casing");
    let after = get_dictionary_entries().expect("read dictionary");
    assert_eq!(
        after[3].from, "neighbour-d",
        "casing of `from` was rewritten"
    );
    assert_eq!(after[3].to, "Also rewritten");
    assert!(
        after[3].case_sensitive,
        "caseSensitive was reset by omission"
    );

    // An explicit caseSensitive still takes effect.
    update_dictionary_entry_by_from("neighbour-d", "Also rewritten".to_string(), Some(false))
        .expect("update by from, explicit flag");
    assert!(
        !get_dictionary_entries().expect("read dictionary")[3].case_sensitive,
        "an explicit caseSensitive was ignored"
    );
}
