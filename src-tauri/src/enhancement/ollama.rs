//! Ollama HTTP client for AI text enhancement
//!
//! Provides local AI enhancement via the Ollama API running at localhost:11434.
//! Supports retry with exponential backoff and configurable timeout.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

/// Default Ollama server address
const DEFAULT_OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Default timeout for API requests in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Maximum number of retry attempts
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// Base delay for exponential backoff in milliseconds
const BASE_RETRY_DELAY_MS: u64 = 100;

/// Request body for Ollama generate endpoint
#[derive(Debug, Serialize)]
struct GenerateRequest {
    model: String,
    prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

/// Response from Ollama generate endpoint (non-streaming)
#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
    #[serde(default)]
    #[allow(dead_code)]
    done: bool,
}

/// Response from Ollama tags endpoint
#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<ModelInfo>,
}

/// Model information from Ollama
#[derive(Debug, Deserialize)]
struct ModelInfo {
    name: String,
}

/// Error types for Ollama operations
#[derive(Debug, thiserror::Error)]
pub enum OllamaError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Request timeout after {0} seconds")]
    Timeout(u64),

    #[error("Server error ({status}): {message}")]
    ServerError { status: u16, message: String },

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("All {attempts} retry attempts failed: {last_error}")]
    RetriesExhausted { attempts: u32, last_error: String },
}

/// Ollama HTTP client for AI text enhancement
///
/// Supports configurable base URL, timeout, and retry logic with
/// exponential backoff for transient failures.
#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    client: reqwest::Client,
    timeout: Duration,
    default_model: Option<String>,
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OllamaClient {
    /// Create a new Ollama client with default settings
    pub fn new() -> Self {
        Self::with_config(DEFAULT_OLLAMA_BASE_URL, DEFAULT_TIMEOUT_SECS, None)
    }

    /// Create a new Ollama client with a custom base URL
    pub fn with_base_url(base_url: String) -> Self {
        Self::with_config(&base_url, DEFAULT_TIMEOUT_SECS, None)
    }

    /// Create a new Ollama client with full configuration
    ///
    /// # Arguments
    ///
    /// * `base_url` - The Ollama server base URL (e.g., "http://localhost:11434")
    /// * `timeout_secs` - Request timeout in seconds
    /// * `default_model` - Optional default model to use if not specified per-request
    pub fn with_config(base_url: &str, timeout_secs: u64, default_model: Option<String>) -> Self {
        let timeout = Duration::from_secs(timeout_secs);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: base_url.to_string(),
            client,
            timeout,
            default_model,
        }
    }

    /// Set the default model for this client
    pub fn set_default_model(&mut self, model: impl Into<String>) {
        self.default_model = Some(model.into());
    }

    /// Get the configured timeout
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Check if Ollama server is available
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        match self.client.get(&url).send().await {
            Ok(response) => response.status().is_success(),
            Err(e) => {
                tracing::debug!("Ollama not available: {}", e);
                false
            }
        }
    }

    /// List available models from Ollama
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to Ollama: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Ollama returned error status: {}",
                response.status()
            ));
        }

        let tags: TagsResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse Ollama response: {}", e))?;

        let model_names: Vec<String> = tags.models.into_iter().map(|m| m.name).collect();

        tracing::debug!("Found {} Ollama models", model_names.len());
        Ok(model_names)
    }

    /// Send a single generate request (internal helper)
    async fn send_generate_request(
        &self,
        request: &GenerateRequest,
    ) -> Result<String, OllamaError> {
        let url = format!("{}/api/generate", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    OllamaError::Timeout(self.timeout.as_secs())
                } else {
                    OllamaError::ConnectionFailed(e.to_string())
                }
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(OllamaError::ServerError { status, message });
        }

        let generate_response: GenerateResponse = response
            .json()
            .await
            .map_err(|e| OllamaError::ParseError(e.to_string()))?;

        Ok(generate_response.response)
    }

    /// Generate text using the specified model with retry logic
    ///
    /// Retries up to 3 times with exponential backoff (100ms, 200ms, 400ms).
    pub async fn generate(&self, model: &str, prompt: &str) -> Result<String> {
        self.generate_with_system(model, prompt, None, None).await
    }

    /// Generate text with a system prompt and optional temperature
    ///
    /// # Arguments
    ///
    /// * `model` - The model name to use
    /// * `prompt` - The user prompt text
    /// * `system_prompt` - Optional system prompt for context
    /// * `temperature` - Optional temperature (0.0 to 1.0)
    pub async fn generate_with_system(
        &self,
        model: &str,
        prompt: &str,
        system_prompt: Option<&str>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let request = GenerateRequest {
            model: model.to_string(),
            prompt: prompt.to_string(),
            system: system_prompt.map(|s| s.to_string()),
            temperature,
            stream: false,
        };

        tracing::debug!(
            "Sending generate request to Ollama with model: {} (system prompt: {})",
            model,
            system_prompt.is_some()
        );

        // Retry with exponential backoff
        let mut last_error: Option<OllamaError> = None;

        for attempt in 0..MAX_RETRY_ATTEMPTS {
            match self.send_generate_request(&request).await {
                Ok(response) => {
                    if attempt > 0 {
                        tracing::debug!("Request succeeded on attempt {}", attempt + 1);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    let is_retryable = match &e {
                        OllamaError::ConnectionFailed(_) | OllamaError::Timeout(_) => true,
                        OllamaError::ServerError { status, .. } => *status >= 500,
                        _ => false,
                    };

                    if !is_retryable || attempt == MAX_RETRY_ATTEMPTS - 1 {
                        tracing::error!("Ollama request failed (attempt {}): {}", attempt + 1, e);
                        last_error = Some(e);
                        break;
                    }

                    let delay_ms = BASE_RETRY_DELAY_MS * 2u64.pow(attempt);
                    tracing::warn!(
                        "Ollama request failed (attempt {}), retrying in {}ms: {}",
                        attempt + 1,
                        delay_ms,
                        e
                    );
                    last_error = Some(e);
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }

        Err(anyhow!(OllamaError::RetriesExhausted {
            attempts: MAX_RETRY_ATTEMPTS,
            last_error: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        }))
    }

    /// Enhance text using the specified model and prompt template
    ///
    /// The prompt template should contain `{text}` which will be replaced with the input text.
    pub async fn enhance_text(
        &self,
        text: &str,
        model: &str,
        prompt_template: &str,
    ) -> Result<String> {
        let full_prompt = prompt_template.replace("{text}", text);
        self.generate(model, &full_prompt).await
    }

    /// Enhance text with a system prompt
    ///
    /// This wraps the text in a TRANSCRIPT tag and uses the system prompt for context.
    pub async fn enhance_with_system(
        &self,
        text: &str,
        model: &str,
        system_prompt: &str,
    ) -> Result<String> {
        let prompt = format!("<TRANSCRIPT>\n{}\n</TRANSCRIPT>", text);
        self.generate_with_system(model, &prompt, Some(system_prompt), Some(0.3))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = OllamaClient::new();
        assert_eq!(client.base_url, DEFAULT_OLLAMA_BASE_URL);
        assert_eq!(client.timeout.as_secs(), DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn test_client_with_custom_url() {
        let custom_url = "http://custom:8080".to_string();
        let client = OllamaClient::with_base_url(custom_url.clone());
        assert_eq!(client.base_url, custom_url);
    }

    #[test]
    fn test_client_with_config() {
        let client =
            OllamaClient::with_config("http://example.com:11434", 60, Some("llama3.2".to_string()));
        assert_eq!(client.base_url, "http://example.com:11434");
        assert_eq!(client.timeout.as_secs(), 60);
        assert_eq!(client.default_model, Some("llama3.2".to_string()));
    }

    #[test]
    fn test_default_impl() {
        let client = OllamaClient::default();
        assert_eq!(client.base_url, DEFAULT_OLLAMA_BASE_URL);
    }

    #[test]
    fn test_generate_request_serialisation_basic() {
        let request = GenerateRequest {
            model: "llama3.2".to_string(),
            prompt: "test prompt".to_string(),
            system: None,
            temperature: None,
            stream: false,
        };

        let json = serde_json::to_string(&request).expect("Failed to serialise");
        assert!(json.contains("\"model\":\"llama3.2\""));
        assert!(json.contains("\"stream\":false"));
        // system and temperature should be omitted when None
        assert!(!json.contains("\"system\""));
        assert!(!json.contains("\"temperature\""));
    }

    #[test]
    fn test_generate_request_serialisation_with_system() {
        let request = GenerateRequest {
            model: "llama3.2".to_string(),
            prompt: "test prompt".to_string(),
            system: Some("You are a helpful assistant.".to_string()),
            temperature: Some(0.3),
            stream: false,
        };

        let json = serde_json::to_string(&request).expect("Failed to serialise");
        assert!(json.contains("\"system\":\"You are a helpful assistant.\""));
        assert!(json.contains("\"temperature\":0.3"));
    }

    #[test]
    fn test_error_display() {
        let err = OllamaError::ConnectionFailed("connection refused".to_string());
        assert_eq!(err.to_string(), "Connection failed: connection refused");

        let err = OllamaError::Timeout(30);
        assert_eq!(err.to_string(), "Request timeout after 30 seconds");

        let err = OllamaError::ServerError {
            status: 500,
            message: "Internal error".to_string(),
        };
        assert_eq!(err.to_string(), "Server error (500): Internal error");

        let err = OllamaError::RetriesExhausted {
            attempts: 3,
            last_error: "timeout".to_string(),
        };
        assert_eq!(err.to_string(), "All 3 retry attempts failed: timeout");
    }

    #[test]
    fn test_set_default_model() {
        let mut client = OllamaClient::new();
        assert!(client.default_model.is_none());

        client.set_default_model("llama3.2");
        assert_eq!(client.default_model, Some("llama3.2".to_string()));
    }

    #[test]
    fn test_timeout_getter() {
        let client = OllamaClient::with_config("http://localhost:11434", 45, None);
        assert_eq!(client.timeout().as_secs(), 45);
    }
}
