//! Telemetry and observability export
//!
//! Provides the `test_loki_connection` Tauri command used by the Settings
//! "Test connection" button, and re-exports helpers for structured telemetry
//! event emission.

use crate::error::Error;

/// Push a single synthetic telemetry line to the given Loki endpoint.
///
/// Used by the Settings "Test connection" button to verify that the URL,
/// auth token, and tenant are correct before the user saves and restarts.
/// Tests the VALUES currently on-screen rather than the saved config so the
/// user can test before/independently of saving. Returns `Ok(())` if Loki
/// accepted the push (2xx response), or an error string describing the failure.
#[tauri::command]
pub async fn test_loki_connection(
    url: String,
    auth: String,
    tenant: Option<String>,
) -> Result<(), Error> {
    crate::ensure_crypto_provider();

    if url.is_empty() {
        return Err("Loki URL is required".to_string().into());
    }

    // Accept either a bare base URL ("http://loki:3100") or one that already
    // ends with the push path so callers don't have to strip it themselves.
    let push_url = if url.ends_with("/loki/api/v1/push") {
        url.clone()
    } else {
        format!("{}/loki/api/v1/push", url.trim_end_matches('/'))
    };

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
        .post(&push_url)
        .header("Content-Type", "application/json")
        .body(body.to_string());

    // Authorization header value is used directly without logging.
    let effective_auth = if auth.is_empty() || auth == crate::config::LOKI_AUTH_MASK {
        // Empty or mask sentinel: try without auth (Loki may allow unauthenticated push)
        None
    } else {
        Some(auth)
    };
    if let Some(ref auth_val) = effective_auth {
        req_builder = req_builder.header("Authorization", auth_val.as_str());
    }
    if let Some(ref t) = tenant {
        if !t.is_empty() {
            req_builder = req_builder.header("X-Scope-OrgID", t.as_str());
        }
    }

    let response = req_builder
        .send()
        .await
        .map_err(|e| format!("Failed to reach Loki at {}: {}", push_url, e))?;

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
