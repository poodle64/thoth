//! Telemetry and observability export
//!
//! Provides the `test_loki_connection` Tauri command used by the Settings
//! "Test connection" button, and re-exports helpers for structured telemetry
//! event emission.

use crate::error::Error;

/// Push a single synthetic telemetry line to the configured Loki endpoint.
///
/// Used by the Settings "Test connection" button to verify that the URL,
/// auth token, and tenant are correct before the user saves and restarts.
/// Returns `Ok(())` if Loki accepted the push (2xx response), or an error
/// string describing the failure.
#[tauri::command]
pub async fn test_loki_connection() -> Result<(), Error> {
    crate::ensure_crypto_provider();

    let cfg = crate::config::get_config().map_err(|e| format!("Failed to read config: {}", e))?;
    let loki_cfg = &cfg.logging;

    if !loki_cfg.remote_enabled || loki_cfg.loki_url.is_empty() {
        return Err("Loki remote logging is not configured".to_string().into());
    }

    let url = format!(
        "{}/loki/api/v1/push",
        loki_cfg.loki_url.trim_end_matches('/')
    );

    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    // Minimal Loki JSON push payload — no transcript content, just a synthetic event.
    let body = serde_json::json!({
        "streams": [{
            "stream": {
                "app": "thoth",
                "version": env!("CARGO_PKG_VERSION")
            },
            "values": [[now_ns.to_string(), "event=test_connection"]]
        }]
    });

    let mut req_builder = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body(body.to_string());

    // Authorization header value is used directly without logging.
    if !loki_cfg.loki_auth.0.is_empty() {
        req_builder = req_builder.header("Authorization", loki_cfg.loki_auth.0.as_str());
    }
    if let Some(tenant) = &loki_cfg.loki_tenant {
        req_builder = req_builder.header("X-Scope-OrgID", tenant.as_str());
    }

    let response = req_builder
        .send()
        .await
        .map_err(|e| format!("Failed to reach Loki at {}: {}", url, e))?;

    if response.status().is_success() {
        tracing::info!(
            target: "telemetry",
            event = "test_connection",
            "loki_test_connection_ok"
        );
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("Loki returned {}: {}", status, body).into())
    }
}
