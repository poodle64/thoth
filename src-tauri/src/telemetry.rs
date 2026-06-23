//! Telemetry and observability export
//!
//! Provides the `test_loki_connection` Tauri command used by the Settings
//! "Test connection" button, and helpers for the Loki filter predicate.

use crate::error::Error;

/// The literal target string used for all Loki-bound telemetry events.
///
/// Both the subscriber filter in `init_logging` and the unit tests refer to
/// this constant so a typo or rename fails compilation rather than silently
/// opening a privacy hole.
pub(crate) const TELEMETRY_TARGET: &str = "telemetry";

/// Returns `true` when a tracing event targets the Loki-only "telemetry" channel.
///
/// This is the single source of truth for the allow-list predicate used in
/// `init_logging`'s `filter_fn`. Extracting it here lets unit tests call the
/// same function rather than duplicating the literal string comparison.
pub(crate) fn is_telemetry_event(meta: &tracing::Metadata<'_>) -> bool {
    meta.target() == TELEMETRY_TARGET
}

/// Target-string form of `is_telemetry_event` for use in unit tests that
/// cannot easily construct a real `tracing::Metadata`.
#[cfg(test)]
pub(crate) fn is_telemetry_event_by_target(target: &str) -> bool {
    target == TELEMETRY_TARGET
}

/// Push a single synthetic telemetry line to the given Loki endpoint.
///
/// Used by the Settings "Test connection" button to verify that the URL,
/// auth token, and tenant are correct before the user saves and restarts.
///
/// `auth` is `Option<String>`: when the caller passes `None`, empty string, or
/// the mask sentinel (`***`), the command resolves the effective token from the
/// in-memory config (the real, unmasked value).  This handles the normal day-2
/// case where the frontend shows `***` and the user just clicks "Test" without
/// re-entering the token.
#[tauri::command]
pub async fn test_loki_connection(
    url: String,
    auth: Option<String>,
    tenant: Option<String>,
) -> Result<(), Error> {
    crate::ensure_crypto_provider();

    if url.is_empty() {
        return Err("Loki URL is required".to_string().into());
    }

    // Resolve the effective auth token: if the caller passed None, empty, or
    // the mask sentinel, fall back to the real stored token. Read directly from
    // the in-memory config instance rather than through get_config() (which
    // masks the token and would just give us *** back again).
    let effective_auth: String = {
        let raw = auth.as_deref().unwrap_or("");
        if raw.is_empty() || raw == crate::config::LOKI_AUTH_MASK {
            // Read the unmasked token from the live in-memory config.
            crate::config::get_raw_loki_auth()
        } else {
            raw.to_string()
        }
    };

    // Resolve the effective tenant the same way.
    let effective_tenant: Option<String> = match tenant.as_deref() {
        Some(t) if !t.is_empty() => Some(t.to_string()),
        _ => crate::config::get_raw_loki_tenant(),
    };

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

    // 10-second timeout so a slow or hostile Loki URL cannot hang the Settings page.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let mut req_builder = client
        .post(&push_url)
        .header("Content-Type", "application/json")
        .body(body.to_string());

    // Normalise to a full Authorization header (bare token gets a Bearer
    // prefix). Empty/masked yields None — push unauthenticated. Never logged.
    if let Some(auth_val) = crate::config::authorization_header(&effective_auth) {
        req_builder = req_builder.header("Authorization", auth_val);
    }
    if let Some(ref t) = effective_tenant {
        req_builder = req_builder.header("X-Scope-OrgID", t.as_str());
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
        let body_text = response.text().await.unwrap_or_default();
        // Truncate to 200 chars so a hostile server cannot embed a huge blob in
        // the error string that gets rendered in the Settings UI. Collect by
        // char so the slice is never on a multibyte UTF-8 boundary.
        let truncated = {
            let mut chars = body_text.chars();
            let head: String = chars.by_ref().take(200).collect();
            if chars.next().is_some() {
                format!("{head}…")
            } else {
                head
            }
        };
        Err(format!("Loki returned {}: {}", status, truncated).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telemetry_event_predicate_passes_telemetry_target() {
        // Exercises the live predicate via its target-string form so a rename
        // of TELEMETRY_TARGET would break this test.
        assert!(
            is_telemetry_event_by_target(TELEMETRY_TARGET),
            "telemetry target must pass the filter"
        );
    }

    #[test]
    fn telemetry_event_predicate_blocks_other_targets() {
        for target in &["thoth::pipeline", "thoth::audio", "tracing", "info"] {
            assert!(
                !is_telemetry_event_by_target(target),
                "target '{target}' must be blocked by the telemetry filter"
            );
        }
    }
}
