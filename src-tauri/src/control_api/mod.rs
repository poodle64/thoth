//! Local Control API — loopback HTTP server for external integrations
//!
//! Binds ONLY to 127.0.0.1 (never 0.0.0.0). Protected by static bearer-token auth.
//! Off by default; opt-in via `integrations.apiEnabled` config.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::validate_request::ValidateRequestHeaderLayer;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Token generation
// ---------------------------------------------------------------------------

/// Token prefix — `sk-thoth-` marks it as a Thoth secret key (mirrors the
/// recognisable `sk-`/`sk-ant-` convention; greppable for secret scanners).
const TOKEN_PREFIX: &str = "sk-thoth-";

/// base62 alphabet (URL-safe, no ambiguous separators).
const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Generate a new bearer token of the canonical secret-key shape:
/// `sk-thoth-<40 base62 chars>` from the OS CSPRNG.
pub fn generate_token() -> String {
    // 40 chars of base62 ≈ 238 bits of entropy. Reject-sample to avoid modulo bias.
    let mut secret = String::with_capacity(40);
    let mut buf = [0u8; 64];
    while secret.len() < 40 {
        getrandom::fill(&mut buf).expect("OS CSPRNG unavailable");
        for &b in buf.iter() {
            if (b as usize) < 248 {
                // 248 = 4 * 62; keeps the distribution uniform across the 62 symbols
                secret.push(BASE62[(b % 62) as usize] as char);
                if secret.len() == 40 {
                    break;
                }
            }
        }
    }
    format!("{}{}", TOKEN_PREFIX, secret)
}

// ---------------------------------------------------------------------------
// Server state / handle
// ---------------------------------------------------------------------------

/// Handle that can signal the running server to stop.
struct ServerHandle {
    abort: tokio::task::AbortHandle,
}

/// Global server handle — Some while the server is running.
static SERVER: Mutex<Option<ServerHandle>> = Mutex::const_new(None);

// ---------------------------------------------------------------------------
// Shared state passed into axum handlers
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ApiState {
    app_version: String,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum AppError {
    Internal(String),
    NotFound(String),
    BadRequest(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::Internal(msg) => {
                let body = serde_json::json!({ "error": msg });
                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
            AppError::NotFound(msg) => {
                let body = serde_json::json!({ "error": msg });
                (StatusCode::NOT_FOUND, Json(body)).into_response()
            }
            AppError::BadRequest(msg) => {
                let body = serde_json::json!({ "error": msg });
                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
        }
    }
}

// AppError::Internal is the catch-all for `?` on Result<_, String> and similar.
// NotFound / BadRequest must be constructed explicitly.
impl From<String> for AppError {
    fn from(e: String) -> Self {
        AppError::Internal(e)
    }
}

impl From<&str> for AppError {
    fn from(e: &str) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Async transcription job registry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeJob {
    status: String,
    transcript: Option<String>,
    error: Option<String>,
}

/// In-memory job store for async file transcription.
static JOBS: Mutex<Option<HashMap<String, TranscribeJob>>> = Mutex::const_new(None);

async fn get_jobs() -> tokio::sync::MutexGuard<'static, Option<HashMap<String, TranscribeJob>>> {
    let mut guard = JOBS.lock().await;
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_health(State(state): State<Arc<ApiState>>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: state.app_version.clone(),
    })
}

async fn handle_get_state() -> Result<impl IntoResponse, AppError> {
    let state = crate::pipeline::get_pipeline_state();
    Ok(Json(state))
}

async fn handle_get_system() -> Result<impl IntoResponse, AppError> {
    let info = crate::platform::get_gpu_info()?;
    Ok(Json(info))
}

async fn handle_get_stats() -> Result<impl IntoResponse, AppError> {
    let stats = crate::database::transcription::get_transcription_stats_cmd()?;
    Ok(Json(stats))
}

async fn handle_get_prompts() -> impl IntoResponse {
    let prompts = crate::enhancement::prompts::get_all_prompts();
    Json(prompts)
}

async fn handle_get_settings() -> Result<impl IntoResponse, AppError> {
    let cfg = crate::config::get_config()?;
    Ok(Json(cfg))
}

async fn handle_patch_settings(
    Json(cfg): Json<crate::config::Config>,
) -> Result<impl IntoResponse, AppError> {
    crate::config::set_config(cfg)?;
    Ok(StatusCode::OK)
}

async fn handle_get_dictionary() -> Result<impl IntoResponse, AppError> {
    let entries = crate::dictionary::get_dictionary_entries()?;
    Ok(Json(entries))
}

/// Payload for POST /dictionary and PUT /dictionary/{index}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddEntryPayload {
    from: String,
    to: String,
    case_sensitive: bool,
}

async fn handle_add_dictionary(
    Json(payload): Json<AddEntryPayload>,
) -> Result<impl IntoResponse, AppError> {
    let entry = crate::dictionary::DictionaryEntry {
        from: payload.from,
        to: payload.to,
        case_sensitive: payload.case_sensitive,
    };
    crate::dictionary::add_dictionary_entry(entry)?;
    Ok(StatusCode::CREATED)
}

/// Reject an out-of-range dictionary index with 404 rather than letting the
/// underlying error map to a 500.
fn check_dictionary_index(index: usize) -> Result<(), AppError> {
    let count = crate::dictionary::get_dictionary_entries()?.len();
    if index >= count {
        return Err(AppError::NotFound(format!(
            "dictionary index {} out of range (have {} entries)",
            index, count
        )));
    }
    Ok(())
}

async fn handle_update_dictionary(
    Path(index): Path<usize>,
    Json(payload): Json<AddEntryPayload>,
) -> Result<impl IntoResponse, AppError> {
    check_dictionary_index(index)?;
    let entry = crate::dictionary::DictionaryEntry {
        from: payload.from,
        to: payload.to,
        case_sensitive: payload.case_sensitive,
    };
    crate::dictionary::update_dictionary_entry(index, entry)?;
    Ok(StatusCode::OK)
}

async fn handle_delete_dictionary(Path(index): Path<usize>) -> Result<impl IntoResponse, AppError> {
    check_dictionary_index(index)?;
    crate::dictionary::remove_dictionary_entry(index)?;
    Ok(StatusCode::OK)
}

/// Payload for POST /dictionary/import
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportPayload {
    json: String,
    merge: bool,
}

async fn handle_import_dictionary(
    Json(payload): Json<ImportPayload>,
) -> Result<impl IntoResponse, AppError> {
    let count = crate::dictionary::import_dictionary(payload.json, payload.merge)?;
    Ok(Json(serde_json::json!({ "imported": count })))
}

async fn handle_export_dictionary() -> Result<impl IntoResponse, AppError> {
    let body = crate::dictionary::export_dictionary()?;
    let value: serde_json::Value = serde_json::from_str(&body)?;
    Ok(Json(value))
}

async fn handle_get_transcription(Path(id): Path<String>) -> Result<impl IntoResponse, AppError> {
    match crate::database::transcription::get_transcription_by_id(id)? {
        Some(t) => Ok(Json(t).into_response()),
        None => Err(AppError::NotFound("transcription not found".to_string())),
    }
}

/// Payload for POST /transcribe
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscribePayload {
    path: String,
}

/// Submit a file for async transcription. Shared by the HTTP API and the MCP server.
///
/// Validates the path, registers a job, spawns the transcription off the executor
/// (and off the live recording pipeline), and returns the job id immediately.
pub(crate) async fn submit_transcribe_job(path: String) -> Result<String, String> {
    if !std::path::Path::new(&path).is_file() {
        return Err(format!("file not found or not readable: {}", path));
    }

    let job_id = Uuid::new_v4().to_string();

    {
        let mut guard = get_jobs().await;
        let jobs = guard.as_mut().unwrap();
        jobs.insert(
            job_id.clone(),
            TranscribeJob {
                status: "queued".to_string(),
                transcript: None,
                error: None,
            },
        );
    }

    let job_id_clone = job_id.clone();
    tokio::spawn(async move {
        // Mark as processing.
        {
            let mut guard = get_jobs().await;
            if let Some(job) = guard.as_mut().unwrap().get_mut(&job_id_clone) {
                job.status = "processing".to_string();
            }
        }

        // transcribe_file is synchronous and CPU-bound.
        let result =
            tokio::task::spawn_blocking(move || crate::transcription::transcribe_file(path)).await;

        let mut guard = get_jobs().await;
        if let Some(job) = guard.as_mut().unwrap().get_mut(&job_id_clone) {
            match result {
                Ok(Ok(transcript)) => {
                    job.status = "completed".to_string();
                    job.transcript = Some(transcript);
                }
                Ok(Err(e)) => {
                    job.status = "failed".to_string();
                    job.error = Some(e);
                }
                Err(e) => {
                    job.status = "failed".to_string();
                    job.error = Some(format!("task panicked: {}", e));
                }
            }
        }
    });

    Ok(job_id)
}

/// Look up a transcription job by id, returning its JSON representation. Shared with MCP.
pub(crate) async fn lookup_transcribe_job(id: &str) -> Option<serde_json::Value> {
    let guard = get_jobs().await;
    guard
        .as_ref()
        .unwrap()
        .get(id)
        .and_then(|job| serde_json::to_value(job).ok())
}

async fn handle_post_transcribe(
    Json(payload): Json<TranscribePayload>,
) -> Result<impl IntoResponse, AppError> {
    let job_id = submit_transcribe_job(payload.path)
        .await
        .map_err(AppError::BadRequest)?;
    Ok(Json(
        serde_json::json!({ "jobId": job_id, "status": "queued" }),
    ))
}

async fn handle_get_transcribe_job(Path(id): Path<String>) -> Result<impl IntoResponse, AppError> {
    match lookup_transcribe_job(&id).await {
        Some(job) => Ok(Json(job).into_response()),
        None => Err(AppError::NotFound(format!("job {} not found", id))),
    }
}

// ---------------------------------------------------------------------------
// Bearer-token auth layer
// ---------------------------------------------------------------------------

/// Build a [`ValidateRequestHeaderLayer`] that enforces bearer-token auth,
/// returning `{"error":"Unauthorized"}` with a 401 status on failure.
///
/// Uses `ValidateRequestHeaderLayer::custom` with a closure so the JSON error
/// body is preserved (the `accept` variant returns a bare status only).
// The Err variant is an axum::http::Response<Body> whose size is dictated by
// the tower ValidateRequestHeaderLayer API; boxing it would change the trait
// bound and break the layer type.
#[allow(clippy::result_large_err)]
fn bearer_auth_layer(
    token: String,
) -> ValidateRequestHeaderLayer<
    impl tower_http::validate_request::ValidateRequest<
        axum::body::Body,
        ResponseBody = axum::body::Body,
    > + Clone,
> {
    ValidateRequestHeaderLayer::custom(move |req: &mut axum::http::Request<axum::body::Body>| {
        let ok = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .is_some_and(|t| t == token);

        if ok {
            Ok(())
        } else {
            let body_bytes = serde_json::to_vec(&serde_json::json!({ "error": "Unauthorized" }))
                .unwrap_or_default();
            let body = axum::body::Body::from(body_bytes);
            let mut res = axum::http::Response::new(body);
            *res.status_mut() = StatusCode::UNAUTHORIZED;
            res.headers_mut().insert(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("application/json"),
            );
            Err(res)
        }
    })
}

// ---------------------------------------------------------------------------
// Router builder
// ---------------------------------------------------------------------------

fn build_router(token: String, app_version: String, mcp_enabled: bool) -> Router {
    let state = Arc::new(ApiState { app_version });

    let auth = bearer_auth_layer(token.clone());

    let mut router = Router::new()
        .route("/health", get(handle_health))
        .route("/state", get(handle_get_state))
        .route("/system", get(handle_get_system))
        .route("/stats", get(handle_get_stats))
        .route("/prompts", get(handle_get_prompts))
        .route("/settings", get(handle_get_settings))
        .route("/settings", patch(handle_patch_settings))
        .route("/dictionary", get(handle_get_dictionary))
        .route("/dictionary", post(handle_add_dictionary))
        .route("/dictionary/import", post(handle_import_dictionary))
        .route("/dictionary/export", get(handle_export_dictionary))
        .route("/dictionary/{index}", put(handle_update_dictionary))
        .route("/dictionary/{index}", delete(handle_delete_dictionary))
        .route("/transcriptions/{id}", get(handle_get_transcription))
        .route("/transcribe", post(handle_post_transcribe))
        .route("/transcribe/{id}", get(handle_get_transcribe_job))
        .layer(auth)
        .with_state(state);

    // Mount the bundled MCP server at /mcp when enabled, behind the same bearer auth.
    if mcp_enabled {
        let mcp_service = crate::mcp_server::build_service();
        let mcp_router = Router::new()
            .nest_service("/mcp", mcp_service)
            .layer(bearer_auth_layer(token));
        router = router.merge(mcp_router);
        tracing::info!("MCP server mounted at /mcp");
    }

    router
}

// ---------------------------------------------------------------------------
// Start / stop
// ---------------------------------------------------------------------------

/// Start the control API server.
///
/// If a server is already running it is stopped first.
/// Binds 127.0.0.1:{port} only.
pub async fn start(port: u16, token: String, mcp_enabled: bool) -> Result<(), String> {
    // Stop any existing server before starting a new one.
    stop().await;

    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let router = build_router(token, app_version, mcp_enabled);

    let addr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), port);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Control API: failed to bind {}:{}: {}", addr.ip(), port, e);
            return Err(format!(
                "Could not start the server on port {port}: {e}. \
                 The port may already be in use — try a different port."
            ));
        }
    };

    tracing::info!("Control API listening on {}", addr);

    let server_fut = axum::serve(listener, router);
    let join_handle = tokio::spawn(async move {
        if let Err(e) = server_fut.await {
            tracing::error!("Control API server error: {}", e);
        }
    });

    let mut guard = SERVER.lock().await;
    *guard = Some(ServerHandle {
        abort: join_handle.abort_handle(),
    });
    Ok(())
}

/// Stop the running control API server, if any.
pub async fn stop() {
    let mut guard = SERVER.lock().await;
    if let Some(handle) = guard.take() {
        handle.abort.abort();
        tracing::info!("Control API server stopped");
    }
}

/// Returns true if the server task is currently running (not aborted).
pub async fn is_running() -> bool {
    let guard = SERVER.lock().await;
    guard.is_some()
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Status of the integrations (reported to the frontend).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationsStatus {
    pub api_enabled: bool,
    pub api_running: bool,
    pub api_port: u16,
    pub mcp_enabled: bool,
    pub has_token: bool,
}

/// Return the current integrations status for the frontend settings panel.
#[tauri::command]
pub async fn get_integrations_status() -> Result<IntegrationsStatus, String> {
    let cfg = crate::config::get_config()?;
    let running = is_running().await;
    Ok(IntegrationsStatus {
        api_enabled: cfg.integrations.api_enabled,
        api_running: running,
        api_port: cfg.integrations.api_port,
        mcp_enabled: cfg.integrations.mcp_enabled,
        has_token: cfg.integrations.api_token.is_some(),
    })
}

/// Enable or disable the Local Control API.
///
/// When enabling: generates a token if none exists, then starts the server.
/// When disabling: stops the server and persists the updated flag.
#[tauri::command]
pub async fn set_api_enabled(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let _ = app; // AppHandle reserved for future event emission
    let mut cfg = crate::config::get_config()?;
    cfg.integrations.api_enabled = enabled;

    if enabled {
        // Generate a token on first enable.
        if cfg.integrations.api_token.is_none() {
            cfg.integrations.api_token = Some(generate_token());
        }
        let token = cfg.integrations.api_token.clone().unwrap();
        let port = cfg.integrations.api_port;
        let mcp = cfg.integrations.mcp_enabled;

        crate::config::set_config(cfg)?;
        start(port, token, mcp).await?;
    } else {
        crate::config::set_config(cfg)?;
        stop().await;
    }
    Ok(())
}

/// Enable or disable the bundled MCP server.
///
/// The MCP server mounts at `/mcp` on the same loopback HTTP server as the
/// Control API. Enabling MCP also enables and starts the Control API if it
/// isn't already running (the MCP route can't exist without the host server).
/// The route change takes effect immediately; no app restart is required.
#[tauri::command]
pub async fn set_mcp_enabled(enabled: bool) -> Result<(), String> {
    let mut cfg = crate::config::get_config()?;
    cfg.integrations.mcp_enabled = enabled;

    // Enabling MCP implies the Control API must be on to host the /mcp route.
    if enabled {
        cfg.integrations.api_enabled = true;
        if cfg.integrations.api_token.is_none() {
            cfg.integrations.api_token = Some(generate_token());
        }
    }

    let port = cfg.integrations.api_port;
    let token = cfg.integrations.api_token.clone();
    let api_enabled = cfg.integrations.api_enabled;
    crate::config::set_config(cfg)?;

    // Restart (or start) the server so the /mcp route appears/disappears live.
    // Awaiting start() means the bind has completed before we return, so the
    // status the UI reads immediately afterwards is accurate.
    if api_enabled {
        if let Some(t) = token {
            start(port, t, enabled).await?;
        }
    }
    Ok(())
}

/// Return the current API token for display/copy in the settings panel.
#[tauri::command]
pub async fn get_api_token() -> Result<Option<String>, String> {
    let cfg = crate::config::get_config()?;
    Ok(cfg.integrations.api_token)
}

/// Generate a new token, persist it, and restart the server if running.
///
/// Returns the new token so the frontend can display it immediately.
#[tauri::command]
pub async fn rotate_api_token(app: tauri::AppHandle) -> Result<String, String> {
    let _ = app;
    let new_token = generate_token();
    let mut cfg = crate::config::get_config()?;
    cfg.integrations.api_token = Some(new_token.clone());
    let was_running = is_running().await;
    let port = cfg.integrations.api_port;
    let mcp = cfg.integrations.mcp_enabled;
    crate::config::set_config(cfg)?;

    if was_running {
        start(port, new_token.clone(), mcp).await?;
    }
    Ok(new_token)
}

/// Change the API port. Restarts the server on the new port if it was running.
#[tauri::command]
pub async fn set_api_port(app: tauri::AppHandle, port: u16) -> Result<(), String> {
    let _ = app;
    let mut cfg = crate::config::get_config()?;
    cfg.integrations.api_port = port;
    let was_running = is_running().await;
    let token = cfg.integrations.api_token.clone();
    let mcp = cfg.integrations.mcp_enabled;
    crate::config::set_config(cfg)?;

    if was_running {
        if let Some(t) = token {
            start(port, t, mcp).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_has_canonical_shape() {
        let t = generate_token();
        assert!(t.starts_with("sk-thoth-"), "prefix: {}", t);
        assert_eq!(t.len(), TOKEN_PREFIX.len() + 40, "length: {}", t);
        let secret = &t[TOKEN_PREFIX.len()..];
        assert!(
            secret.bytes().all(|b| BASE62.contains(&b)),
            "secret must be base62: {}",
            secret
        );
    }

    #[test]
    fn tokens_are_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
    }
}
