//! Database migration system for Thoth.
//!
//! Migrations are versioned and tracked in the `migrations` table.
//! Each migration is run exactly once, in order.

use rusqlite::Connection;

use crate::database::schema::{
    ALTER_ADD_ENHANCEMENT_DURATION, ALTER_ADD_ENHANCEMENT_MODEL_NAME,
    ALTER_ADD_TRANSCRIPTION_DURATION, ALTER_ADD_TRANSCRIPTION_MODEL_NAME, CREATE_MIGRATIONS_TABLE,
    CREATE_TRANSCRIPTIONS_CREATED_AT_INDEX, CREATE_TRANSCRIPTIONS_IS_ENHANCED_INDEX,
    CREATE_TRANSCRIPTIONS_TABLE,
};
use crate::database::DatabaseError;

/// A database migration with a version number, name, and SQL statements.
struct Migration {
    version: i32,
    name: &'static str,
    statements: &'static [&'static str],
}

/// All migrations to be applied, in order.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "create_transcriptions_table",
        statements: &[
            CREATE_TRANSCRIPTIONS_TABLE,
            CREATE_TRANSCRIPTIONS_CREATED_AT_INDEX,
            CREATE_TRANSCRIPTIONS_IS_ENHANCED_INDEX,
        ],
    },
    Migration {
        version: 2,
        name: "add_transcription_metadata",
        statements: &[
            ALTER_ADD_TRANSCRIPTION_MODEL_NAME,
            ALTER_ADD_TRANSCRIPTION_DURATION,
            ALTER_ADD_ENHANCEMENT_MODEL_NAME,
            ALTER_ADD_ENHANCEMENT_DURATION,
        ],
    },
];

/// Returns the current schema version from the database.
fn get_current_version(conn: &Connection) -> Result<i32, DatabaseError> {
    let version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(version)
}

/// Records a migration as applied.
fn record_migration(conn: &Connection, version: i32, name: &str) -> Result<(), DatabaseError> {
    conn.execute(
        "INSERT INTO migrations (version, name) VALUES (?1, ?2)",
        (version, name),
    )?;
    Ok(())
}

/// Runs all pending migrations.
///
/// Migrations are run in a transaction; if any migration fails, all changes
/// are rolled back.
pub fn run_migrations(conn: &mut Connection) -> Result<(), DatabaseError> {
    // First, ensure the migrations table exists
    conn.execute_batch(CREATE_MIGRATIONS_TABLE)?;

    let current_version = get_current_version(conn)?;
    tracing::info!("Current database schema version: {}", current_version);

    // Find migrations that need to be applied
    let pending: Vec<&Migration> = MIGRATIONS
        .iter()
        .filter(|m| m.version > current_version)
        .collect();

    if pending.is_empty() {
        tracing::info!("Database schema is up to date");
        return Ok(());
    }

    tracing::info!("{} pending migration(s) to apply", pending.len());

    // Apply each migration in a transaction
    for migration in pending {
        tracing::info!(
            "Applying migration {} (v{})",
            migration.name,
            migration.version
        );

        let tx = conn.transaction()?;

        for statement in migration.statements {
            tx.execute_batch(statement).map_err(|e| {
                DatabaseError::Migration(format!("Migration {} failed: {}", migration.name, e))
            })?;
        }

        record_migration(&tx, migration.version, migration.name)?;
        tx.commit()?;

        tracing::info!("Migration {} applied successfully", migration.name);
    }

    let final_version = get_current_version(conn)?;
    tracing::info!("Database schema now at version {}", final_version);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_migrations_are_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();

        // Run migrations twice; should not fail
        run_migrations(&mut conn).unwrap();
        run_migrations(&mut conn).unwrap();

        // Check that transcriptions table exists
        let table_exists: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='transcriptions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, 1);
    }

    #[test]
    fn test_migration_version_tracking() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        let version: i32 = conn
            .query_row("SELECT MAX(version) FROM migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn test_transcriptions_table_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).unwrap();

        // Insert a test record with all columns to verify schema
        conn.execute(
            r#"
            INSERT INTO transcriptions (
                id, text, raw_text, duration_seconds, created_at, audio_path,
                is_enhanced, enhancement_prompt,
                transcription_model_name, transcription_duration_seconds,
                enhancement_model_name, enhancement_duration_seconds
            )
            VALUES (
                'test-uuid', 'Hello world', 'hello world', 5.5,
                '2025-01-15T10:30:00Z', '/path/to/audio.wav', 1, 'grammar',
                'ggml-large-v3-turbo', 1.2, 'llama3.2:3b', 0.8
            )
            "#,
            [],
        )
        .unwrap();

        // Verify the record was inserted correctly
        let text: String = conn
            .query_row(
                "SELECT text FROM transcriptions WHERE id = 'test-uuid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(text, "Hello world");

        // Verify is_enhanced is stored as integer
        let is_enhanced: i32 = conn
            .query_row(
                "SELECT is_enhanced FROM transcriptions WHERE id = 'test-uuid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(is_enhanced, 1);

        // Verify metadata columns
        let model_name: Option<String> = conn
            .query_row(
                "SELECT transcription_model_name FROM transcriptions WHERE id = 'test-uuid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(model_name, Some("ggml-large-v3-turbo".to_string()));

        let transcription_time: Option<f64> = conn
            .query_row(
                "SELECT transcription_duration_seconds FROM transcriptions WHERE id = 'test-uuid'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(transcription_time, Some(1.2));
    }
}
