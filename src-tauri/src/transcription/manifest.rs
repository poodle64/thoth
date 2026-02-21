//! Remote model manifest for dynamic model discovery
//!
//! Fetches model information from a remote JSON manifest to keep
//! the model list up-to-date without requiring app updates.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// URL for the model manifest (can be changed to your own hosting)
const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/poodle64/thoth/main/models/manifest.json";

/// Cache duration for the manifest (24 hours)
const MANIFEST_CACHE_HOURS: u64 = 24;

/// Model manifest containing all available models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    /// Manifest version (for future compatibility)
    pub version: u32,
    /// Last updated timestamp (ISO 8601)
    pub updated: String,
    /// List of available models
    pub models: Vec<RemoteModelInfo>,
}

/// Information about a model from the remote manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteModelInfo {
    /// Unique identifier for the model (e.g., "parakeet-tdt-0.6b-v3-int8")
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of the model's capabilities
    pub description: String,
    /// Model version string
    pub version: String,
    /// Download URL for the model archive
    pub download_url: String,
    /// Expected download size in bytes
    pub download_size: u64,
    /// Expected extracted size in bytes
    pub extracted_size: u64,
    /// SHA256 checksum of the archive (for verification)
    pub sha256: Option<String>,
    /// Required files that must exist after extraction
    pub required_files: Vec<String>,
    /// Directory name inside the archive (for extraction)
    pub archive_directory: Option<String>,
    /// Supported languages (empty = all)
    pub languages: Vec<String>,
    /// Model type (e.g., "transducer", "ctc")
    pub model_type: String,
    /// Whether this is the recommended/default model
    #[serde(default)]
    pub recommended: bool,
    /// Minimum app version required (semver)
    pub min_app_version: Option<String>,
}

/// Combined model info for the frontend (remote + local status)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model ID
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Model version
    pub version: String,
    /// Download size in MB (approximate)
    pub size_mb: u32,
    /// Whether the model is downloaded locally
    pub downloaded: bool,
    /// Path to the model directory
    pub path: String,
    /// Actual size on disk in bytes (if downloaded)
    pub disk_size: Option<u64>,
    /// Whether this is the recommended model
    pub recommended: bool,
    /// Supported languages
    pub languages: Vec<String>,
    /// Whether an update is available
    pub update_available: bool,
    /// Whether this is the currently selected model
    pub selected: bool,
    /// Model type (e.g., "whisper_ggml", "nemo_transducer")
    pub model_type: String,
    /// Whether this model's backend is available in the current build
    pub backend_available: bool,
}

/// Cached manifest with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedManifest {
    /// When the manifest was fetched
    fetched_at: u64,
    /// The manifest data
    manifest: ModelManifest,
}

/// Get the manifest cache file path
fn get_cache_path() -> PathBuf {
    home_dir_or_fallback()
        .join(".thoth")
        .join("models")
        .join("manifest_cache.json")
}

fn home_dir_or_fallback() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        tracing::error!("Could not determine home directory, using /tmp");
        PathBuf::from("/tmp")
    })
}

/// Load cached manifest if it exists and is not expired
fn load_cached_manifest() -> Option<ModelManifest> {
    let cache_path = get_cache_path();
    if !cache_path.exists() {
        return None;
    }

    let data = std::fs::read_to_string(&cache_path).ok()?;
    let cached: CachedManifest = serde_json::from_str(&data).ok()?;

    // Check if cache is expired
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();

    let cache_age_hours = (now - cached.fetched_at) / 3600;
    if cache_age_hours >= MANIFEST_CACHE_HOURS {
        tracing::debug!("Manifest cache expired ({} hours old)", cache_age_hours);
        return None;
    }

    tracing::debug!("Using cached manifest ({} hours old)", cache_age_hours);
    Some(cached.manifest)
}

/// Save manifest to cache
fn save_manifest_cache(manifest: &ModelManifest) -> Result<()> {
    let cache_path = get_cache_path();

    // Ensure parent directory exists
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let cached = CachedManifest {
        fetched_at: now,
        manifest: manifest.clone(),
    };

    let data = serde_json::to_string_pretty(&cached)?;
    std::fs::write(&cache_path, data)?;

    tracing::debug!("Saved manifest to cache");
    Ok(())
}

/// Fetch the model manifest from the remote URL
pub async fn fetch_manifest(force_refresh: bool) -> Result<ModelManifest> {
    // Try cache first unless forcing refresh
    if !force_refresh {
        if let Some(cached) = load_cached_manifest() {
            return Ok(cached);
        }
    }

    tracing::info!("Fetching model manifest from {}", MANIFEST_URL);

    let client = reqwest::Client::builder()
        .user_agent("Thoth/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client.get(MANIFEST_URL).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to fetch manifest: HTTP {}",
            response.status()
        ));
    }

    let manifest: ModelManifest = response.json().await?;

    // Cache the manifest
    if let Err(e) = save_manifest_cache(&manifest) {
        tracing::warn!("Failed to cache manifest: {}", e);
    }

    tracing::info!(
        "Fetched manifest v{} with {} models",
        manifest.version,
        manifest.models.len()
    );

    Ok(manifest)
}

/// Bundled manifest JSON (embedded at compile time)
const BUNDLED_MANIFEST: &str = include_str!("../../../models/manifest.json");

/// Get the default/fallback manifest when remote is unavailable
pub fn get_fallback_manifest() -> ModelManifest {
    // Parse the bundled manifest
    match serde_json::from_str::<ModelManifest>(BUNDLED_MANIFEST) {
        Ok(manifest) => {
            tracing::info!(
                "Using bundled manifest v{} with {} models",
                manifest.version,
                manifest.models.len()
            );
            manifest
        }
        Err(e) => {
            tracing::error!("Failed to parse bundled manifest: {}. Using minimal fallback.", e);
            // Minimal fallback if bundled manifest is somehow corrupted
            let now = chrono::Utc::now().to_rfc3339();
            ModelManifest {
                version: 3,
                updated: now,
                models: vec![RemoteModelInfo {
                    id: "parakeet-tdt-0.6b-v2-int8".to_string(),
                    name: "Parakeet TDT 0.6B V2 (int8)".to_string(),
                    description: "Best English model - State-of-the-art 6.05% average WER.".to_string(),
                    version: "2.0.0".to_string(),
                    download_url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2".to_string(),
                    download_size: 482_468_385,
                    extracted_size: 661_190_513,
                    sha256: None,
                    required_files: vec![
                        "encoder.int8.onnx".to_string(),
                        "decoder.int8.onnx".to_string(),
                        "joiner.int8.onnx".to_string(),
                        "tokens.txt".to_string(),
                    ],
                    archive_directory: Some("sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8".to_string()),
                    languages: vec!["en".to_string()],
                    model_type: "nemo_transducer".to_string(),
                    recommended: true,
                    min_app_version: None,
                }],
            }
        }
    }
}

/// Check if a specific model is downloaded locally
pub fn is_model_downloaded(model: &RemoteModelInfo) -> bool {
    let model_dir = get_model_directory(&model.id);

    for file in &model.required_files {
        let path = model_dir.join(file);
        if !path.exists() {
            return false;
        }

        // Verify file has content
        if let Ok(metadata) = std::fs::metadata(&path) {
            if metadata.len() == 0 {
                return false;
            }
        } else {
            return false;
        }
    }

    true
}

/// Get the directory for a specific model
pub fn get_model_directory(model_id: &str) -> PathBuf {
    // Use model ID as directory name (sanitized)
    let safe_id = model_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    home_dir_or_fallback().join(".thoth").join("models").join(safe_id)
}

/// Get disk size for a downloaded model
pub fn get_model_disk_size(model: &RemoteModelInfo) -> Option<u64> {
    let model_dir = get_model_directory(&model.id);

    model
        .required_files
        .iter()
        .filter_map(|file| {
            std::fs::metadata(model_dir.join(file))
                .ok()
                .map(|m| m.len())
        })
        .reduce(|a, b| a + b)
}

/// Check if the backend for a given model type is available in this build
pub fn is_backend_available(model_type: &str) -> bool {
    match model_type {
        "whisper_ggml" => true,
        "nemo_transducer" => cfg!(feature = "parakeet"),
        "fluidaudio_coreml" => cfg!(all(target_os = "macos", feature = "fluidaudio")),
        _ => false,
    }
}

/// Convert remote model info to frontend model info
pub fn to_model_info(remote: &RemoteModelInfo, selected_id: Option<&str>) -> ModelInfo {
    let downloaded = is_model_downloaded(remote);
    let disk_size = if downloaded {
        get_model_disk_size(remote)
    } else {
        None
    };

    let selected = selected_id
        .map(|id| id == remote.id)
        .unwrap_or(remote.recommended);

    // For FluidAudio models, show the actual cache directory
    let path = if remote.model_type == "fluidaudio_coreml" {
        #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
        {
            super::fluidaudio::model_cache_directory()
                .to_string_lossy()
                .to_string()
        }
        #[cfg(not(all(target_os = "macos", feature = "fluidaudio")))]
        {
            get_model_directory(&remote.id)
                .to_string_lossy()
                .to_string()
        }
    } else {
        get_model_directory(&remote.id)
            .to_string_lossy()
            .to_string()
    };

    ModelInfo {
        id: remote.id.clone(),
        name: remote.name.clone(),
        description: remote.description.clone(),
        version: remote.version.clone(),
        size_mb: (remote.download_size / (1024 * 1024)) as u32,
        downloaded,
        path,
        disk_size,
        recommended: remote.recommended,
        languages: remote.languages.clone(),
        update_available: false, // TODO: Implement version comparison
        selected,
        model_type: remote.model_type.clone(),
        backend_available: is_backend_available(&remote.model_type),
    }
}

/// Tauri command: Fetch model manifest
///
/// Uses the higher-versioned manifest between remote and bundled, so that
/// new models added in app updates are visible even before the remote
/// manifest on GitHub is updated.
#[tauri::command]
pub async fn fetch_model_manifest(force_refresh: bool) -> Result<Vec<ModelInfo>, String> {
    let remote_manifest = match fetch_manifest(force_refresh).await {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!("Failed to fetch remote manifest: {}", e);
            None
        }
    };

    let bundled = get_fallback_manifest();

    let manifest = match remote_manifest {
        Some(remote) if remote.version >= bundled.version => remote,
        Some(remote) => {
            tracing::info!(
                "Bundled manifest v{} is newer than remote v{}, using bundled",
                bundled.version,
                remote.version
            );
            bundled
        }
        None => {
            tracing::info!("Using bundled manifest v{}", bundled.version);
            bundled
        }
    };

    let selected_id = crate::config::get_config()
        .ok()
        .and_then(|c| c.transcription.model_id.clone());

    let models: Vec<ModelInfo> = manifest
        .models
        .iter()
        .map(|m| to_model_info(m, selected_id.as_deref()))
        .collect();

    Ok(models)
}

/// Tauri command: Get manifest last update time
#[tauri::command]
pub fn get_manifest_update_time() -> Option<String> {
    let cache_path = get_cache_path();
    if !cache_path.exists() {
        return None;
    }

    let data = std::fs::read_to_string(&cache_path).ok()?;
    let cached: CachedManifest = serde_json::from_str(&data).ok()?;

    // Convert timestamp to ISO 8601
    let datetime = chrono::DateTime::from_timestamp(cached.fetched_at as i64, 0)?;
    Some(datetime.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_manifest() {
        let manifest = get_fallback_manifest();
        assert_eq!(manifest.version, 8);
        assert_eq!(manifest.models.len(), 6);

        let model = &manifest.models[0];
        assert_eq!(model.id, "ggml-large-v3-turbo");
        assert!(model.recommended);
    }

    #[test]
    fn test_parakeet_models_in_manifest() {
        let manifest = get_fallback_manifest();
        let parakeet_models: Vec<_> = manifest
            .models
            .iter()
            .filter(|m| m.model_type == "nemo_transducer")
            .collect();
        assert_eq!(parakeet_models.len(), 2);
        assert!(parakeet_models.iter().any(|m| m.id == "parakeet-tdt-0.6b-v2-int8"));
        assert!(parakeet_models.iter().any(|m| m.id == "parakeet-tdt-0.6b-v3-int8"));
    }

    #[test]
    fn test_fluidaudio_model_in_manifest() {
        let manifest = get_fallback_manifest();
        let fa_model = manifest
            .models
            .iter()
            .find(|m| m.model_type == "fluidaudio_coreml");
        assert!(fa_model.is_some(), "FluidAudio model should be in manifest");
        let fa = fa_model.unwrap();
        assert_eq!(fa.id, "fluidaudio-parakeet-tdt-coreml");
        assert_eq!(fa.required_files, vec![".fluidaudio_ready"]);
        assert!(!fa.recommended);
    }

    #[test]
    fn test_backend_availability() {
        assert!(is_backend_available("whisper_ggml"));
        assert!(!is_backend_available("unknown_type"));
    }

    #[test]
    fn test_model_directory() {
        let dir = get_model_directory("parakeet-tdt-0.6b-v3-int8");
        assert!(dir.to_string_lossy().contains(".thoth"));
        assert!(dir.to_string_lossy().contains("models"));
    }

    #[test]
    fn test_to_model_info() {
        let remote = RemoteModelInfo {
            id: "test-model".to_string(),
            name: "Test Model".to_string(),
            description: "A test model".to_string(),
            version: "1.0.0".to_string(),
            download_url: "https://example.com/model.tar.bz2".to_string(),
            download_size: 100 * 1024 * 1024,
            extracted_size: 110 * 1024 * 1024,
            sha256: None,
            required_files: vec!["model.onnx".to_string()],
            archive_directory: None,
            languages: vec!["en".to_string()],
            model_type: "test".to_string(),
            recommended: false,
            min_app_version: None,
        };

        let info = to_model_info(&remote, None);
        assert_eq!(info.id, "test-model");
        assert_eq!(info.size_mb, 100);
        assert!(!info.downloaded);
        assert!(!info.selected);
        assert_eq!(info.model_type, "test");
        assert!(!info.backend_available);

        let info_selected = to_model_info(&remote, Some("test-model"));
        assert!(info_selected.selected);
    }
}
