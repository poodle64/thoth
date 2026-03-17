//! AI text enhancement subsystem
//!
//! Provides AI-powered text enhancement using local Ollama models or any
//! OpenAI-compatible server, with context capture support for clipboard and
//! selected text.

pub mod anthropic;
pub mod context;
pub mod ollama;
pub mod openai_compat;
pub mod prompts;

pub use anthropic::AnthropicClient;
pub use context::{
    build_context, build_enhancement_context, get_clipboard_context, ContextCapture,
};
pub use ollama::OllamaClient;
pub use openai_compat::OpenAiCompatClient;
pub use prompts::{
    delete_custom_prompt_cmd, get_all_prompts, get_builtin_prompts_cmd, get_custom_prompts_cmd,
    get_prompt_by_id, save_custom_prompt_cmd, PromptTemplate,
};

use parking_lot::Mutex;
use std::sync::OnceLock;

/// Which AI backend to use for enhancement
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Ollama,
    OpenAiCompat,
    Anthropic,
}

impl BackendType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "openai_compat" => BackendType::OpenAiCompat,
            "anthropic" => BackendType::Anthropic,
            _ => BackendType::Ollama,
        }
    }
}

/// Holds the active backend configuration
#[derive(Debug, Clone)]
struct EnhancementBackend {
    backend_type: BackendType,
    ollama: OllamaClient,
    openai_compat: Option<OpenAiCompatClient>,
    anthropic: Option<AnthropicClient>,
}

impl Default for EnhancementBackend {
    fn default() -> Self {
        Self {
            backend_type: BackendType::Ollama,
            ollama: OllamaClient::new(),
            openai_compat: None,
            anthropic: None,
        }
    }
}

/// Global backend instance
static BACKEND: OnceLock<Mutex<EnhancementBackend>> = OnceLock::new();

fn get_backend() -> &'static Mutex<EnhancementBackend> {
    BACKEND.get_or_init(|| Mutex::new(EnhancementBackend::default()))
}

/// Configure the enhancement backend. Called when config is applied.
pub fn configure_backend(
    backend: &str,
    base_url: &str,
    api_key: Option<&str>,
    anthropic_model: Option<&str>,
    anthropic_base_url: Option<&str>,
) {
    let backend_type = BackendType::from_str(backend);
    let mut state = get_backend().lock();

    state.backend_type = backend_type;

    match backend_type {
        BackendType::Ollama => {
            state.ollama = OllamaClient::with_base_url(base_url.to_string());
        }
        BackendType::OpenAiCompat => {
            state.openai_compat = Some(OpenAiCompatClient::new(
                base_url,
                api_key.map(|k| k.to_string()),
            ));
        }
        BackendType::Anthropic => {
            if let Some(key) = api_key.filter(|k| !k.is_empty()) {
                state.anthropic = Some(AnthropicClient::new(
                    key.to_string(),
                    anthropic_model
                        .unwrap_or("claude-haiku-4-5-20251001")
                        .to_string(),
                    anthropic_base_url.map(|u| u.to_string()),
                ));
            }
        }
    }

    tracing::info!("Enhancement backend configured: {:?}", backend_type);
}

/// Check if the AI server is available
#[tauri::command]
pub async fn check_ollama_available() -> bool {
    let state = get_backend().lock().clone();
    match state.backend_type {
        BackendType::Ollama => state.ollama.is_available().await,
        BackendType::OpenAiCompat => match &state.openai_compat {
            Some(client) => client.is_available().await,
            None => false,
        },
        BackendType::Anthropic => match &state.anthropic {
            Some(client) => client.is_available().await,
            None => false,
        },
    }
}

/// List available models from the active backend
#[tauri::command]
pub async fn list_ollama_models() -> Result<Vec<String>, String> {
    let state = get_backend().lock().clone();

    match state.backend_type {
        BackendType::Ollama => state.ollama.list_models().await.map_err(|e| {
            tracing::error!("Failed to list Ollama models: {}", e);
            format!("Failed to list models: {}", e)
        }),
        BackendType::OpenAiCompat => match &state.openai_compat {
            Some(client) => client.list_models().await.map_err(|e| {
                tracing::error!("Failed to list OpenAI-compat models: {}", e);
                format!("Failed to list models: {}", e)
            }),
            None => Err("OpenAI-compatible backend not configured".to_string()),
        },
        BackendType::Anthropic => Ok(vec![
            "claude-haiku-4-5-20251001".to_string(),
            "claude-sonnet-4-6".to_string(),
            "claude-opus-4-6".to_string(),
        ]),
    }
}

/// Enhance text using the active backend
#[tauri::command]
pub async fn enhance_text(text: String, model: String, prompt: String) -> Result<String, String> {
    if text.is_empty() {
        return Err("Text cannot be empty".to_string());
    }

    if model.is_empty() {
        return Err("Model cannot be empty".to_string());
    }

    let state = get_backend().lock().clone();

    tracing::info!(
        "Enhancing text with model '{}' ({} characters, backend: {:?})",
        model,
        text.len(),
        state.backend_type
    );

    let result = match state.backend_type {
        BackendType::Ollama => state
            .ollama
            .enhance_text(&text, &model, &prompt)
            .await
            .map_err(|e| {
                tracing::error!("Enhancement failed: {}", e);
                format!("Enhancement failed: {}", e)
            })?,
        BackendType::OpenAiCompat => match &state.openai_compat {
            Some(client) => client
                .enhance_text(&text, &model, &prompt)
                .await
                .map_err(|e| {
                    tracing::error!("Enhancement failed: {}", e);
                    format!("Enhancement failed: {}", e)
                })?,
            None => return Err("OpenAI-compatible backend not configured".to_string()),
        },
        BackendType::Anthropic => match &state.anthropic {
            Some(client) => client
                .enhance_text(&text, &prompt)
                .await
                .map_err(|e| {
                    tracing::error!("Anthropic enhancement failed: {}", e);
                    format!("Enhancement failed: {}", e)
                })?,
            None => {
                return Err(
                    "Anthropic backend not configured. Please add your API key.".to_string(),
                )
            }
        },
    };

    tracing::info!(
        "Enhancement complete ({} -> {} characters)",
        text.len(),
        result.len()
    );

    Ok(result)
}

/// Set the enhancement backend from the frontend
#[tauri::command]
pub fn set_enhancement_backend(
    backend: String,
    base_url: String,
    api_key: Option<String>,
    anthropic_api_key: Option<String>,
    anthropic_model: Option<String>,
    anthropic_base_url: Option<String>,
) -> Result<(), String> {
    let effective_key = if backend == "anthropic" {
        anthropic_api_key.as_deref()
    } else {
        api_key.as_deref()
    };
    configure_backend(
        &backend,
        &base_url,
        effective_key,
        anthropic_model.as_deref(),
        anthropic_base_url.as_deref(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_type_from_str() {
        assert_eq!(BackendType::from_str("ollama"), BackendType::Ollama);
        assert_eq!(
            BackendType::from_str("openai_compat"),
            BackendType::OpenAiCompat
        );
        assert_eq!(
            BackendType::from_str("anthropic"),
            BackendType::Anthropic
        );
        assert_eq!(BackendType::from_str("unknown"), BackendType::Ollama);
    }

    #[test]
    fn test_default_backend() {
        let backend = EnhancementBackend::default();
        assert_eq!(backend.backend_type, BackendType::Ollama);
        assert!(backend.openai_compat.is_none());
        assert!(backend.anthropic.is_none());
    }
}
