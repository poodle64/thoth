//! Database integration tests for the Thoth transcription pipeline.
//!
//! Tests the save/load/delete operations on transcriptions using
//! a temporary database that is cleaned up after each test.

use rusqlite::Connection;
use tempfile::TempDir;

/// Helper to create an in-memory database with migrations applied.
fn create_test_database() -> Connection {
    let conn = Connection::open_in_memory().expect("Failed to create in-memory database");

    // Run migrations manually (schema setup)
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS transcriptions (
            id TEXT PRIMARY KEY,
            text TEXT NOT NULL,
            raw_text TEXT,
            duration_seconds REAL,
            created_at TEXT NOT NULL,
            audio_path TEXT,
            is_enhanced INTEGER NOT NULL DEFAULT 0,
            enhancement_prompt TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_transcriptions_created_at ON transcriptions(created_at);
        CREATE INDEX IF NOT EXISTS idx_transcriptions_is_enhanced ON transcriptions(is_enhanced);

        INSERT INTO migrations (version, name) VALUES (1, 'create_transcriptions_table');
        "#,
    )
    .expect("Failed to run migrations");

    conn
}

// =============================================================================
// Transcription CRUD Tests
// =============================================================================

#[test]
fn test_create_transcription() {
    let conn = create_test_database();

    // Insert a transcription
    conn.execute(
        r#"
        INSERT INTO transcriptions (id, text, raw_text, duration_seconds, created_at, audio_path, is_enhanced, enhancement_prompt)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        (
            "test-uuid-001",
            "Hello world, this is a test transcription.",
            None::<String>,
            Some(5.5_f64),
            "2025-01-15T10:30:00Z",
            None::<String>,
            0_i32,
            None::<String>,
        ),
    )
    .expect("Failed to insert transcription");

    // Verify it was inserted
    let count: i32 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .expect("Failed to count transcriptions");

    assert_eq!(count, 1);
}

#[test]
fn test_read_transcription_by_id() {
    let conn = create_test_database();

    // Insert a transcription
    conn.execute(
        r#"
        INSERT INTO transcriptions (id, text, raw_text, duration_seconds, created_at, audio_path, is_enhanced, enhancement_prompt)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        (
            "read-test-uuid",
            "Test read operation",
            Some("test read operation"),
            Some(3.2_f64),
            "2025-01-15T11:00:00Z",
            Some("/tmp/audio.wav"),
            1_i32,
            Some("grammar"),
        ),
    )
    .expect("Failed to insert transcription");

    // Read it back
    let (text, raw_text, duration, is_enhanced, prompt): (
        String,
        Option<String>,
        Option<f64>,
        i32,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT text, raw_text, duration_seconds, is_enhanced, enhancement_prompt FROM transcriptions WHERE id = ?1",
            ["read-test-uuid"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .expect("Failed to read transcription");

    assert_eq!(text, "Test read operation");
    assert_eq!(raw_text, Some("test read operation".to_string()));
    assert!((duration.unwrap() - 3.2).abs() < 0.001);
    assert_eq!(is_enhanced, 1);
    assert_eq!(prompt, Some("grammar".to_string()));
}

#[test]
fn test_read_nonexistent_transcription() {
    let conn = create_test_database();

    // Try to read a non-existent transcription
    let result = conn.query_row::<String, _, _>(
        "SELECT text FROM transcriptions WHERE id = ?1",
        ["nonexistent-uuid"],
        |row| row.get(0),
    );

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        rusqlite::Error::QueryReturnedNoRows
    ));
}

#[test]
fn test_update_transcription() {
    let conn = create_test_database();

    // Insert a transcription
    conn.execute(
        r#"
        INSERT INTO transcriptions (id, text, raw_text, duration_seconds, created_at, audio_path, is_enhanced, enhancement_prompt)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        (
            "update-test-uuid",
            "Original text",
            None::<String>,
            Some(2.0_f64),
            "2025-01-15T12:00:00Z",
            None::<String>,
            0_i32,
            None::<String>,
        ),
    )
    .expect("Failed to insert transcription");

    // Update it
    let rows_affected = conn
        .execute(
            r#"
            UPDATE transcriptions
            SET text = ?2, is_enhanced = ?3, enhancement_prompt = ?4, raw_text = ?5
            WHERE id = ?1
            "#,
            (
                "update-test-uuid",
                "Enhanced text with better grammar",
                1_i32,
                Some("grammar"),
                Some("Original text"),
            ),
        )
        .expect("Failed to update transcription");

    assert_eq!(rows_affected, 1);

    // Verify the update
    let (text, is_enhanced, raw_text): (String, i32, Option<String>) = conn
        .query_row(
            "SELECT text, is_enhanced, raw_text FROM transcriptions WHERE id = ?1",
            ["update-test-uuid"],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("Failed to read updated transcription");

    assert_eq!(text, "Enhanced text with better grammar");
    assert_eq!(is_enhanced, 1);
    assert_eq!(raw_text, Some("Original text".to_string()));
}

#[test]
fn test_delete_transcription() {
    let conn = create_test_database();

    // Insert a transcription
    conn.execute(
        r#"
        INSERT INTO transcriptions (id, text, created_at, is_enhanced)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        (
            "delete-test-uuid",
            "To be deleted",
            "2025-01-15T13:00:00Z",
            0_i32,
        ),
    )
    .expect("Failed to insert transcription");

    // Verify it exists
    let count_before: i32 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .expect("Failed to count");

    assert_eq!(count_before, 1);

    // Delete it
    let rows_affected = conn
        .execute(
            "DELETE FROM transcriptions WHERE id = ?1",
            ["delete-test-uuid"],
        )
        .expect("Failed to delete transcription");

    assert_eq!(rows_affected, 1);

    // Verify it's gone
    let count_after: i32 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .expect("Failed to count");

    assert_eq!(count_after, 0);
}

#[test]
fn test_delete_nonexistent_transcription() {
    let conn = create_test_database();

    // Try to delete a non-existent transcription
    let rows_affected = conn
        .execute("DELETE FROM transcriptions WHERE id = ?1", ["nonexistent"])
        .expect("Delete should not fail");

    assert_eq!(rows_affected, 0);
}

// =============================================================================
// List and Search Tests
// =============================================================================

#[test]
fn test_list_transcriptions_with_pagination() {
    let conn = create_test_database();

    // Insert multiple transcriptions
    for i in 0..10 {
        conn.execute(
            r#"
            INSERT INTO transcriptions (id, text, created_at, is_enhanced)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            (
                format!("list-test-{:02}", i),
                format!("Transcription number {}", i),
                format!("2025-01-15T{:02}:00:00Z", 10 + i),
                0_i32,
            ),
        )
        .expect("Failed to insert transcription");
    }

    // List with pagination (limit 5, offset 0)
    let mut stmt = conn
        .prepare("SELECT id FROM transcriptions ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")
        .expect("Failed to prepare statement");

    let ids: Vec<String> = stmt
        .query_map([5_i64, 0_i64], |row| row.get(0))
        .expect("Failed to query")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect");

    assert_eq!(ids.len(), 5);
    // Most recent first (created_at DESC)
    assert_eq!(ids[0], "list-test-09");

    // List with offset
    let ids_page2: Vec<String> = stmt
        .query_map([5_i64, 5_i64], |row| row.get(0))
        .expect("Failed to query")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect");

    assert_eq!(ids_page2.len(), 5);
    assert_eq!(ids_page2[0], "list-test-04");
}

#[test]
fn test_search_transcriptions_by_text() {
    let conn = create_test_database();

    // Insert transcriptions with different content
    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        (
            "search-1",
            "The quick brown fox jumps over the lazy dog",
            "2025-01-15T10:00:00Z",
            0_i32,
        ),
    )
    .expect("Failed to insert");

    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        (
            "search-2",
            "Hello world, how are you today?",
            "2025-01-15T11:00:00Z",
            0_i32,
        ),
    )
    .expect("Failed to insert");

    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        (
            "search-3",
            "Another fox was spotted in the garden",
            "2025-01-15T12:00:00Z",
            0_i32,
        ),
    )
    .expect("Failed to insert");

    // Search for "fox"
    let mut stmt = conn
        .prepare("SELECT id FROM transcriptions WHERE text LIKE ?1 ORDER BY created_at DESC")
        .expect("Failed to prepare");

    let results: Vec<String> = stmt
        .query_map(["%fox%"], |row| row.get(0))
        .expect("Failed to query")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect");

    assert_eq!(results.len(), 2);
    assert!(results.contains(&"search-1".to_string()));
    assert!(results.contains(&"search-3".to_string()));
}

#[test]
fn test_count_transcriptions() {
    let conn = create_test_database();

    // Insert some transcriptions
    for i in 0..5 {
        conn.execute(
            "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
            (
                format!("count-{}", i),
                format!("Text {}", i),
                "2025-01-15T10:00:00Z",
                0_i32,
            ),
        )
        .expect("Failed to insert");
    }

    // Count all
    let total: i32 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .expect("Failed to count");

    assert_eq!(total, 5);
}

// =============================================================================
// Edge Cases and Constraints
// =============================================================================

#[test]
fn test_transcription_with_special_characters() {
    let conn = create_test_database();

    let special_text = r#"This has "quotes", 'apostrophes', newlines
    and tabs	, plus emoji 🎉 and unicode: café résumé"#;

    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        ("special-chars", special_text, "2025-01-15T10:00:00Z", 0_i32),
    )
    .expect("Failed to insert special characters");

    let retrieved: String = conn
        .query_row(
            "SELECT text FROM transcriptions WHERE id = ?1",
            ["special-chars"],
            |row| row.get(0),
        )
        .expect("Failed to retrieve");

    assert_eq!(retrieved, special_text);
}

#[test]
fn test_transcription_with_empty_text() {
    let conn = create_test_database();

    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        ("empty-text", "", "2025-01-15T10:00:00Z", 0_i32),
    )
    .expect("Failed to insert empty text");

    let retrieved: String = conn
        .query_row(
            "SELECT text FROM transcriptions WHERE id = ?1",
            ["empty-text"],
            |row| row.get(0),
        )
        .expect("Failed to retrieve");

    assert_eq!(retrieved, "");
}

#[test]
fn test_transcription_with_very_long_text() {
    let conn = create_test_database();

    // Create a very long text (simulate a long transcription)
    let long_text = "This is a test sentence. ".repeat(1000);

    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        ("long-text", &long_text, "2025-01-15T10:00:00Z", 0_i32),
    )
    .expect("Failed to insert long text");

    let retrieved: String = conn
        .query_row(
            "SELECT text FROM transcriptions WHERE id = ?1",
            ["long-text"],
            |row| row.get(0),
        )
        .expect("Failed to retrieve");

    assert_eq!(retrieved.len(), long_text.len());
}

#[test]
fn test_duplicate_id_rejected() {
    let conn = create_test_database();

    // Insert first transcription
    conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        ("duplicate-id", "First", "2025-01-15T10:00:00Z", 0_i32),
    )
    .expect("Failed to insert first");

    // Try to insert duplicate
    let result = conn.execute(
        "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
        ("duplicate-id", "Second", "2025-01-15T11:00:00Z", 0_i32),
    );

    assert!(result.is_err());
}

// =============================================================================
// File-based Database Tests (using tempfile)
// =============================================================================

#[test]
fn test_database_persistence_to_file() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let db_path = temp_dir.path().join("test.db");

    // Create database and insert data
    {
        let conn = Connection::open(&db_path).expect("Failed to open database");
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS transcriptions (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                created_at TEXT NOT NULL,
                is_enhanced INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .expect("Failed to create table");

        conn.execute(
            "INSERT INTO transcriptions (id, text, created_at, is_enhanced) VALUES (?1, ?2, ?3, ?4)",
            ("persist-test", "Persisted text", "2025-01-15T10:00:00Z", 0_i32),
        )
        .expect("Failed to insert");
    }

    // Reopen and verify data persisted
    {
        let conn = Connection::open(&db_path).expect("Failed to reopen database");

        let text: String = conn
            .query_row(
                "SELECT text FROM transcriptions WHERE id = ?1",
                ["persist-test"],
                |row| row.get(0),
            )
            .expect("Failed to retrieve persisted data");

        assert_eq!(text, "Persisted text");
    }
}
