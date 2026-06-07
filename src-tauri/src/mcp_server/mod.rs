//! Bundled MCP server — exposes Thoth's control surface to MCP clients.
//!
//! Implemented with rmcp (the official Rust MCP SDK). Mounts as a tower service
//! on the same loopback axum router as the Control API, behind the same bearer-token
//! auth. Tools call the same shared core functions the GUI and HTTP API use; no
//! business logic lives here (transport only). Opt-in via `integrations.mcpEnabled`.
//!
//! Tool surface (task-centric, per the house MCP design principles):
//! - `dictionary` (dispatcher: list/add/update/delete/import/export)
//! - `setting`    (dispatcher: get/update)
//! - `transcription` (dispatcher: list/get/stats)
//! - `transcribe_file` / `transcribe_status` (async file transcription)
//! - `get_state`, `get_system`, `list_prompts`

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
};

/// The MCP server state. Cloned per session by the transport's service factory.
#[derive(Clone)]
pub struct ThothMcp {
    // Read by the `#[tool_handler]` macro-generated `ServerHandler` impl.
    #[allow(dead_code)]
    tool_router: ToolRouter<ThothMcp>,
}

impl Default for ThothMcp {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DictionaryParams {
    /// The operation: `list`, `add`, `update`, `delete`, `import`, or `export`.
    pub action: String,
    /// For `add`/`update`: the text to find. For `update`/`delete`: identify by index.
    #[serde(default)]
    pub from: Option<String>,
    /// For `add`/`update`: the replacement text.
    #[serde(default)]
    pub to: Option<String>,
    /// For `add`/`update`: whether the match is case-sensitive (default false).
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    /// For `update`/`delete`: the zero-based index of the entry (from `list`).
    #[serde(default)]
    pub index: Option<usize>,
    /// For `import`: a JSON string of dictionary entries.
    #[serde(default)]
    pub json: Option<String>,
    /// For `import`: merge with existing entries (true) or replace (false). Default true.
    #[serde(default)]
    pub merge: Option<bool>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanonicalParams {
    /// The operation: `list`, `add`, `update`, or `remove`.
    pub action: String,
    /// For `add`/`update`: the canonical term string (e.g. "portcullis", "LiteLLM").
    #[serde(default)]
    pub term: Option<String>,
    /// For `add`/`update`: explicit spelling aliases (case-insensitive exact matches).
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    /// For `add`/`update`: matching policy — `aliasOnly` (default), `phonetic`, or `conservative`.
    #[serde(default)]
    pub policy: Option<String>,
    /// For `update`/`remove`: the zero-based index of the term (from `list`).
    #[serde(default)]
    pub index: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SettingParams {
    /// The operation: `get` (read all settings) or `update`.
    pub action: String,
    /// For `update`: a JSON object of settings to change, merged onto the current
    /// config (e.g. `{"enhancement":{"enabled":true}}`). Missing fields keep their values.
    #[serde(default)]
    pub patch: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TranscriptionParams {
    /// The operation: `list` (recent history), `get` (one by id), or `stats` (quality).
    pub action: String,
    /// For `get`: the transcription id.
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TranscribeFileParams {
    /// Absolute or `~`-relative path to a local audio file (WAV, MP3, M4A, OGG, FLAC).
    pub path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TranscribeStatusParams {
    /// The job id returned by `transcribe_file`.
    pub job_id: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a successful text result from a serialisable value (pretty JSON).
fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

/// Map a core-call error message into an MCP tool error on failure.
fn core_err(msg: String) -> McpError {
    McpError::internal_error(msg, None)
}

/// Parse an optional policy string into a `SnapPolicy`.  `None` defaults to `AliasOnly`.
fn parse_snap_policy(policy: Option<&str>) -> Result<crate::canonical::SnapPolicy, McpError> {
    match policy {
        None | Some("aliasOnly") => Ok(crate::canonical::SnapPolicy::AliasOnly),
        Some("phonetic") => Ok(crate::canonical::SnapPolicy::Phonetic),
        Some("conservative") => Ok(crate::canonical::SnapPolicy::Conservative),
        Some(other) => Err(core_err(format!(
            "unknown policy '{}'; must be aliasOnly | phonetic | conservative",
            other
        ))),
    }
}

#[tool_router]
impl ThothMcp {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Manage Thoth's personal dictionary (find/replace entries applied to transcriptions). Use this to view or change spelling corrections and word replacements. Action: list | add | update | delete | import | export. add/update require from + to (+ optional caseSensitive); update/delete require index (from list); import requires json (+ optional merge). Returns: the entry list (list/add/update), a count (import), or a JSON string (export)."
    )]
    async fn dictionary(
        &self,
        Parameters(p): Parameters<DictionaryParams>,
    ) -> Result<CallToolResult, McpError> {
        match p.action.as_str() {
            "list" => {
                let entries = crate::dictionary::get_dictionary_entries()
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&entries)
            }
            "add" => {
                let entry = crate::dictionary::DictionaryEntry {
                    from: p
                        .from
                        .ok_or_else(|| core_err("`from` required for add".into()))?,
                    to: p
                        .to
                        .ok_or_else(|| core_err("`to` required for add".into()))?,
                    case_sensitive: p.case_sensitive.unwrap_or(false),
                };
                crate::dictionary::add_dictionary_entry(entry)
                    .map_err(|e| core_err(e.to_string()))?;
                let entries = crate::dictionary::get_dictionary_entries()
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&entries)
            }
            "update" => {
                let index = p
                    .index
                    .ok_or_else(|| core_err("`index` required for update".into()))?;
                let entry = crate::dictionary::DictionaryEntry {
                    from: p
                        .from
                        .ok_or_else(|| core_err("`from` required for update".into()))?,
                    to: p
                        .to
                        .ok_or_else(|| core_err("`to` required for update".into()))?,
                    case_sensitive: p.case_sensitive.unwrap_or(false),
                };
                crate::dictionary::update_dictionary_entry(index, entry)
                    .map_err(|e| core_err(e.to_string()))?;
                let entries = crate::dictionary::get_dictionary_entries()
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&entries)
            }
            "delete" => {
                let index = p
                    .index
                    .ok_or_else(|| core_err("`index` required for delete".into()))?;
                crate::dictionary::remove_dictionary_entry(index)
                    .map_err(|e| core_err(e.to_string()))?;
                let entries = crate::dictionary::get_dictionary_entries()
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&entries)
            }
            "import" => {
                let json = p
                    .json
                    .ok_or_else(|| core_err("`json` required for import".into()))?;
                let count = crate::dictionary::import_dictionary(json, p.merge.unwrap_or(true))
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&serde_json::json!({ "imported": count }))
            }
            "export" => {
                let json =
                    crate::dictionary::export_dictionary().map_err(|e| core_err(e.to_string()))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            other => Err(core_err(format!("unknown action: {}", other))),
        }
    }

    #[tool(
        description = "Manage Thoth's canonical-term registry (phonetic/fuzzy snapping of acoustic variants to a registered spelling). Register a term ONCE and all acoustic/spelling variants auto-snap to it. Action: list | add | update | remove. add/update require term (+ optional aliases, policy, index for update/remove). policy: aliasOnly (default, exact aliases only), phonetic (AND gate: Double-Metaphone key match AND edit-distance >= 0.55), conservative (same AND gate, higher 0.85 threshold for terms that collide with common words). Returns: the term list."
    )]
    async fn canonical(
        &self,
        Parameters(p): Parameters<CanonicalParams>,
    ) -> Result<CallToolResult, McpError> {
        match p.action.as_str() {
            "list" => {
                let terms =
                    crate::canonical::get_canonical_terms().map_err(|e| core_err(e.to_string()))?;
                json_result(&terms)
            }
            "add" => {
                let term_str = p
                    .term
                    .ok_or_else(|| core_err("`term` required for add".into()))?;
                let policy = parse_snap_policy(p.policy.as_deref())?;
                let ct = crate::canonical::CanonicalTerm {
                    term: term_str,
                    aliases: p.aliases.unwrap_or_default(),
                    policy,
                    max_words: 3,
                    threshold: None,
                };
                crate::canonical::add_canonical_term(ct).map_err(|e| core_err(e.to_string()))?;
                let terms =
                    crate::canonical::get_canonical_terms().map_err(|e| core_err(e.to_string()))?;
                json_result(&terms)
            }
            "update" => {
                let index = p
                    .index
                    .ok_or_else(|| core_err("`index` required for update".into()))?;
                let term_str = p
                    .term
                    .ok_or_else(|| core_err("`term` required for update".into()))?;
                let policy = parse_snap_policy(p.policy.as_deref())?;
                let ct = crate::canonical::CanonicalTerm {
                    term: term_str,
                    aliases: p.aliases.unwrap_or_default(),
                    policy,
                    max_words: 3,
                    threshold: None,
                };
                crate::canonical::update_canonical_term(index, ct)
                    .map_err(|e| core_err(e.to_string()))?;
                let terms =
                    crate::canonical::get_canonical_terms().map_err(|e| core_err(e.to_string()))?;
                json_result(&terms)
            }
            "remove" => {
                let index = p
                    .index
                    .ok_or_else(|| core_err("`index` required for remove".into()))?;
                crate::canonical::remove_canonical_term(index)
                    .map_err(|e| core_err(e.to_string()))?;
                let terms =
                    crate::canonical::get_canonical_terms().map_err(|e| core_err(e.to_string()))?;
                json_result(&terms)
            }
            other => Err(core_err(format!("unknown action: {}", other))),
        }
    }

    #[tool(
        description = "Read or change Thoth's settings (AI enhancement on/off, backend, prompt; output filters; Australian spelling; selected model; sounds). Action: get | update. update requires patch — a JSON object of fields to change, merged onto the current config. Returns: the full settings object."
    )]
    async fn setting(
        &self,
        Parameters(p): Parameters<SettingParams>,
    ) -> Result<CallToolResult, McpError> {
        match p.action.as_str() {
            "get" => {
                let cfg = crate::config::get_config().map_err(|e| core_err(e.to_string()))?;
                json_result(&cfg)
            }
            "update" => {
                let patch = p
                    .patch
                    .ok_or_else(|| core_err("`patch` required for update".into()))?;
                // Merge the patch onto the current config, then set.
                let mut current = serde_json::to_value(
                    crate::config::get_config().map_err(|e| core_err(e.to_string()))?,
                )
                .map_err(|e| core_err(e.to_string()))?;
                let patch_val: serde_json::Value = serde_json::from_str(&patch)
                    .map_err(|e| core_err(format!("invalid patch JSON: {}", e)))?;
                merge_json(&mut current, &patch_val);
                let new_cfg: crate::config::Config = serde_json::from_value(current)
                    .map_err(|e| core_err(format!("merged config invalid: {}", e)))?;
                crate::config::set_config(new_cfg).map_err(|e| core_err(e.to_string()))?;
                let cfg = crate::config::get_config().map_err(|e| core_err(e.to_string()))?;
                json_result(&cfg)
            }
            other => Err(core_err(format!("unknown action: {}", other))),
        }
    }

    #[tool(
        description = "Read transcription history and quality. Action: list (recent records), get (one by id), stats (counts, average duration, per-model throughput). get requires id. Returns: records (list), a record (get), or summary statistics (stats)."
    )]
    async fn transcription(
        &self,
        Parameters(p): Parameters<TranscriptionParams>,
    ) -> Result<CallToolResult, McpError> {
        match p.action.as_str() {
            "stats" => {
                let stats = crate::database::transcription::get_transcription_stats_cmd()
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&stats)
            }
            "get" => {
                let id =
                    p.id.ok_or_else(|| core_err("`id` required for get".into()))?;
                let record = crate::database::transcription::get_transcription_by_id(id)
                    .map_err(|e| core_err(e.to_string()))?;
                match record {
                    Some(r) => json_result(&r),
                    None => Err(core_err("transcription not found".into())),
                }
            }
            "list" => {
                // Recent records: reuse the stats-backed history listing if available;
                // fall back to an empty-ids fetch which the export module supports.
                let records = crate::export::get_transcriptions(Vec::new())
                    .map_err(|e| core_err(e.to_string()))?;
                json_result(&records)
            }
            other => Err(core_err(format!("unknown action: {}", other))),
        }
    }

    #[tool(
        description = "Transcribe a local audio file through Thoth (WAV, MP3, M4A, OGG, FLAC). Runs as a background job that does not disturb live recording. Returns a jobId; poll `transcribe_status` for the transcript."
    )]
    async fn transcribe_file(
        &self,
        Parameters(p): Parameters<TranscribeFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let job_id = crate::control_api::submit_transcribe_job(p.path)
            .await
            .map_err(|e| core_err(e.to_string()))?;
        json_result(&serde_json::json!({ "jobId": job_id, "status": "queued" }))
    }

    #[tool(
        description = "Check the status of a `transcribe_file` job. Returns status (queued/processing/completed/failed) and, when completed, the transcript."
    )]
    async fn transcribe_status(
        &self,
        Parameters(p): Parameters<TranscribeStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        match crate::control_api::lookup_transcribe_job(&p.job_id).await {
            Some(job) => json_result(&job),
            None => Err(core_err(format!("job {} not found", p.job_id))),
        }
    }

    #[tool(description = "Get Thoth's current pipeline state (idle, recording, or processing).")]
    async fn get_state(&self) -> Result<CallToolResult, McpError> {
        let state = crate::pipeline::get_pipeline_state();
        json_result(&state)
    }

    #[tool(description = "Get GPU/system info and transcription readiness.")]
    async fn get_system(&self) -> Result<CallToolResult, McpError> {
        let info = crate::platform::get_gpu_info().map_err(|e| core_err(e.to_string()))?;
        json_result(&info)
    }

    #[tool(description = "List the available AI-enhancement prompt templates.")]
    async fn list_prompts(&self) -> Result<CallToolResult, McpError> {
        let prompts = crate::enhancement::prompts::get_all_prompts();
        json_result(&prompts)
    }
}

#[tool_handler]
impl ServerHandler for ThothMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "Use this server to read and control the locally-running Thoth voice-transcription \
                 app on this machine. Dispatchers: `dictionary` (list/add/update/delete/import/export), \
                 `setting` (get/update), `transcription` (list/get/stats). Singletons: `transcribe_file` \
                 + `transcribe_status` (transcribe a local audio file as a background job), `get_state`, \
                 `get_system`, `list_prompts`. All operations mirror what the user can do in Thoth's GUI; \
                 destructive and system operations are intentionally not exposed. This controls only the \
                 local instance."
                    .to_string(),
            )
    }
}

// ---------------------------------------------------------------------------
// Transport service (mounted on the Control API's axum router at /mcp)
// ---------------------------------------------------------------------------

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

/// Build the streamable-HTTP MCP service for mounting on the loopback axum router.
///
/// `allowed_hosts` defaults to loopback only (anti DNS-rebinding); the Control API's
/// bearer-token auth layer is applied in front of the mount point.
pub fn build_service() -> StreamableHttpService<ThothMcp, LocalSessionManager> {
    StreamableHttpService::new(
        || Ok(ThothMcp::new()),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    )
}

/// Recursively merge `patch` into `target` (objects merged key-wise; other values replaced).
fn merge_json(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge_json(t.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (t, p) => *t = p.clone(),
    }
}
