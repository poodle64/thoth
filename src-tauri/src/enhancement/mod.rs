//! AI text enhancement subsystem
//!
//! Provides AI-powered text enhancement using local LLM backends:
//! - Ollama (default)
//! - Any OpenAI-compatible endpoint (LM Studio, llama.cpp server, vLLM, etc.)

pub mod context;
pub mod ollama;
pub mod openai_compat;
pub mod prompts;

pub use context::{
    ContextCapture, build_context, build_enhancement_context, get_clipboard_context,
};
pub use ollama::OllamaClient;
pub use openai_compat::OpenAiCompatClient;
pub use prompts::{
    PromptTemplate, delete_custom_prompt_cmd, get_all_prompts, get_builtin_prompts_cmd,
    get_custom_prompts_cmd, get_prompt_by_id, save_custom_prompt_cmd,
};

use parking_lot::Mutex;
use std::sync::OnceLock;

/// Which enhancement backend is active
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Ollama,
    OpenAiCompat,
}

/// Holds the active backend configuration
struct EnhancementBackend {
    backend_type: BackendType,
    ollama: OllamaClient,
    openai_compat: Option<OpenAiCompatClient>,
}

impl EnhancementBackend {
    fn new() -> Self {
        Self {
            backend_type: BackendType::Ollama,
            ollama: OllamaClient::new(),
            openai_compat: None,
        }
    }
}

/// Global enhancement backend instance
static BACKEND: OnceLock<Mutex<EnhancementBackend>> = OnceLock::new();

fn get_backend() -> &'static Mutex<EnhancementBackend> {
    BACKEND.get_or_init(|| Mutex::new(EnhancementBackend::new()))
}

/// Configure the active enhancement backend.
///
/// Called on startup (after config load) and after `set_config` changes the
/// enhancement section. Must be called before the first pipeline run.
///
/// Returns the `BackendType` that was actually activated (callers may request
/// `openai_compat` but receive `Ollama` if the URL is invalid).
///
/// # Arguments
///
/// * `backend` - `"ollama"` or `"openai_compat"` (any other value defaults to Ollama)
/// * `ollama_url` - Ollama base URL (used when backend is Ollama)
/// * `openai_compat_url` - OpenAI-compat base URL (used when backend is openai_compat)
/// * `api_key` - Optional API key for the OpenAI-compat endpoint
pub fn configure_backend(
    backend: &str,
    ollama_url: &str,
    openai_compat_url: &str,
    api_key: Option<&str>,
) -> BackendType {
    let mut b = get_backend().lock();

    // Always update the Ollama client URL
    b.ollama = OllamaClient::with_base_url(ollama_url.to_string());

    match backend {
        "openai_compat" => {
            match OpenAiCompatClient::new(
                openai_compat_url.to_string(),
                api_key.map(|k| k.to_string()),
            ) {
                Ok(client) => {
                    b.backend_type = BackendType::OpenAiCompat;
                    b.openai_compat = Some(client);
                    tracing::info!(
                        "Enhancement backend: OpenAI-compat at {}",
                        openai_compat_url
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Invalid OpenAI-compat URL '{}', falling back to Ollama: {}",
                        openai_compat_url,
                        e
                    );
                    b.backend_type = BackendType::Ollama;
                    b.openai_compat = None;
                }
            }
        }
        _ => {
            b.backend_type = BackendType::Ollama;
            b.openai_compat = None;
            tracing::info!("Enhancement backend: Ollama at {}", ollama_url);
        }
    }

    b.backend_type
}

// --- Tauri Commands ---

/// Check if the Ollama server is available
#[tauri::command]
pub async fn check_ollama_available() -> bool {
    let client = get_backend().lock().ollama.clone();
    client.is_available().await
}

/// List available Ollama models
#[tauri::command]
pub async fn list_ollama_models() -> Result<Vec<String>, String> {
    let client = get_backend().lock().ollama.clone();
    client.list_models().await.map_err(|e| {
        tracing::error!("Failed to list Ollama models: {}", e);
        format!("Failed to list models: {}", e)
    })
}

/// Check if the configured OpenAI-compatible server is available
#[tauri::command]
pub async fn check_openai_compat_available() -> bool {
    let client = get_backend().lock().openai_compat.clone();
    match client {
        Some(c) => c.is_available().await,
        None => false,
    }
}

/// List available models from the OpenAI-compatible server
#[tauri::command]
pub async fn list_openai_compat_models() -> Result<Vec<String>, String> {
    let client = get_backend().lock().openai_compat.clone();
    match client {
        Some(c) => c.list_models().await.map_err(|e| {
            tracing::error!("Failed to list OpenAI-compat models: {}", e);
            format!("Failed to list models: {}", e)
        }),
        None => Err("OpenAI-compatible backend not configured".to_string()),
    }
}

/// Enhance text using the active backend.
///
/// The prompt template must contain `{text}`, which is substituted with the
/// transcript in-place before being sent as the sole user message. Both the
/// Ollama and OpenAI-compat backends use this single-message format.
///
/// The public signature `(text, model, prompt)` is unchanged; only internal
/// dispatch changed.
#[tauri::command]
pub async fn enhance_text(text: String, model: String, prompt: String) -> Result<String, String> {
    if text.is_empty() {
        return Err("Text cannot be empty".to_string());
    }

    if model.is_empty() {
        return Err("Model cannot be empty".to_string());
    }

    let (backend_type, ollama, openai_compat) = {
        let b = get_backend().lock();
        (b.backend_type, b.ollama.clone(), b.openai_compat.clone())
    };

    tracing::info!(
        "Enhancing text with model '{}' ({} chars, backend: {:?})",
        model,
        text.len(),
        backend_type
    );

    let result = match backend_type {
        BackendType::Ollama => ollama
            .enhance_text(&text, &model, &prompt)
            .await
            .map_err(|e| {
                tracing::error!("Ollama enhancement failed: {}", e);
                format!("Enhancement failed: {}", e)
            })?,
        BackendType::OpenAiCompat => {
            let client = openai_compat
                .ok_or_else(|| "OpenAI-compatible backend not configured".to_string())?;
            client
                .enhance_text(&text, &model, &prompt)
                .await
                .map_err(|e| {
                    tracing::error!("OpenAI-compat enhancement failed: {}", e);
                    format!("Enhancement failed: {}", e)
                })?
        }
    };

    tracing::info!(
        "Enhancement complete ({} -> {} characters)",
        text.len(),
        result.len()
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    /// Serialises tests that call configure_backend so they cannot interleave on
    /// the shared BACKEND singleton. Each test asserts only on the BackendType
    /// returned by configure_backend (which is the value it just wrote), so the
    /// guard is sufficient to avoid both write-write and read-write races.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    #[test]
    fn test_backend_initialises_as_ollama() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let bt = configure_backend(
            "ollama",
            "http://localhost:11434",
            "http://localhost:1234",
            None,
        );
        assert_eq!(bt, BackendType::Ollama);
    }

    #[test]
    fn test_configure_backend_switches_to_openai_compat() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let bt = configure_backend(
            "openai_compat",
            "http://localhost:11434",
            "http://localhost:1234",
            None,
        );
        assert_eq!(bt, BackendType::OpenAiCompat);
    }

    #[test]
    fn test_configure_backend_invalid_url_falls_back_to_ollama() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let bt = configure_backend(
            "openai_compat",
            "http://localhost:11434",
            "file:///bad-scheme",
            None,
        );
        assert_eq!(bt, BackendType::Ollama);
    }

    #[test]
    fn test_configure_backend_unknown_backend_defaults_to_ollama() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let bt = configure_backend(
            "unknown_backend",
            "http://localhost:11434",
            "http://localhost:1234",
            None,
        );
        assert_eq!(bt, BackendType::Ollama);
    }
}
