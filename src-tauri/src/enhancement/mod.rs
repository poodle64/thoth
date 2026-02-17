//! AI text enhancement subsystem
//!
//! Provides AI-powered text enhancement using local Ollama models,
//! with context capture support for clipboard and selected text.

pub mod context;
pub mod ollama;
pub mod prompts;

pub use context::{
    build_context, build_enhancement_context, get_clipboard_context, ContextCapture,
};
pub use ollama::OllamaClient;
pub use prompts::{
    delete_custom_prompt_cmd, get_all_prompts, get_builtin_prompts_cmd, get_custom_prompts_cmd,
    get_prompt_by_id, save_custom_prompt_cmd, PromptTemplate,
};

use parking_lot::Mutex;
use std::sync::OnceLock;

/// Global Ollama client instance
static OLLAMA_CLIENT: OnceLock<Mutex<OllamaClient>> = OnceLock::new();

fn get_client() -> &'static Mutex<OllamaClient> {
    OLLAMA_CLIENT.get_or_init(|| Mutex::new(OllamaClient::new()))
}

/// Check if Ollama server is available
#[tauri::command]
pub async fn check_ollama_available() -> bool {
    let client = get_client().lock().clone();
    client.is_available().await
}

/// List available Ollama models
#[tauri::command]
pub async fn list_ollama_models() -> Result<Vec<String>, String> {
    let client = get_client().lock().clone();

    client.list_models().await.map_err(|e| {
        tracing::error!("Failed to list Ollama models: {}", e);
        format!("Failed to list models: {}", e)
    })
}

/// Enhance text using Ollama
///
/// The prompt should contain `{text}` which will be replaced with the input text.
#[tauri::command]
pub async fn enhance_text(text: String, model: String, prompt: String) -> Result<String, String> {
    if text.is_empty() {
        return Err("Text cannot be empty".to_string());
    }

    if model.is_empty() {
        return Err("Model cannot be empty".to_string());
    }

    let client = get_client().lock().clone();

    tracing::info!(
        "Enhancing text with model '{}' ({} characters)",
        model,
        text.len()
    );

    let result = client
        .enhance_text(&text, &model, &prompt)
        .await
        .map_err(|e| {
            tracing::error!("Enhancement failed: {}", e);
            format!("Enhancement failed: {}", e)
        })?;

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

    #[test]
    fn test_client_initialisation() {
        let client = get_client();
        let _guard = client.lock();
        // Client should be initialised without panicking
    }
}
