//! Thoth - Privacy-first voice transcription
//!
//! Desktop application for macOS and Linux.

use tauri::Manager;

use crate::error::Error;

pub mod app_handle;
pub mod audio;
pub mod canonical;
pub mod clipboard;
pub mod commands;
pub mod config;
pub mod control_api;
pub mod database;
pub mod dictionary;
pub mod enhancement;
pub mod error;
pub mod export;
pub mod keyboard_service;
pub mod mcp_server;
pub mod mouse_tracker;
pub mod pipeline;
pub mod platform;
pub mod recording_indicator;
pub mod shortcuts;
pub mod sound;
pub mod storage;
pub mod telemetry;
pub mod text_insert;
#[cfg(target_os = "macos")]
mod traffic_lights;
pub mod transcription;
pub mod tray;

/// Header height in pixels (must match CSS --header-height)
#[cfg(target_os = "macos")]
const HEADER_HEIGHT: f64 = 52.0;

/// Traffic light X position (left margin)
#[cfg(target_os = "macos")]
const TRAFFIC_LIGHT_X: f64 = 13.0;

/// Register a single shortcut, using keyboard_service for standalone modifiers
fn register_single_shortcut(
    app: &tauri::AppHandle,
    id: &str,
    accelerator: &str,
    description: &str,
) -> Result<(), Error> {
    // Check if this is a standalone modifier shortcut (e.g., ShiftRight)
    if keyboard_service::is_modifier_shortcut(accelerator) {
        // Register with keyboard service
        if keyboard_service::register_modifier_shortcut(
            id.to_string(),
            accelerator.to_string(),
            description.to_string(),
        ) {
            Ok(())
        } else {
            Err(format!("Failed to register modifier shortcut: {}", accelerator).into())
        }
    } else {
        // Use regular global shortcut system
        shortcuts::register_shortcut(
            app.clone(),
            id.to_string(),
            accelerator.to_string(),
            description.to_string(),
        )
    }
}

/// Re-register all shortcuts from saved config.
///
/// Unregisters everything first, then registers from the current config.
/// Called by the frontend after clearing or resetting a shortcut.
#[tauri::command]
fn reregister_shortcuts(app: tauri::AppHandle) -> Result<(), Error> {
    // Unregister everything
    shortcuts::unregister_all_shortcuts(app.clone())?;

    // Re-register from config
    let cfg = config::get_config().map_err(|e| format!("Failed to load config: {}", e))?;
    register_shortcuts_from_config(&app, &cfg);

    tracing::info!("Re-registered all shortcuts from config");
    Ok(())
}

/// Register shortcuts from saved configuration
fn register_shortcuts_from_config(app: &tauri::AppHandle, cfg: &config::Config) {
    use shortcuts::manager::shortcut_ids;

    // On Wayland, the actual global-shortcut binding is owned by the XDG portal,
    // set up once here. On X11 this is a no-op and the per-shortcut registration
    // below binds via the Tauri plugin as on macOS.
    #[cfg(target_os = "linux")]
    shortcuts::linux::init_global_shortcuts(app);

    // Collect (id, accelerator, description) tuples for all configured shortcuts
    let shortcuts: Vec<(&str, &str, &str)> = [
        Some((
            shortcut_ids::TOGGLE_RECORDING,
            cfg.shortcuts.toggle_recording.as_str(),
            "Toggle recording",
        )),
        cfg.shortcuts.toggle_recording_alt.as_deref().map(|accel| {
            (
                shortcut_ids::TOGGLE_RECORDING_ALT,
                accel,
                "Toggle recording (alternative)",
            )
        }),
        cfg.shortcuts.copy_last.as_deref().map(|accel| {
            (
                shortcut_ids::COPY_LAST_TRANSCRIPTION,
                accel,
                "Copy last transcription",
            )
        }),
        cfg.shortcuts.toggle_enhancement.as_deref().map(|accel| {
            (
                shortcut_ids::TOGGLE_ENHANCEMENT,
                accel,
                "Toggle AI enhancement",
            )
        }),
    ]
    .into_iter()
    .flatten()
    .filter(|(_, accel, _)| !accel.is_empty())
    .collect();

    for (id, accelerator, description) in shortcuts {
        match register_single_shortcut(app, id, accelerator, description) {
            Ok(()) => tracing::info!("Registered {} shortcut: {}", id, accelerator),
            Err(e) => tracing::warn!("Failed to register {} shortcut: {}", id, e),
        }
    }

    // Start the keyboard service if any modifier shortcuts were registered
    keyboard_service::start_monitoring(app.clone());
}

/// Installs the `ring` crypto provider as the process-wide rustls default,
/// exactly once. reqwest is configured with `rustls-no-provider` (to avoid
/// aws-lc-sys, whose C build fails under the -march=armv8-a baseline the macOS
/// build sets for whisper.cpp), so a provider must be installed before any
/// reqwest `Client` is built or reqwest panics. Idempotent and safe to call
/// from every client-construction site, including unit tests that never run
/// `run()`.
pub(crate) fn ensure_crypto_provider() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Bridge between `init_logging` and the Tauri `setup` hook.
///
/// `init_logging` registers the Loki layer with the subscriber and stores the
/// `BackgroundTask` here. The Tauri `setup` hook takes it out and spawns it on
/// the async runtime once that runtime is available.
static LOKI_TASK_STORE: std::sync::Mutex<Option<tracing_loki::BackgroundTask>> =
    std::sync::Mutex::new(None);

/// Build a `tracing-loki` `(Layer, BackgroundTask)` pair from `LoggingConfig`.
///
/// Returns `None` on any error (bad URL, bad label name, etc.) so the caller
/// gracefully falls back to local-only logging.
fn build_loki_components(
    cfg: &config::LoggingConfig,
) -> Option<(tracing_loki::Layer, tracing_loki::BackgroundTask)> {
    let url = cfg.loki_url.parse::<url::Url>().ok()?;

    // hostname crate gives us the machine name without pulling in system deps beyond
    // what is already in the tree.
    let host = hostname::get()
        .ok()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());

    let mut builder = tracing_loki::builder()
        .label("app", "thoth")
        .ok()?
        .label("host", host)
        .ok()?
        .label("version", env!("CARGO_PKG_VERSION"))
        .ok()?;

    for pair in &cfg.loki_labels {
        builder = builder.label(pair[0].as_str(), pair[1].as_str()).ok()?;
    }

    // Authorization header — value is intentionally not logged anywhere.
    if !cfg.loki_auth.0.is_empty() {
        builder = builder
            .http_header("Authorization", cfg.loki_auth.0.as_str())
            .ok()?;
    }

    if let Some(tenant) = &cfg.loki_tenant {
        builder = builder.http_header("X-Scope-OrgID", tenant.as_str()).ok()?;
    }

    builder.build_url(url).ok()
}

/// Initialise the layered tracing subscriber.
///
/// Layers:
/// - **File**: daily rolling appender (`~/.thoth/logs/thoth-YYYY-MM-DD.log`), retaining
///   `local_retention_days` files before pruning the oldest.
/// - **Stdout**: unchanged dev convenience.
/// - **Loki (optional)**: built only when `logging.remote_enabled` is true and a URL is set.
///   Filtered to `target == "telemetry"` events only — the structural privacy boundary that
///   prevents transcript content from ever reaching the remote endpoint.
///   The Loki `BackgroundTask` is stored in `LOKI_COMPONENTS` and spawned in the Tauri `setup`
///   hook once the async runtime is available. The `Layer` itself is registered here so events
///   queue into its channel from first use; the task just needs to start before the first flush.
///
/// Configuration is read synchronously from disk so the subscriber is ready before any event
/// fires. Changes require a restart (documented in the UI).
fn init_logging() {
    use tracing_subscriber::Layer;
    use tracing_subscriber::prelude::*;

    /// Local-time timestamp formatter using chrono
    struct LocalTimer;
    impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
        fn format_time(
            &self,
            w: &mut tracing_subscriber::fmt::format::Writer<'_>,
        ) -> std::fmt::Result {
            write!(
                w,
                "{}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
            )
        }
    }

    let logging_cfg = config::read_logging_config_early();

    let log_dir = dirs::home_dir()
        .map(|h| h.join(".thoth").join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let _ = std::fs::create_dir_all(&log_dir);

    // Daily rolling appender with bounded retention. Falls back to a no-op writer on
    // error (e.g. permission denied) so the app still starts without logging to disk.
    let (appender, guard) = match tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("thoth")
        .filename_suffix("log")
        .max_log_files(logging_cfg.local_retention_days as usize)
        .build(&log_dir)
    {
        Ok(a) => tracing_appender::non_blocking(a),
        Err(_) => tracing_appender::non_blocking(std::io::sink()),
    };
    // Leak the WorkerGuard so the background writer lives for the process lifetime
    // and flushes on shutdown.
    std::mem::forget(guard);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(appender)
        .with_timer(LocalTimer)
        .with_ansi(false);

    let stdout_layer = tracing_subscriber::fmt::layer().with_timer(LocalTimer);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer);

    if logging_cfg.remote_enabled && !logging_cfg.loki_url.is_empty() {
        if let Some((loki_layer, loki_task)) = build_loki_components(&logging_cfg) {
            // The Loki layer is filtered to `target == "telemetry"` only — the
            // allow-list boundary that makes content leakage structurally impossible.
            let telemetry_filter =
                tracing_subscriber::filter::filter_fn(|meta| meta.target() == "telemetry");

            // Store only the background task; the layer is consumed by registry.with().
            // The Tauri setup hook takes the task out of LOKI_TASK_STORE and spawns it.
            {
                let mut stored = LOKI_TASK_STORE.lock().unwrap_or_else(|e| e.into_inner());
                *stored = Some(loki_task);
            }

            registry
                .with(loki_layer.with_filter(telemetry_filter))
                .init();
            return;
        }
        // Log after init so the warning reaches the file layer — call init first.
        registry.init();
        tracing::warn!(
            "Loki remote logging enabled but layer could not be built (bad URL or label?); \
             falling back to local-only logging"
        );
        return;
    }

    registry.init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    ensure_crypto_provider();

    init_logging();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Another instance tried to launch — focus the existing main window
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostarted"]),
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init());

    // Updater plugin only on desktop platforms (not mobile)
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }

    builder
        .setup(|app| {
            tracing::info!("Thoth starting");

            // Spawn the Loki background task if the remote logging layer was built.
            // init_logging() registered the layer and stored the task here; the async
            // runtime is now live so we can spawn it. Events queued before this point
            // are buffered in the layer's channel and drained once the task starts.
            if let Some(task) = LOKI_TASK_STORE
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                tauri::async_runtime::spawn(task);
                tracing::info!("Loki telemetry background task started");

                // Emit app-start telemetry event now that the task is live.
                let gpu_label = platform::get_gpu_info()
                    .map(|g| g.gpu_name.unwrap_or_else(|| g.compiled_backend.clone()))
                    .unwrap_or_else(|_| "unknown".to_string());
                tracing::info!(
                    target: "telemetry",
                    version = env!("CARGO_PKG_VERSION"),
                    os = std::env::consts::OS,
                    gpu = %gpu_label,
                    "app_start"
                );
            }

            // Store the app handle for the few deep paths that emit user-facing
            // events without a handle of their own (e.g. audio device fallback).
            app_handle::set(app.handle().clone());

            // Request microphone permission BEFORE any audio enumeration.
            // cpal's device enumeration touches CoreAudio which triggers the
            // system mic prompt implicitly — but with no completion handler,
            // so we can't detect the result. By calling requestAccess first,
            // we own the dialog and get the result via our completion handler
            // which emits a permission-changed event to the frontend.
            #[cfg(target_os = "macos")]
            {
                use platform::macos::MicrophoneStatus;
                if platform::macos::check_microphone_permission() == MicrophoneStatus::NotDetermined
                {
                    platform::macos::request_microphone_permission(app.handle().clone());
                }
            }

            // Initialise database early so tray menu queries work immediately
            database::initialise_database().map_err(|e| {
                tracing::error!("Failed to initialise database: {}", e);
                Box::new(e) as Box<dyn std::error::Error>
            })?;

            // Set up system tray
            tray::setup_tray(app)?;

            // Load config and register shortcuts
            if let Ok(cfg) = config::get_config() {
                // Wire up the enhancement backend before the first pipeline run
                config::apply_enhancement_backend(&cfg.enhancement);

                // Register shortcuts from config
                let app_handle = app.handle().clone();
                register_shortcuts_from_config(&app_handle, &cfg);

                // On Linux/Wayland without `wtype`, advise the user once so text
                // insertion does not silently fall back to a permission prompt.
                #[cfg(target_os = "linux")]
                text_insert::emit_linux_typing_advisory(&app_handle);

                // Linux: apply the saved window-decoration preference at startup.
                // With decorations off, the custom in-app close button (the
                // WindowControls component) takes over.
                #[cfg(target_os = "linux")]
                if !cfg.general.window_decorations {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_decorations(false);
                        tracing::info!("Window decorations disabled per config");
                    }
                }

                // Start the Local Control API if enabled in config. The API and
                // MCP server default on. The bearer token lives in its own
                // reset-proof store (control_api::token_store), generated once
                // and migrated from any legacy config.json token, so it never
                // rotates across reinstalls or config resets.
                if cfg.integrations.api_enabled {
                    let token = control_api::token_store::get_or_create_token();
                    let port = cfg.integrations.api_port;
                    let mcp = cfg.integrations.mcp_enabled;
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = control_api::start(port, token, mcp).await {
                            tracing::error!("Control API failed to start on launch: {}", e);
                        }
                    });
                }
            }

            // macOS-specific setup
            #[cfg(target_os = "macos")]
            {
                // Detect an applied update and reset stale macOS permissions.
                //
                // macOS keys TCC grants to the code-signing identity, which
                // changes on each build, so after an update the previously
                // granted microphone / accessibility / input-monitoring
                // permissions silently stop working. When the recorded
                // last-run version differs from this binary's version we reset
                // those three permissions once so the user re-grants from a
                // clean slate. A genuinely fresh install (no recorded version)
                // does NOT trigger a reset — there is nothing stale yet.
                if let Ok(cfg) = config::get_config() {
                    let current = env!("CARGO_PKG_VERSION").to_string();
                    let changed = cfg
                        .general
                        .last_run_version
                        .as_deref()
                        .is_some_and(|prev| prev != current);

                    // Record the current version regardless, so the reset fires
                    // at most once per update.
                    let mut updated = cfg.clone();
                    updated.general.last_run_version = Some(current.clone());
                    if let Err(e) = config::set_config(updated) {
                        tracing::error!("Failed to record last-run version: {}", e);
                    }

                    if changed {
                        tracing::info!(
                            "Update detected ({} → {}); resetting macOS permissions",
                            cfg.general.last_run_version.as_deref().unwrap_or("?"),
                            current
                        );
                        // Spawn so the admin-prompt does not block window setup.
                        tauri::async_runtime::spawn_blocking(|| {
                            match platform::reset_permissions_after_update() {
                                Ok(msg) => tracing::info!("Post-update permission reset: {}", msg),
                                Err(e) => {
                                    tracing::warn!("Post-update permission reset skipped: {}", e)
                                }
                            }
                        });
                    }
                }

                // Set dock visibility based on user config
                let show_dock = config::get_config()
                    .map(|c| c.general.show_in_dock)
                    .unwrap_or(false);
                if show_dock {
                    app.set_activation_policy(tauri::ActivationPolicy::Regular);
                } else {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }

                // Position traffic lights and prevent main window destruction on close.
                // The main window hosts the pipeline event listeners — if it's destroyed,
                // global shortcuts stop working. Hide instead of close.
                if let Some(window) = app.get_webview_window("main") {
                    traffic_lights::setup_traffic_lights(&window, TRAFFIC_LIGHT_X, HEADER_HEIGHT);

                    let win = window.clone();
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = win.hide();
                        }
                    });
                }

                // Log accessibility permission status on startup for diagnostics.
                // The frontend pulls the authoritative state via check_accessibility +
                // verify_accessibility_functional on mount — no event emission needed.
                let has_accessibility = platform::check_accessibility();
                if has_accessibility {
                    if platform::verify_accessibility_functional() {
                        tracing::info!("Accessibility permission granted and functional");
                    } else {
                        tracing::warn!(
                            "Accessibility permission appears granted but is stale — \
                             TCC entry may need resetting"
                        );
                    }
                } else {
                    tracing::warn!(
                        "Accessibility permission not granted - global shortcuts may not work"
                    );
                }
            }

            // Pre-warm the recording indicator window to eliminate first-show delay.
            // This loads the webview content in the background so it's ready instantly.
            recording_indicator::prewarm_indicator_window(app.handle());

            // Initialise mouse tracker for cursor-following recording indicator
            mouse_tracker::init(app.handle());

            // Pre-warm the transcription model to trigger Metal shader compilation.
            // This runs on a background thread so it doesn't block app startup.
            std::thread::spawn(|| {
                transcription::warmup_transcription();
            });

            // Re-warm the model after wake-from-sleep (CoreML cache eviction)
            #[cfg(target_os = "macos")]
            platform::macos::register_wake_observer();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Commands
            commands::greet,
            commands::show_window,
            commands::hide_window,
            commands::toggle_window,
            commands::open_url,
            commands::remove_quarantine,
            commands::open_privacy_pane,
            commands::relaunch_app,
            commands::set_show_in_dock,
            commands::get_show_in_dock,
            commands::set_audio_device,
            commands::get_audio_device,
            // Platform
            platform::check_accessibility,
            platform::request_accessibility,
            platform::verify_accessibility_functional,
            platform::reset_tcc_permissions,
            platform::check_microphone_permission,
            platform::request_microphone_permission,
            platform::get_gpu_info,
            // Audio
            audio::device::list_audio_devices,
            audio::preview::start_audio_preview,
            audio::preview::stop_audio_preview,
            audio::preview::is_audio_preview_running,
            audio::start_recording,
            audio::stop_recording,
            audio::is_recording,
            audio::warm_up_recording,
            // Transcription
            transcription::init_transcription,
            transcription::init_whisper_transcription,
            transcription::init_parakeet_transcription,
            transcription::transcribe_file,
            transcription::is_transcription_ready,
            transcription::get_transcription_backend,
            transcription::get_model_directory,
            transcription::get_whisper_model_directory,
            transcription::is_whisper_model_downloaded,
            transcription::filter_transcription,
            transcription::get_selected_model_id,
            transcription::set_selected_model_id,
            transcription::is_parakeet_available,
            transcription::init_fluidaudio_transcription,
            transcription::is_fluidaudio_available,
            transcription::is_fluidaudio_cached,
            transcription::download::check_model_downloaded,
            transcription::download::get_download_progress,
            transcription::download::download_model,
            transcription::download::get_model_info,
            transcription::download::delete_model,
            transcription::download::reset_download_state,
            transcription::manifest::fetch_model_manifest,
            transcription::manifest::get_manifest_update_time,
            // Enhancement
            enhancement::check_ollama_available,
            enhancement::list_ollama_models,
            enhancement::check_openai_compat_available,
            enhancement::list_openai_compat_models,
            enhancement::enhance_text,
            enhancement::context::get_clipboard_context,
            enhancement::context::build_enhancement_context,
            // Prompt Templates
            enhancement::prompts::get_all_prompts,
            enhancement::prompts::get_builtin_prompts_cmd,
            enhancement::prompts::get_custom_prompts_cmd,
            enhancement::prompts::save_custom_prompt_cmd,
            enhancement::prompts::delete_custom_prompt_cmd,
            enhancement::prompts::get_prompt_by_id,
            // Database
            database::init_database,
            database::get_database_path_command,
            database::transcription::save_transcription,
            database::transcription::get_transcription_by_id,
            database::transcription::list_all_transcriptions,
            database::transcription::delete_transcription_by_id,
            database::transcription::delete_all_transcriptions_cmd,
            database::transcription::reconcile_orphaned_recordings_cmd,
            database::transcription::search_transcriptions_text,
            database::transcription::count_transcriptions_filtered,
            database::transcription::get_transcription_stats_cmd,
            database::insights::get_insights,
            database::insights::get_cruft_candidates,
            database::insights::compute_audio_rms,
            // Trash / quarantine
            database::trash::quarantine_recordings,
            database::trash::restore_recordings,
            database::trash::purge_trash,
            database::trash::list_trash,
            // Export
            export::search_history,
            export::export_to_json,
            export::export_to_csv,
            export::export_to_txt,
            export::get_transcriptions,
            // Config
            config::get_config,
            config::set_config,
            config::set_shortcut_config,
            config::set_enhancement_api_key,
            config::reset_config,
            config::get_config_path_cmd,
            // Shortcuts
            shortcuts::register_shortcut,
            shortcuts::unregister_shortcut,
            shortcuts::list_registered_shortcuts,
            shortcuts::get_default_shortcuts,
            shortcuts::register_default_shortcuts,
            shortcuts::unregister_all_shortcuts,
            shortcuts::try_register_shortcut,
            shortcuts::check_shortcut_available,
            shortcuts::get_shortcut_suggestions,
            shortcuts::validate_shortcut,
            reregister_shortcuts,
            // Dictionary
            dictionary::get_dictionary_entries,
            dictionary::add_dictionary_entry,
            dictionary::update_dictionary_entry,
            dictionary::remove_dictionary_entry,
            dictionary::import_dictionary,
            dictionary::export_dictionary,
            dictionary::apply_dictionary_to_text,
            dictionary::get_vocabulary_for_context,
            // Canonical terms
            canonical::get_canonical_terms,
            canonical::add_canonical_term,
            canonical::update_canonical_term,
            canonical::remove_canonical_term,
            // Text insertion
            text_insert::insert_text,
            text_insert::insert_text_by_typing,
            text_insert::insert_text_by_paste,
            // Clipboard
            clipboard::copy_to_clipboard,
            clipboard::read_clipboard,
            clipboard::clear_clipboard,
            clipboard::copy_transcription,
            clipboard::get_clipboard_settings,
            clipboard::set_clipboard_settings,
            clipboard::get_clipboard_history,
            clipboard::clear_clipboard_history,
            clipboard::remove_clipboard_history_entry,
            clipboard::copy_from_history,
            clipboard::restore_clipboard,
            clipboard::get_restore_delay,
            clipboard::paste_transcription,
            // Sound feedback
            sound::play_recording_start_sound,
            sound::play_recording_stop_sound,
            sound::play_transcription_complete_sound,
            sound::play_error_sound,
            sound::are_sounds_enabled,
            sound::set_sounds_enabled,
            // Pipeline (full recording -> transcription -> output flow)
            pipeline::pipeline_start_recording,
            pipeline::pipeline_stop_and_process,
            pipeline::pipeline_toggle_recording,
            pipeline::pipeline_transcribe_file,
            pipeline::pipeline_retranscribe,
            pipeline::pipeline_cancel,
            pipeline::is_pipeline_running,
            pipeline::get_pipeline_state,
            // Recording indicator
            recording_indicator::show_recording_indicator,
            recording_indicator::hide_recording_indicator,
            // Tray
            tray::get_tray_state_cmd,
            tray::update_tray_recording_state,
            tray::refresh_tray_menu,
            // Storage management
            storage::get_storage_usage,
            storage::delete_all_recordings,
            storage::delete_all_logs,
            storage::delete_fluidaudio_cache,
            storage::delete_all_data,
            // Keyboard service (shortcut capture + modifier monitoring)
            keyboard_service::enter_capture_mode,
            keyboard_service::exit_capture_mode,
            keyboard_service::is_capture_active_cmd,
            keyboard_service::check_input_monitoring,
            keyboard_service::request_input_monitoring,
            keyboard_service::try_start_keyboard_service,
            keyboard_service::report_key_event,
            // Control API
            control_api::get_integrations_status,
            control_api::set_api_enabled,
            control_api::set_mcp_enabled,
            control_api::get_api_token,
            control_api::rotate_api_token,
            control_api::set_api_port,
            // Logging / telemetry
            telemetry::test_loki_connection,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
