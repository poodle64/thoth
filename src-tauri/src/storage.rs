//! Storage management for Thoth
//!
//! Provides disk usage reporting and cleanup commands for all data
//! locations: models, recordings, logs, database, config, and
//! FluidAudio CoreML cache.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Disk usage breakdown by category
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    /// Speech recognition models (~/.thoth/models/)
    pub models_bytes: u64,
    /// Audio recordings (~/.thoth/Recordings/)
    pub recordings_bytes: u64,
    /// Debug logs (~/.thoth/logs/)
    pub logs_bytes: u64,
    /// SQLite database (~/.thoth/thoth.db)
    pub database_bytes: u64,
    /// Config + dictionary + prompts (small files)
    pub config_bytes: u64,
    /// FluidAudio CoreML cache (~/Library/Application Support/FluidAudio/Models/)
    pub fluidaudio_bytes: u64,
    /// Total across all categories
    pub total_bytes: u64,
    /// Number of recording files
    pub recording_count: u64,
    /// Number of log files
    pub log_count: u64,
}

/// Get the Thoth data directory (~/.thoth)
fn thoth_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".thoth")
}

/// Get FluidAudio model cache directory
///
/// Only targets the Models subdirectory — FluidAudio may store other
/// data in the parent `Application Support/FluidAudio/` directory.
fn fluidaudio_models_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join("Library")
        .join("Application Support")
        .join("FluidAudio")
        .join("Models")
}

/// Calculate total size of a directory recursively
fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    walkdir(path)
}

/// Recursive directory size calculation
fn walkdir(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                total += walkdir(&entry_path);
            } else if let Ok(meta) = entry_path.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Count files in a directory (non-recursive)
fn file_count(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    fs::read_dir(path)
        .map(|entries| entries.flatten().filter(|e| e.path().is_file()).count() as u64)
        .unwrap_or(0)
}

/// Calculate the size of known config files
fn config_file_sizes(base: &Path) -> u64 {
    let files = ["config.json", "dictionary.json", "prompts.json"];
    files
        .iter()
        .filter_map(|f| fs::metadata(base.join(f)).ok())
        .map(|m| m.len())
        .sum()
}

/// Get storage usage breakdown
#[tauri::command]
pub fn get_storage_usage() -> Result<StorageUsage, String> {
    let base = thoth_dir();

    let models_bytes = dir_size(&base.join("models"));
    let recordings_bytes = dir_size(&base.join("Recordings"));
    let logs_bytes = dir_size(&base.join("logs"));
    let database_bytes = fs::metadata(base.join("thoth.db"))
        .map(|m| m.len())
        .unwrap_or(0);
    let config_bytes = config_file_sizes(&base);
    let fluidaudio_bytes = dir_size(&fluidaudio_models_dir());

    let recording_count = file_count(&base.join("Recordings"));
    let log_count = file_count(&base.join("logs"));

    let total_bytes = models_bytes
        + recordings_bytes
        + logs_bytes
        + database_bytes
        + config_bytes
        + fluidaudio_bytes;

    Ok(StorageUsage {
        models_bytes,
        recordings_bytes,
        logs_bytes,
        database_bytes,
        config_bytes,
        fluidaudio_bytes,
        total_bytes,
        recording_count,
        log_count,
    })
}

/// Delete all audio recordings
#[tauri::command]
pub fn delete_all_recordings() -> Result<u64, String> {
    let recordings_dir = thoth_dir().join("Recordings");
    if !recordings_dir.exists() {
        return Ok(0);
    }

    let mut deleted = 0u64;
    let entries = fs::read_dir(&recordings_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                tracing::warn!("Failed to delete recording {:?}: {}", path, e);
            } else {
                deleted += 1;
            }
        }
    }

    tracing::info!("Deleted {} recording files", deleted);
    Ok(deleted)
}

/// Delete all log files
#[tauri::command]
pub fn delete_all_logs() -> Result<u64, String> {
    let logs_dir = thoth_dir().join("logs");
    if !logs_dir.exists() {
        return Ok(0);
    }

    let mut deleted = 0u64;
    let entries = fs::read_dir(&logs_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Err(e) = fs::remove_file(&path) {
                tracing::warn!("Failed to delete log file {:?}: {}", path, e);
            } else {
                deleted += 1;
            }
        }
    }

    tracing::info!("Deleted {} log files", deleted);
    Ok(deleted)
}

/// Delete the FluidAudio CoreML model cache
#[tauri::command]
pub fn delete_fluidaudio_cache() -> Result<(), String> {
    let cache_dir = fluidaudio_models_dir();
    if !cache_dir.exists() {
        return Ok(());
    }

    fs::remove_dir_all(&cache_dir).map_err(|e| {
        format!(
            "Failed to delete FluidAudio cache at {}: {}",
            cache_dir.display(),
            e
        )
    })?;

    // Also remove the ready marker so Model Manager reflects the change
    let marker_dir = thoth_dir()
        .join("models")
        .join("fluidaudio-parakeet-tdt-coreml");
    let marker_path = marker_dir.join(".fluidaudio_ready");
    if marker_path.exists() {
        let _ = fs::remove_file(&marker_path);
    }

    tracing::info!("Deleted FluidAudio cache directory");
    Ok(())
}

/// Delete ALL Thoth data (full reset / uninstall cleanup)
///
/// Removes ~/.thoth/ and ~/Library/Application Support/FluidAudio/Models/
#[tauri::command]
pub fn delete_all_data() -> Result<(), String> {
    let base = thoth_dir();
    if base.exists() {
        fs::remove_dir_all(&base)
            .map_err(|e| format!("Failed to delete Thoth data at {}: {}", base.display(), e))?;
        tracing::info!("Deleted Thoth data directory: {}", base.display());
    }

    let fluid_dir = fluidaudio_models_dir();
    if fluid_dir.exists() {
        fs::remove_dir_all(&fluid_dir).map_err(|e| {
            format!(
                "Failed to delete FluidAudio cache at {}: {}",
                fluid_dir.display(),
                e
            )
        })?;
        tracing::info!(
            "Deleted FluidAudio cache directory: {}",
            fluid_dir.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thoth_dir_path() {
        let dir = thoth_dir();
        assert!(dir.to_string_lossy().contains(".thoth"));
    }

    #[test]
    fn test_fluidaudio_dir_path() {
        let dir = fluidaudio_models_dir();
        assert!(dir.to_string_lossy().contains("FluidAudio"));
    }

    #[test]
    fn test_dir_size_nonexistent() {
        let path = PathBuf::from("/nonexistent/path/that/doesnt/exist");
        assert_eq!(dir_size(&path), 0);
    }

    #[test]
    fn test_file_count_nonexistent() {
        let path = PathBuf::from("/nonexistent/path/that/doesnt/exist");
        assert_eq!(file_count(&path), 0);
    }
}
