//! Transcription CRUD operations.
//!
//! Provides functions for creating, reading, updating, and deleting transcriptions
//! in the SQLite database.

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::{open_connection, DatabaseError};

/// A transcription record stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    /// Unique identifier (UUID).
    pub id: String,
    /// The transcribed text (possibly enhanced).
    pub text: String,
    /// Original text before enhancement (if any).
    pub raw_text: Option<String>,
    /// Duration of the audio in seconds.
    pub duration_seconds: Option<f64>,
    /// When the transcription was created (ISO 8601).
    pub created_at: String,
    /// Path to the audio file (if retained).
    pub audio_path: Option<String>,
    /// Whether the text has been enhanced by AI.
    pub is_enhanced: bool,
    /// Which enhancement prompt was used (if enhanced).
    pub enhancement_prompt: Option<String>,
    /// Name of the transcription model used (e.g., "ggml-large-v3-turbo").
    pub transcription_model_name: Option<String>,
    /// Time taken to transcribe the audio, in seconds.
    pub transcription_duration_seconds: Option<f64>,
    /// Name of the AI model used for enhancement (e.g., "llama3.2:3b").
    pub enhancement_model_name: Option<String>,
    /// Time taken for AI enhancement, in seconds.
    pub enhancement_duration_seconds: Option<f64>,
}

impl Transcription {
    /// Creates a new transcription with a generated UUID and current timestamp.
    pub fn new(text: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            text,
            raw_text: None,
            duration_seconds: None,
            created_at: Utc::now().to_rfc3339(),
            audio_path: None,
            is_enhanced: false,
            enhancement_prompt: None,
            transcription_model_name: None,
            transcription_duration_seconds: None,
            enhancement_model_name: None,
            enhancement_duration_seconds: None,
        }
    }

    /// Creates a new transcription with all fields specified.
    #[allow(clippy::too_many_arguments)]
    pub fn with_details(
        text: String,
        raw_text: Option<String>,
        duration_seconds: Option<f64>,
        audio_path: Option<String>,
        is_enhanced: bool,
        enhancement_prompt: Option<String>,
        transcription_model_name: Option<String>,
        transcription_duration_seconds: Option<f64>,
        enhancement_model_name: Option<String>,
        enhancement_duration_seconds: Option<f64>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            text,
            raw_text,
            duration_seconds,
            created_at: Utc::now().to_rfc3339(),
            audio_path,
            is_enhanced,
            enhancement_prompt,
            transcription_model_name,
            transcription_duration_seconds,
            enhancement_model_name,
            enhancement_duration_seconds,
        }
    }
}

// =============================================================================
// Database Functions
// =============================================================================

/// Creates a new transcription in the database.
pub fn create_transcription(transcription: &Transcription) -> Result<(), DatabaseError> {
    let conn = open_connection()?;

    conn.execute(
        r#"
        INSERT INTO transcriptions (
            id, text, raw_text, duration_seconds, created_at, audio_path,
            is_enhanced, enhancement_prompt,
            transcription_model_name, transcription_duration_seconds,
            enhancement_model_name, enhancement_duration_seconds
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
        "#,
        params![
            transcription.id,
            transcription.text,
            transcription.raw_text,
            transcription.duration_seconds,
            transcription.created_at,
            transcription.audio_path,
            transcription.is_enhanced as i32,
            transcription.enhancement_prompt,
            transcription.transcription_model_name,
            transcription.transcription_duration_seconds,
            transcription.enhancement_model_name,
            transcription.enhancement_duration_seconds,
        ],
    )?;

    tracing::debug!("Created transcription: {}", transcription.id);
    Ok(())
}

/// Column list for all SELECT queries.
const SELECT_COLUMNS: &str = r#"
    id, text, raw_text, duration_seconds, created_at, audio_path,
    is_enhanced, enhancement_prompt,
    transcription_model_name, transcription_duration_seconds,
    enhancement_model_name, enhancement_duration_seconds
"#;

/// Map a database row to a Transcription struct.
fn row_to_transcription(row: &rusqlite::Row) -> rusqlite::Result<Transcription> {
    Ok(Transcription {
        id: row.get(0)?,
        text: row.get(1)?,
        raw_text: row.get(2)?,
        duration_seconds: row.get(3)?,
        created_at: row.get(4)?,
        audio_path: row.get(5)?,
        is_enhanced: row.get::<_, i32>(6)? != 0,
        enhancement_prompt: row.get(7)?,
        transcription_model_name: row.get(8)?,
        transcription_duration_seconds: row.get(9)?,
        enhancement_model_name: row.get(10)?,
        enhancement_duration_seconds: row.get(11)?,
    })
}

/// Retrieves a transcription by its ID.
pub fn get_transcription(id: &str) -> Result<Option<Transcription>, DatabaseError> {
    let conn = open_connection()?;

    let result = conn.query_row(
        &format!(
            "SELECT {} FROM transcriptions WHERE id = ?1",
            SELECT_COLUMNS
        ),
        params![id],
        row_to_transcription,
    );

    match result {
        Ok(transcription) => Ok(Some(transcription)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Updates an existing transcription.
pub fn update_transcription(transcription: &Transcription) -> Result<(), DatabaseError> {
    let conn = open_connection()?;

    let rows_affected = conn.execute(
        r#"
        UPDATE transcriptions
        SET text = ?2, raw_text = ?3, duration_seconds = ?4, audio_path = ?5,
            is_enhanced = ?6, enhancement_prompt = ?7,
            transcription_model_name = ?8, transcription_duration_seconds = ?9,
            enhancement_model_name = ?10, enhancement_duration_seconds = ?11
        WHERE id = ?1
        "#,
        params![
            transcription.id,
            transcription.text,
            transcription.raw_text,
            transcription.duration_seconds,
            transcription.audio_path,
            transcription.is_enhanced as i32,
            transcription.enhancement_prompt,
            transcription.transcription_model_name,
            transcription.transcription_duration_seconds,
            transcription.enhancement_model_name,
            transcription.enhancement_duration_seconds,
        ],
    )?;

    if rows_affected == 0 {
        tracing::warn!("No transcription found with id: {}", transcription.id);
    } else {
        tracing::debug!("Updated transcription: {}", transcription.id);
    }

    Ok(())
}

/// Deletes a transcription by its ID.
pub fn delete_transcription(id: &str) -> Result<bool, DatabaseError> {
    let conn = open_connection()?;

    let rows_affected = conn.execute("DELETE FROM transcriptions WHERE id = ?1", params![id])?;

    if rows_affected > 0 {
        tracing::debug!("Deleted transcription: {}", id);
        Ok(true)
    } else {
        tracing::warn!("No transcription found with id: {}", id);
        Ok(false)
    }
}

/// Lists all transcriptions, ordered by creation date (newest first).
pub fn list_transcriptions(
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Transcription>, DatabaseError> {
    let conn = open_connection()?;

    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);

    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM transcriptions ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        SELECT_COLUMNS
    ))?;

    let transcriptions = stmt
        .query_map(params![limit, offset], row_to_transcription)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(transcriptions)
}

/// Searches transcriptions using LIKE pattern matching on the text field.
pub fn search_transcriptions(
    query: &str,
    limit: Option<i64>,
) -> Result<Vec<Transcription>, DatabaseError> {
    let conn = open_connection()?;

    let limit = limit.unwrap_or(50);
    let search_pattern = format!("%{}%", query);

    let mut stmt = conn.prepare(&format!(
        "SELECT {} FROM transcriptions WHERE text LIKE ?1 ORDER BY created_at DESC LIMIT ?2",
        SELECT_COLUMNS
    ))?;

    let transcriptions = stmt
        .query_map(params![search_pattern, limit], row_to_transcription)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(transcriptions)
}

/// Counts the total number of transcriptions, optionally filtered by a search query.
pub fn count_transcriptions(query: Option<&str>) -> Result<usize, DatabaseError> {
    let conn = open_connection()?;

    let count: i64 = match query {
        Some(q) if !q.is_empty() => {
            let search_pattern = format!("%{}%", q);
            conn.query_row(
                "SELECT COUNT(*) FROM transcriptions WHERE text LIKE ?1",
                params![search_pattern],
                |row| row.get(0),
            )?
        }
        _ => conn.query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))?,
    };

    Ok(count as usize)
}

// =============================================================================
// Statistics
// =============================================================================

/// Aggregated statistics for the performance analysis dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionStats {
    /// Total number of transcriptions.
    pub total_count: usize,
    /// Number of transcriptions with performance metadata.
    pub analysable_count: usize,
    /// Number of enhanced transcriptions.
    pub enhanced_count: usize,
    /// Total audio duration across all transcriptions (seconds).
    pub total_audio_duration: f64,
    /// Per-model transcription performance stats.
    pub transcription_models: Vec<ModelStats>,
    /// Per-model enhancement performance stats.
    pub enhancement_models: Vec<ModelStats>,
}

/// Performance stats for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    /// Model name.
    pub name: String,
    /// Number of transcriptions using this model.
    pub count: usize,
    /// Average audio duration (seconds).
    pub avg_audio_duration: f64,
    /// Average processing time (seconds).
    pub avg_processing_time: f64,
    /// Real-time factor (audio duration / processing time). Higher is faster.
    pub speed_factor: f64,
}

/// Computes aggregated transcription statistics.
pub fn get_transcription_stats() -> Result<TranscriptionStats, DatabaseError> {
    let conn = open_connection()?;

    // Summary counts
    let total_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))?;

    let analysable_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transcriptions WHERE transcription_duration_seconds IS NOT NULL",
        [],
        |row| row.get(0),
    )?;

    let enhanced_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM transcriptions WHERE is_enhanced = 1",
        [],
        |row| row.get(0),
    )?;

    let total_audio_duration: f64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_seconds), 0) FROM transcriptions",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    // Per-model transcription stats
    let mut stmt = conn.prepare(
        r#"
        SELECT
            transcription_model_name,
            COUNT(*) as cnt,
            COALESCE(AVG(duration_seconds), 0) as avg_audio,
            AVG(transcription_duration_seconds) as avg_proc
        FROM transcriptions
        WHERE transcription_model_name IS NOT NULL
          AND transcription_duration_seconds IS NOT NULL
        GROUP BY transcription_model_name
        ORDER BY cnt DESC
        "#,
    )?;

    let transcription_models = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            let avg_audio: f64 = row.get(2)?;
            let avg_proc: f64 = row.get(3)?;
            let speed_factor = if avg_proc > 0.0 {
                avg_audio / avg_proc
            } else {
                0.0
            };
            Ok(ModelStats {
                name,
                count: count as usize,
                avg_audio_duration: avg_audio,
                avg_processing_time: avg_proc,
                speed_factor,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Per-model enhancement stats
    let mut stmt = conn.prepare(
        r#"
        SELECT
            enhancement_model_name,
            COUNT(*) as cnt,
            COALESCE(AVG(duration_seconds), 0) as avg_audio,
            AVG(enhancement_duration_seconds) as avg_proc
        FROM transcriptions
        WHERE enhancement_model_name IS NOT NULL
          AND enhancement_duration_seconds IS NOT NULL
        GROUP BY enhancement_model_name
        ORDER BY cnt DESC
        "#,
    )?;

    let enhancement_models = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            let avg_audio: f64 = row.get(2)?;
            let avg_proc: f64 = row.get(3)?;
            Ok(ModelStats {
                name,
                count: count as usize,
                avg_audio_duration: avg_audio,
                avg_processing_time: avg_proc,
                speed_factor: 0.0, // Not meaningful for enhancement
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TranscriptionStats {
        total_count: total_count as usize,
        analysable_count: analysable_count as usize,
        enhanced_count: enhanced_count as usize,
        total_audio_duration,
        transcription_models,
        enhancement_models,
    })
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// Returns aggregated transcription statistics for the performance dashboard.
#[tauri::command]
pub fn get_transcription_stats_cmd() -> Result<TranscriptionStats, String> {
    get_transcription_stats().map_err(|e| {
        tracing::error!("Failed to get transcription stats: {}", e);
        format!("Failed to get stats: {}", e)
    })
}

/// Saves a new transcription to the database.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn save_transcription(
    text: String,
    raw_text: Option<String>,
    duration_seconds: Option<f64>,
    audio_path: Option<String>,
    is_enhanced: bool,
    enhancement_prompt: Option<String>,
    transcription_model_name: Option<String>,
    transcription_duration_seconds: Option<f64>,
    enhancement_model_name: Option<String>,
    enhancement_duration_seconds: Option<f64>,
) -> Result<Transcription, String> {
    let transcription = Transcription::with_details(
        text,
        raw_text,
        duration_seconds,
        audio_path,
        is_enhanced,
        enhancement_prompt,
        transcription_model_name,
        transcription_duration_seconds,
        enhancement_model_name,
        enhancement_duration_seconds,
    );

    create_transcription(&transcription).map_err(|e| {
        tracing::error!("Failed to save transcription: {}", e);
        format!("Failed to save transcription: {}", e)
    })?;

    Ok(transcription)
}

/// Retrieves a transcription by its ID.
#[tauri::command]
pub fn get_transcription_by_id(id: String) -> Result<Option<Transcription>, String> {
    get_transcription(&id).map_err(|e| {
        tracing::error!("Failed to get transcription {}: {}", id, e);
        format!("Failed to get transcription: {}", e)
    })
}

/// Lists all transcriptions with optional pagination.
#[tauri::command]
pub fn list_all_transcriptions(
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Transcription>, String> {
    list_transcriptions(limit, offset).map_err(|e| {
        tracing::error!("Failed to list transcriptions: {}", e);
        format!("Failed to list transcriptions: {}", e)
    })
}

/// Deletes a transcription by its ID.
#[tauri::command]
pub fn delete_transcription_by_id(id: String) -> Result<bool, String> {
    delete_transcription(&id).map_err(|e| {
        tracing::error!("Failed to delete transcription {}: {}", id, e);
        format!("Failed to delete transcription: {}", e)
    })
}

/// Searches transcriptions by text content.
#[tauri::command]
pub fn search_transcriptions_text(
    query: String,
    limit: Option<i64>,
) -> Result<Vec<Transcription>, String> {
    search_transcriptions(&query, limit).map_err(|e| {
        tracing::error!("Failed to search transcriptions: {}", e);
        format!("Failed to search transcriptions: {}", e)
    })
}

/// Counts transcriptions, optionally filtered by a search query.
#[tauri::command]
pub fn count_transcriptions_filtered(query: Option<String>) -> Result<usize, String> {
    count_transcriptions(query.as_deref()).map_err(|e| {
        tracing::error!("Failed to count transcriptions: {}", e);
        format!("Failed to count transcriptions: {}", e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcription_new() {
        let t = Transcription::new("Hello world".to_string());
        assert!(!t.id.is_empty());
        assert_eq!(t.text, "Hello world");
        assert!(!t.is_enhanced);
        assert!(t.raw_text.is_none());
    }

    #[test]
    fn test_transcription_with_details() {
        let t = Transcription::with_details(
            "Enhanced text".to_string(),
            Some("original text".to_string()),
            Some(5.5),
            Some("/path/to/audio.wav".to_string()),
            true,
            Some("grammar".to_string()),
            Some("ggml-large-v3-turbo".to_string()),
            Some(1.2),
            Some("llama3.2:3b".to_string()),
            Some(0.8),
        );

        assert_eq!(t.text, "Enhanced text");
        assert_eq!(t.raw_text, Some("original text".to_string()));
        assert_eq!(t.duration_seconds, Some(5.5));
        assert!(t.is_enhanced);
        assert_eq!(t.enhancement_prompt, Some("grammar".to_string()));
        assert_eq!(
            t.transcription_model_name,
            Some("ggml-large-v3-turbo".to_string())
        );
        assert_eq!(t.transcription_duration_seconds, Some(1.2));
        assert_eq!(t.enhancement_model_name, Some("llama3.2:3b".to_string()));
        assert_eq!(t.enhancement_duration_seconds, Some(0.8));
    }
}
