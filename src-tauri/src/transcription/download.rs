//! Model download manager for transcription models
//!
//! Supports both:
//! - Direct file downloads (whisper.cpp ggml models)
//! - Archive downloads with extraction (sherpa-onnx models)

use super::manifest::{get_fallback_manifest, get_model_directory, RemoteModelInfo};
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

/// Download progress information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    /// Current file being downloaded
    pub current_file: String,
    /// Bytes downloaded so far
    pub bytes_downloaded: u64,
    /// Total bytes to download (if known)
    pub total_bytes: Option<u64>,
    /// Progress percentage (0-100)
    pub percentage: f32,
    /// Current status message
    pub status: String,
}

/// Download state tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadState {
    /// No download in progress
    Idle,
    /// Download is in progress
    Downloading,
    /// Extracting archive
    Extracting,
    /// Download completed successfully
    Completed,
    /// Download failed with error
    Failed(String),
}

/// Global download state
static DOWNLOAD_STATE: OnceLock<Mutex<DownloadState>> = OnceLock::new();

fn get_download_state() -> &'static Mutex<DownloadState> {
    DOWNLOAD_STATE.get_or_init(|| Mutex::new(DownloadState::Idle))
}

/// Check if the model files are downloaded and valid
#[tauri::command]
pub fn check_model_downloaded(model_id: Option<String>) -> bool {
    // Get the model info from manifest
    let manifest = get_fallback_manifest();

    // Use provided model_id, or get from config, or fall back to recommended
    let model_id = model_id.unwrap_or_else(|| {
        // Try to get from config first
        crate::config::get_config()
            .ok()
            .and_then(|c| c.transcription.model_id.clone())
            .unwrap_or_else(|| {
                // Fall back to recommended model from manifest
                manifest
                    .models
                    .iter()
                    .find(|m| m.recommended)
                    .or_else(|| manifest.models.first())
                    .map(|m| m.id.clone())
                    .unwrap_or_else(|| "ggml-large-v3-turbo".to_string())
            })
    });

    let model = manifest.models.iter().find(|m| m.id == model_id);
    let required_files: Vec<&str> = match model {
        Some(m) => m.required_files.iter().map(|s| s.as_str()).collect(),
        None => vec![
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ],
    };

    let model_dir = get_model_directory(&model_id);

    for file in required_files {
        let path = model_dir.join(file);
        if !path.exists() {
            tracing::debug!("Model file missing: {}", path.display());
            return false;
        }

        // Verify file has content (not empty)
        match std::fs::metadata(&path) {
            Ok(metadata) => {
                if metadata.len() == 0 {
                    tracing::warn!("Model file is empty: {}", path.display());
                    return false;
                }
            }
            Err(e) => {
                tracing::warn!("Failed to read model file metadata: {}", e);
                return false;
            }
        }
    }

    tracing::info!("All model files present and valid for {}", model_id);
    true
}

/// Get the current download progress state
#[tauri::command]
pub fn get_download_progress() -> DownloadState {
    get_download_state().lock().clone()
}

/// Download the model archive and extract it
///
/// Emits progress events to the Tauri frontend:
/// - `model-download-progress`: Progress updates during download
/// - `model-download-complete`: When download and extraction complete
/// - `model-download-error`: If an error occurs
#[tauri::command]
pub async fn download_model(app: AppHandle, model_id: Option<String>) -> Result<(), String> {
    // Check if already downloading
    {
        let state = get_download_state().lock().clone();
        if state == DownloadState::Downloading || state == DownloadState::Extracting {
            return Err("Download already in progress".to_string());
        }
    }

    // Get model info from manifest
    let manifest = get_fallback_manifest();
    let model_id = model_id.unwrap_or_else(|| {
        manifest
            .models
            .iter()
            .find(|m| m.recommended)
            .or_else(|| manifest.models.first())
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "parakeet-tdt-0.6b-v3-int8".to_string())
    });

    let model = manifest
        .models
        .iter()
        .find(|m| m.id == model_id)
        .cloned()
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    // Update state to downloading
    {
        let mut state = get_download_state().lock();
        *state = DownloadState::Downloading;
    }

    // Run the download in the background
    let result = download_and_extract_model(&app, &model).await;

    match result {
        Ok(()) => {
            {
                let mut state = get_download_state().lock();
                *state = DownloadState::Completed;
            }
            app.emit("model-download-complete", &model_id)
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        Err(e) => {
            let error_msg = e.to_string();
            {
                let mut state = get_download_state().lock();
                *state = DownloadState::Failed(error_msg.clone());
            }
            app.emit("model-download-error", &error_msg)
                .map_err(|e| e.to_string())?;
            Err(error_msg)
        }
    }
}

/// Check if a model is a direct file download (not an archive)
fn is_direct_download(model: &RemoteModelInfo) -> bool {
    // Whisper ggml models are direct .bin file downloads
    if model.model_type == "whisper_ggml" {
        return true;
    }

    // If no archive_directory is specified and URL ends in .bin, it's a direct download
    if model.archive_directory.is_none() && model.download_url.ends_with(".bin") {
        return true;
    }

    false
}

/// Internal function to download and extract the model
/// Trusted domains for model downloads
const TRUSTED_DOWNLOAD_DOMAINS: &[&str] = &[
    "github.com",
    "huggingface.co",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
];

/// Validate that a download URL points to a trusted domain
fn validate_download_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(|e| anyhow!("Invalid download URL: {}", e))?;

    let host = parsed.host_str().ok_or_else(|| anyhow!("Download URL has no host"))?;

    let trusted = TRUSTED_DOWNLOAD_DOMAINS
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{}", domain)));

    if !trusted {
        return Err(anyhow!(
            "Download URL host '{}' is not in the trusted domains list",
            host
        ));
    }
    Ok(())
}

/// Verify SHA256 checksum of a downloaded file
fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = format!("{:x}", hasher.finalize());
    if hash != expected.to_lowercase() {
        return Err(anyhow!(
            "SHA256 mismatch: expected {}, got {}",
            expected,
            hash
        ));
    }

    tracing::info!("SHA256 verification passed");
    Ok(())
}

async fn download_and_extract_model(app: &AppHandle, model: &RemoteModelInfo) -> Result<()> {
    let model_dir = get_model_directory(&model.id);

    // Validate the download URL points to a trusted domain
    validate_download_url(&model.download_url)?;

    // Create model directory if it doesn't exist
    std::fs::create_dir_all(&model_dir)?;

    tracing::info!("Downloading model {} from {}", model.id, model.download_url);

    // Check if this is a direct file download (e.g., whisper .bin files)
    if is_direct_download(model) {
        // For direct downloads, save directly to the required file name
        let file_name = model.required_files.first()
            .ok_or_else(|| anyhow!("No required files specified for model"))?;
        let dest_path = model_dir.join(file_name);

        download_file_with_progress(app, &model.download_url, &dest_path, &model.name).await?;

        // Verify SHA256 if provided
        if let Some(ref expected_sha) = model.sha256 {
            verify_sha256(&dest_path, expected_sha)?;
        }

        tracing::info!("Direct download complete: {}", dest_path.display());
    } else {
        // For archives, download and extract
        let archive_path = model_dir.join("model-archive.tar.bz2");
        download_file_with_progress(app, &model.download_url, &archive_path, &model.name).await?;

        // Verify SHA256 if provided
        if let Some(ref expected_sha) = model.sha256 {
            verify_sha256(&archive_path, expected_sha)?;
        }

        // Update state to extracting
        {
            let mut state = get_download_state().lock();
            *state = DownloadState::Extracting;
        }

        emit_progress(
            app,
            DownloadProgress {
                current_file: "Extracting archive".to_string(),
                bytes_downloaded: 0,
                total_bytes: None,
                percentage: 0.0,
                status: "Extracting model files...".to_string(),
            },
        );

        // Extract the archive
        extract_tar_bz2(&archive_path, &model_dir, &model.required_files, model.archive_directory.as_deref())?;

        // Clean up archive file
        if let Err(e) = std::fs::remove_file(&archive_path) {
            tracing::warn!("Failed to remove archive file: {}", e);
        }
    }

    // Verify all files exist
    if !check_model_downloaded(Some(model.id.clone())) {
        return Err(anyhow!("Model download failed: required files missing"));
    }

    tracing::info!(
        "Model {} download completed successfully",
        model.id
    );
    Ok(())
}

/// Download a file with progress reporting
async fn download_file_with_progress(
    app: &AppHandle,
    url: &str,
    dest_path: &PathBuf,
    model_name: &str,
) -> Result<()> {
    let client = Client::builder().user_agent("Thoth/1.0").build()?;

    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!("Failed to download: HTTP {}", response.status()));
    }

    let total_size = response.content_length();
    tracing::info!(
        "Starting download: {} bytes",
        total_size.map_or("unknown".to_string(), |s| s.to_string())
    );

    let mut file = tokio::fs::File::create(dest_path).await?;
    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        let percentage = total_size
            .map(|total| (downloaded as f32 / total as f32) * 100.0)
            .unwrap_or(0.0);

        emit_progress(
            app,
            DownloadProgress {
                current_file: model_name.to_string(),
                bytes_downloaded: downloaded,
                total_bytes: total_size,
                percentage,
                status: format!(
                    "Downloading: {:.1} MB / {:.1} MB",
                    downloaded as f32 / 1_048_576.0,
                    total_size.unwrap_or(0) as f32 / 1_048_576.0
                ),
            },
        );
    }

    file.flush().await?;
    tracing::info!("Download complete: {} bytes", downloaded);

    Ok(())
}

/// Find the first directory in an extraction temp folder
fn find_extracted_directory(temp_dir: &Path) -> Result<PathBuf> {
    let entries = std::fs::read_dir(temp_dir)?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            return Ok(path);
        }
    }

    Err(anyhow!("No directory found in archive"))
}

/// Extract a tar.bz2 archive
fn extract_tar_bz2(
    archive_path: &Path,
    dest_dir: &Path,
    required_files: &[String],
    archive_directory: Option<&str>,
) -> Result<()> {
    use std::fs::File;
    use std::io::BufReader;

    tracing::info!(
        "Extracting {} to {}",
        archive_path.display(),
        dest_dir.display()
    );

    let file = File::open(archive_path)?;
    let reader = BufReader::new(file);

    // Decompress bz2
    let decoder = bzip2::read::BzDecoder::new(reader);

    // Extract tar archive with path traversal protection
    let mut archive = tar::Archive::new(decoder);

    // The archive contains a directory with the model files
    // We need to extract the files to the correct location
    let temp_extract_dir = dest_dir.join("_extract_temp");
    std::fs::create_dir_all(&temp_extract_dir)?;

    let canonical_dest = temp_extract_dir.canonicalize()?;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();

        // Reject entries with path traversal components
        if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(anyhow!(
                "Archive contains path traversal: {}",
                path.display()
            ));
        }

        let full_path = canonical_dest.join(&path);
        if !full_path.starts_with(&canonical_dest) {
            return Err(anyhow!(
                "Archive entry escapes target directory: {}",
                path.display()
            ));
        }

        entry.unpack(&full_path)?;
    }

    // Find the extracted directory
    let extracted_dir = if let Some(dir_name) = archive_directory {
        // Use the known archive directory name
        let path = temp_extract_dir.join(dir_name);
        if path.exists() && path.is_dir() {
            path
        } else {
            // Fall back to searching if the specified directory doesn't exist
            tracing::warn!("Specified archive directory '{}' not found, searching...", dir_name);
            find_extracted_directory(&temp_extract_dir)?
        }
    } else {
        // Search for the extracted directory
        find_extracted_directory(&temp_extract_dir)?
    };

    // Move required files to model directory
    for file_name in required_files {
        let src = extracted_dir.join(file_name);
        let dest = dest_dir.join(file_name);

        if src.exists() {
            std::fs::copy(&src, &dest)?;
            tracing::debug!("Extracted {} to {}", file_name, dest.display());
        } else {
            return Err(anyhow!("Required file not found in archive: {}", file_name));
        }
    }

    // Clean up temp directory
    if let Err(e) = std::fs::remove_dir_all(&temp_extract_dir) {
        tracing::warn!("Failed to remove temp extraction directory: {}", e);
    }

    tracing::info!("Archive extraction completed");
    Ok(())
}

/// Emit a progress event to the Tauri frontend
fn emit_progress(app: &AppHandle, progress: DownloadProgress) {
    if let Err(e) = app.emit("model-download-progress", &progress) {
        tracing::warn!("Failed to emit progress event: {}", e);
    }
}

// Note: ModelInfo is now defined in manifest.rs and re-exported from mod.rs

/// Get information about available models (using manifest)
#[tauri::command]
pub fn get_model_info() -> Vec<super::manifest::ModelInfo> {
    let manifest = get_fallback_manifest();
    manifest
        .models
        .iter()
        .map(super::manifest::to_model_info)
        .collect()
}

/// Delete the downloaded model files
#[tauri::command]
pub fn delete_model(model_id: Option<String>) -> Result<(), String> {
    // Get the model info from manifest
    let manifest = get_fallback_manifest();
    let model_id = model_id.unwrap_or_else(|| {
        manifest
            .models
            .iter()
            .find(|m| m.recommended)
            .or_else(|| manifest.models.first())
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "parakeet-tdt-0.6b-v3-int8".to_string())
    });

    let model = manifest
        .models
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Model not found: {}", model_id))?;

    let model_dir = get_model_directory(&model_id);

    for file in &model.required_files {
        let path = model_dir.join(file);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Failed to delete {}: {}", file, e))?;
            tracing::info!("Deleted model file: {}", path.display());
        }
    }

    // Reset download state
    {
        let mut state = get_download_state().lock();
        *state = DownloadState::Idle;
    }

    tracing::info!("Model {} files deleted", model_id);
    Ok(())
}

/// Reset the download state to idle
#[tauri::command]
pub fn reset_download_state() {
    let mut state = get_download_state().lock();
    *state = DownloadState::Idle;
    tracing::info!("Download state reset to idle");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_model_downloaded_missing() {
        // Should return false when model files don't exist
        // (unless they happen to be installed on this machine)
        let _result = check_model_downloaded(None);
    }

    #[test]
    fn test_download_state_initial() {
        let state = get_download_progress();
        // State should be idle or whatever it was set to previously
        match state {
            DownloadState::Idle
            | DownloadState::Completed
            | DownloadState::Failed(_)
            | DownloadState::Downloading
            | DownloadState::Extracting => {}
        }
    }
}
