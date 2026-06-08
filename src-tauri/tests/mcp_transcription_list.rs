//! Regression tests for the MCP `transcription` tool — issue #82.
//!
//! The `list` action once returned `[]` despite thousands of records, because
//! it called a query function that early-returns an empty list. It now goes
//! through `export::search_history`, the same path exercised here. These tests
//! drive the real database-backed functions the MCP actions call:
//!
//!   • `list`  → `export::search_history(...)`
//!   • `stats` → `database::transcription::get_transcription_stats_cmd()`
//!
//! and assert that `list` returns seeded records and that the `list` and
//! `stats` counts agree (the consistency the issue asked us to guard).
//!
//! The whole database layer talks to a process-global connection path, so the
//! suite runs in its own integration-test binary and points `THOTH_DATA_DIR`
//! at a throwaway directory. All assertions live in one test so the single
//! process-global path is set exactly once.

use tempfile::TempDir;
use thoth_lib::database::{self, Transcription};
use thoth_lib::export;

#[test]
fn list_returns_seeded_records_and_matches_stats() {
    // Point the database at a throwaway dir BEFORE any connection is opened.
    let temp = TempDir::new().expect("create temp dir");
    std::env::set_var("THOTH_DATA_DIR", temp.path());

    database::initialise_database().expect("initialise database");

    // Seed a handful of records, like the real pipeline would.
    const SEEDED: usize = 5;
    for i in 0..SEEDED {
        let record = Transcription::new(format!("regression record number {i}"));
        database::create_transcription(&record).expect("insert transcription");
    }

    // `list` path — must return the seeded records, not an empty array (#82).
    let listed = export::search_history(None, None, None, None, Some(100), Some(0))
        .expect("search_history (list) should succeed");
    assert_eq!(
        listed.records.len(),
        SEEDED,
        "list returned {} records, expected {SEEDED}",
        listed.records.len()
    );
    assert_eq!(
        listed.total_count as usize, SEEDED,
        "list total_count mismatch"
    );

    // `stats` path — counts must be consistent with `list`.
    let stats = database::transcription::get_transcription_stats_cmd()
        .expect("get_transcription_stats_cmd (stats) should succeed");
    assert_eq!(stats.total_count, SEEDED, "stats total_count mismatch");
    assert_eq!(
        stats.total_count, listed.total_count as usize,
        "list and stats counts disagree"
    );

    // Keep the temp dir alive until every query has run.
    drop(temp);
}
