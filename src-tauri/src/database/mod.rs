//! Database module for Thoth.
//!
//! Provides SQLite database connection management and migrations.
//! Database is stored at `~/.thoth/thoth.db`.

pub mod migrations;
pub mod schema;
pub mod transcription;

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::database::migrations::run_migrations;

/// Global database path, initialised once.
static DATABASE_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Database error types.
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Failed to create database directory: {0}")]
    DirectoryCreation(#[from] std::io::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Migration failed: {0}")]
    Migration(String),
}

/// Returns the path to the Thoth database directory (~/.thoth).
fn get_thoth_directory() -> Result<PathBuf, DatabaseError> {
    let home = dirs::home_dir().ok_or_else(|| {
        DatabaseError::DirectoryCreation(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not find home directory",
        ))
    })?;

    Ok(home.join(".thoth"))
}

/// Returns the path to the database file (~/.thoth/thoth.db).
pub fn get_database_path() -> Result<PathBuf, DatabaseError> {
    let thoth_dir = get_thoth_directory()?;
    Ok(thoth_dir.join("thoth.db"))
}

/// Ensures the database directory exists and returns the database path.
fn ensure_database_directory() -> Result<PathBuf, DatabaseError> {
    let thoth_dir = get_thoth_directory()?;

    if !thoth_dir.exists() {
        std::fs::create_dir_all(&thoth_dir)?;
        tracing::info!("Created Thoth directory at {:?}", thoth_dir);
    }

    Ok(thoth_dir.join("thoth.db"))
}

/// Opens a connection to the database.
///
/// Each call creates a new connection. For thread safety in Tauri commands,
/// create a new connection per command invocation.
pub fn open_connection() -> Result<Connection, DatabaseError> {
    // DATABASE_PATH is normally set by initialise_database() at startup.
    // The expect here is a safeguard for direct open_connection() calls.
    let db_path = DATABASE_PATH.get_or_init(|| {
        ensure_database_directory()
            .expect("database directory must be writable; called before initialise_database()?")
    });

    let conn = Connection::open(db_path)?;

    // Enable foreign keys
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;

    Ok(conn)
}

/// Initialises the database, creating the directory and running migrations.
///
/// This should be called once on application startup.
pub fn initialise_database() -> Result<(), DatabaseError> {
    tracing::info!("Initialising database");

    // Ensure directory exists and get path
    let db_path = ensure_database_directory()?;
    DATABASE_PATH.get_or_init(|| db_path.clone());

    tracing::info!("Database path: {:?}", db_path);

    // Open connection and run migrations
    let mut conn = open_connection()?;
    run_migrations(&mut conn)?;

    tracing::info!("Database initialised successfully");
    Ok(())
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// Initialises the database. Call this on application startup.
#[tauri::command]
pub fn init_database() -> Result<(), String> {
    initialise_database().map_err(|e| {
        tracing::error!("Failed to initialise database: {}", e);
        format!("Failed to initialise database: {}", e)
    })
}

/// Returns the path to the database file.
#[tauri::command]
pub fn get_database_path_command() -> Result<String, String> {
    get_database_path()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("Failed to get database path: {}", e))
}

// =============================================================================
// Re-exports
// =============================================================================

// Re-export transcription types for convenience
pub use transcription::Transcription;

// Re-export transcription CRUD functions
pub use transcription::{
    count_transcriptions, create_transcription, delete_transcription, get_transcription,
    list_transcriptions, search_transcriptions, update_transcription,
};

// Re-export transcription Tauri commands
pub use transcription::{
    count_transcriptions_filtered, delete_transcription_by_id, get_transcription_by_id,
    get_transcription_stats_cmd, list_all_transcriptions, save_transcription,
    search_transcriptions_text,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_path_format() {
        let path = get_database_path().unwrap();
        assert!(path.to_string_lossy().contains(".thoth"));
        assert!(path.to_string_lossy().ends_with("thoth.db"));
    }
}
