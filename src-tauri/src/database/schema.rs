//! Database schema definitions for Thoth.
//!
//! Contains SQL statements for creating and managing database tables.

/// SQL statement to create the migrations tracking table.
pub const CREATE_MIGRATIONS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);
"#;

/// SQL statement to create the transcriptions table.
pub const CREATE_TRANSCRIPTIONS_TABLE: &str = r#"
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
"#;

/// SQL statement to create an index on created_at for efficient time-based queries.
pub const CREATE_TRANSCRIPTIONS_CREATED_AT_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_transcriptions_created_at ON transcriptions(created_at);
"#;

/// SQL statement to create an index on is_enhanced for filtering.
pub const CREATE_TRANSCRIPTIONS_IS_ENHANCED_INDEX: &str = r#"
CREATE INDEX IF NOT EXISTS idx_transcriptions_is_enhanced ON transcriptions(is_enhanced);
"#;

/// SQL statements to add metadata columns (v2 migration).
pub const ALTER_ADD_TRANSCRIPTION_MODEL_NAME: &str =
    "ALTER TABLE transcriptions ADD COLUMN transcription_model_name TEXT;";

pub const ALTER_ADD_TRANSCRIPTION_DURATION: &str =
    "ALTER TABLE transcriptions ADD COLUMN transcription_duration_seconds REAL;";

pub const ALTER_ADD_ENHANCEMENT_MODEL_NAME: &str =
    "ALTER TABLE transcriptions ADD COLUMN enhancement_model_name TEXT;";

pub const ALTER_ADD_ENHANCEMENT_DURATION: &str =
    "ALTER TABLE transcriptions ADD COLUMN enhancement_duration_seconds REAL;";
