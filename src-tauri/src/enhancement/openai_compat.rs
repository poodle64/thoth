//! OpenAI-compatible HTTP client for AI text enhancement
//!
//! Provides AI enhancement via any OpenAI-compatible API server (oMLX, LM Studio,
//! LocalAI, Ollama OpenAI-compat mode, etc.) using the `/v1/chat/completions`
//! endpoint. Supports optional Bearer token authentication and retry with
//! exponential backoff.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

/// Default timeout for API requests in seconds
/// Set to 120s to accommodate local models (e.g. oMLX) that may take longer
/// to generate a response — especially when the model is still warming up.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Maximum number of retry attempts
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// Base delay for exponential backoff in milliseconds
const BASE_RETRY_DELAY_MS: u64 = 100;

// ── Request / response types ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

// ── Error types ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum OpenAiCompatError {
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

// ── Client ──────────────────────────────────────────────────────────────────

/// OpenAI-compatible HTTP client for AI text enhancement
#[derive(Debug, Clone)]
pub struct OpenAiCompatClient {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
    timeout: Duration,
}

impl OpenAiCompatClient {
    /// Create a new client with the given base URL and optional API key.
    pub fn new(base_url: &str, api_key: Option<String>) -> Self {
        Self::with_timeout(base_url, api_key, DEFAULT_TIMEOUT_SECS)
    }

    /// Create a new client with a custom timeout.
    pub fn with_timeout(base_url: &str, api_key: Option<String>, timeout_secs: u64) -> Self {
        let timeout = Duration::from_secs(timeout_secs);
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            client,
            timeout,
        }
    }

    /// Check if the server is available by hitting `/v1/models`.
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/v1/models", self.base_url);
        let mut req = self.client.get(&url);
        if let Some(key) = self.effective_api_key() {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        match req.send().await {
            Ok(response) => response.status().is_success(),
            Err(e) => {
                tracing::debug!("OpenAI-compat server not available: {}", e);
                false
            }
        }
    }

    /// List available models via `/v1/models`.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/v1/models", self.base_url);
        let mut req = self.client.get(&url);
        if let Some(key) = self.effective_api_key() {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req
            .send()
            .await
            .map_err(|e| anyhow!("Failed to connect to OpenAI-compat server: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Server returned error status: {}",
                response.status()
            ));
        }

        let models: ModelsResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse models response: {}", e))?;

        let ids: Vec<String> = models.data.into_iter().map(|m| m.id).collect();
        tracing::debug!("Found {} models via OpenAI-compat API", ids.len());
        Ok(ids)
    }

    /// Enhance text using a prompt template containing `{text}`.
    pub async fn enhance_text(
        &self,
        text: &str,
        model: &str,
        prompt_template: &str,
    ) -> Result<String> {
        let full_prompt = prompt_template.replace("{text}", text);
        self.chat(model, &full_prompt, None, None).await
    }

    /// Enhance text with a system prompt (wraps text in TRANSCRIPT tags).
    pub async fn enhance_with_system(
        &self,
        text: &str,
        model: &str,
        system_prompt: &str,
    ) -> Result<String> {
        let user_msg = format!("<TRANSCRIPT>\n{}\n</TRANSCRIPT>", text);
        self.chat(model, &user_msg, Some(system_prompt), Some(0.3))
            .await
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    /// Send a chat completion request with retry logic.
    async fn chat(
        &self,
        model: &str,
        user_message: &str,
        system_prompt: Option<&str>,
        temperature: Option<f32>,
    ) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: sys.to_string(),
            });
        }
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: user_message.to_string(),
        });

        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages,
            temperature,
            stream: false,
        };

        tracing::debug!(
            "Sending chat completion request with model: {} (system prompt: {})",
            model,
            system_prompt.is_some()
        );

        let mut last_error: Option<OpenAiCompatError> = None;

        for attempt in 0..MAX_RETRY_ATTEMPTS {
            match self.send_chat_request(&request).await {
                Ok(response) => {
                    if attempt > 0 {
                        tracing::debug!("Request succeeded on attempt {}", attempt + 1);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    let is_retryable = match &e {
                        OpenAiCompatError::ConnectionFailed(_)
                        | OpenAiCompatError::Timeout(_) => true,
                        OpenAiCompatError::ServerError { status, .. } => *status >= 500,
                        _ => false,
                    };

                    if !is_retryable || attempt == MAX_RETRY_ATTEMPTS - 1 {
                        tracing::error!(
                            "OpenAI-compat request failed (attempt {}): {}",
                            attempt + 1,
                            e
                        );
                        last_error = Some(e);
                        break;
                    }

                    let delay_ms = BASE_RETRY_DELAY_MS * 2u64.pow(attempt);
                    tracing::warn!(
                        "OpenAI-compat request failed (attempt {}), retrying in {}ms: {}",
                        attempt + 1,
                        delay_ms,
                        e
                    );
                    last_error = Some(e);
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }

        Err(anyhow!(OpenAiCompatError::RetriesExhausted {
            attempts: MAX_RETRY_ATTEMPTS,
            last_error: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        }))
    }

    /// Send a single chat completion request.
    async fn send_chat_request(
        &self,
        request: &ChatCompletionRequest,
    ) -> Result<String, OpenAiCompatError> {
        let url = format!("{}/v1/chat/completions", self.base_url);

        let mut req = self.client.post(&url).json(request);
        if let Some(key) = self.effective_api_key() {
            req = req.header("Authorization", format!("Bearer {}", key));
        }

        let response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                OpenAiCompatError::Timeout(self.timeout.as_secs())
            } else {
                OpenAiCompatError::ConnectionFailed(e.to_string())
            }
        })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(OpenAiCompatError::ServerError { status, message });
        }

        let chat_response: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| OpenAiCompatError::ParseError(e.to_string()))?;

        chat_response
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| OpenAiCompatError::ParseError("No choices in response".to_string()))
    }

    /// Return the API key only if it is non-empty.
    fn effective_api_key(&self) -> Option<&str> {
        self.api_key
            .as_deref()
            .filter(|k| !k.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = OpenAiCompatClient::new("http://localhost:8080", None);
        assert_eq!(client.base_url, "http://localhost:8080");
        assert!(client.api_key.is_none());
    }

    #[test]
    fn test_client_strips_trailing_slash() {
        let client = OpenAiCompatClient::new("http://localhost:8080/", None);
        assert_eq!(client.base_url, "http://localhost:8080");
    }

    #[test]
    fn test_client_with_api_key() {
        let client =
            OpenAiCompatClient::new("http://localhost:8080", Some("sk-test123".to_string()));
        assert_eq!(client.api_key, Some("sk-test123".to_string()));
    }

    #[test]
    fn test_effective_api_key_none() {
        let client = OpenAiCompatClient::new("http://localhost:8080", None);
        assert!(client.effective_api_key().is_none());
    }

    #[test]
    fn test_effective_api_key_empty() {
        let client = OpenAiCompatClient::new("http://localhost:8080", Some("".to_string()));
        assert!(client.effective_api_key().is_none());
    }

    #[test]
    fn test_effective_api_key_present() {
        let client =
            OpenAiCompatClient::new("http://localhost:8080", Some("sk-test".to_string()));
        assert_eq!(client.effective_api_key(), Some("sk-test"));
    }

    #[test]
    fn test_chat_request_serialisation() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            temperature: Some(0.3),
            stream: false,
        };

        let json = serde_json::to_string(&request).expect("Failed to serialise");
        assert!(json.contains("\"model\":\"gpt-4\""));
        assert!(json.contains("\"stream\":false"));
        assert!(json.contains("\"temperature\":0.3"));
    }

    #[test]
    fn test_chat_request_no_temperature() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            temperature: None,
            stream: false,
        };

        let json = serde_json::to_string(&request).expect("Failed to serialise");
        assert!(!json.contains("\"temperature\""));
    }

    #[test]
    fn test_error_display() {
        let err = OpenAiCompatError::ConnectionFailed("connection refused".to_string());
        assert_eq!(err.to_string(), "Connection failed: connection refused");

        let err = OpenAiCompatError::Timeout(30);
        assert_eq!(err.to_string(), "Request timeout after 30 seconds");

        let err = OpenAiCompatError::ServerError {
            status: 401,
            message: "Unauthorized".to_string(),
        };
        assert_eq!(err.to_string(), "Server error (401): Unauthorized");

        let err = OpenAiCompatError::RetriesExhausted {
            attempts: 3,
            last_error: "timeout".to_string(),
        };
        assert_eq!(err.to_string(), "All 3 retry attempts failed: timeout");
    }
}
