//! Thoth - Privacy-first voice transcription
//!
//! Desktop application for macOS and Linux.

use tauri::Manager;

pub mod audio;
pub mod clipboard;
pub mod commands;
pub mod config;
pub mod database;
pub mod dictionary;
pub mod enhancement;
pub mod export;
pub mod handsfree;
pub mod keyboard_capture;
pub mod modifier_monitor;
pub mod mouse_tracker;
pub mod pipeline;
pub mod platform;
pub mod ptt;
pub mod recording_indicator;
pub mod shortcuts;
pub mod sound;
pub mod text_insert;
mod traffic_lights;
pub mod transcription;
pub mod tray;

/// Header height in pixels (must match CSS --header-height)
const HEADER_HEIGHT: f64 = 52.0;

/// Traffic light X position (left margin)
const TRAFFIC_LIGHT_X: f64 = 13.0;

/// Register a single shortcut, using modifier monitor for standalone modifiers
fn register_single_shortcut(
    app: &tauri::AppHandle,
    id: &str,
    accelerator: &str,
    description: &str,
) -> Result<(), String> {
    // Check if this is a standalone modifier shortcut (e.g., ShiftRight)
    if modifier_monitor::is_modifier_shortcut(accelerator) {
        // Register with modifier monitor instead of global shortcut
        if modifier_monitor::register_modifier_shortcut(
            id.to_string(),
            accelerator.to_string(),
            description.to_string(),
        ) {
            // Restart the monitor to pick up the new shortcut
            modifier_monitor::restart_monitor(app.clone())?;
            Ok(())
        } else {
            Err(format!(
                "Failed to register modifier shortcut: {}",
                accelerator
            ))
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

/// Register shortcuts from saved configuration
fn register_shortcuts_from_config(app: &tauri::AppHandle, cfg: &config::Config) {
    use shortcuts::manager::shortcut_ids;

    // Toggle recording shortcut
    if !cfg.shortcuts.toggle_recording.is_empty() {
        if let Err(e) = register_single_shortcut(
            app,
            shortcut_ids::TOGGLE_RECORDING,
            &cfg.shortcuts.toggle_recording,
            "Toggle recording",
        ) {
            tracing::warn!("Failed to register toggle_recording shortcut: {}", e);
        } else {
            tracing::info!(
                "Registered toggle_recording shortcut: {}",
                cfg.shortcuts.toggle_recording
            );
        }
    }

    // Alternative toggle recording shortcut
    if let Some(ref alt) = cfg.shortcuts.toggle_recording_alt {
        if !alt.is_empty() {
            if let Err(e) = register_single_shortcut(
                app,
                shortcut_ids::TOGGLE_RECORDING_ALT,
                alt,
                "Toggle recording (alternative)",
            ) {
                tracing::warn!("Failed to register toggle_recording_alt shortcut: {}", e);
            } else {
                tracing::info!("Registered toggle_recording_alt shortcut: {}", alt);
            }
        }
    }

    // Copy last transcription shortcut
    if let Some(ref copy) = cfg.shortcuts.copy_last {
        if !copy.is_empty() {
            if let Err(e) = register_single_shortcut(
                app,
                shortcut_ids::COPY_LAST_TRANSCRIPTION,
                copy,
                "Copy last transcription",
            ) {
                tracing::warn!("Failed to register copy_last shortcut: {}", e);
            } else {
                tracing::info!("Registered copy_last shortcut: {}", copy);
            }
        }
    }

    // Start the modifier monitor if any modifier shortcuts were registered
    if let Err(e) = modifier_monitor::start_monitor(app.clone()) {
        tracing::warn!("Failed to start modifier monitor: {}", e);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Set up file-based logging for debugging (local time for readability)
    use tracing_subscriber::prelude::*;

    /// Format timestamps using the system's local time via chrono
    struct LocalTimer;
    impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
        fn format_time(
            &self,
            w: &mut tracing_subscriber::fmt::format::Writer<'_>,
        ) -> std::fmt::Result {
            write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"))
        }
    }

    let log_dir = dirs::home_dir()
        .map(|h| h.join(".thoth").join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("thoth-debug.log"))
        .ok();

    if let Some(file) = log_file {
        let file_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::sync::Mutex::new(file))
            .with_timer(LocalTimer)
            .with_ansi(false);
        let stdout_layer = tracing_subscriber::fmt::layer().with_timer(LocalTimer);
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with(stdout_layer)
            .with(file_layer)
            .init();
    } else {
        tracing_subscriber::fmt().with_timer(LocalTimer).init();
    }

    let mut builder = tauri::Builder::default()
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

            // Initialise database early so tray menu queries work immediately
            database::initialise_database().map_err(|e| {
                tracing::error!("Failed to initialise database: {}", e);
                Box::new(e) as Box<dyn std::error::Error>
            })?;

            // Set up system tray
            tray::setup_tray(app)?;

            // Load config and register shortcuts
            if let Ok(cfg) = config::get_config() {
                // Register shortcuts from config
                let app_handle = app.handle().clone();
                register_shortcuts_from_config(&app_handle, &cfg);

                // Sync PTT mode from config
                let ptt_enabled = cfg.shortcuts.recording_mode == config::RecordingMode::PushToTalk;
                if let Err(e) = ptt::set_ptt_mode_enabled(ptt_enabled) {
                    tracing::warn!("Failed to set initial PTT mode: {}", e);
                } else {
                    tracing::info!(
                        "Recording mode initialised: {}",
                        if ptt_enabled {
                            "push-to-talk"
                        } else {
                            "toggle"
                        }
                    );
                }
            }

            // macOS-specific setup
            #[cfg(target_os = "macos")]
            {
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

                // Check accessibility permission on startup
                let has_accessibility = platform::check_accessibility();
                if has_accessibility {
                    tracing::info!("Accessibility permission granted");
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Commands
            commands::greet,
            commands::show_window,
            commands::hide_window,
            commands::toggle_window,
            commands::open_url,
            commands::set_show_in_dock,
            commands::get_show_in_dock,
            commands::set_audio_device,
            commands::get_audio_device,
            // Platform
            platform::check_accessibility,
            platform::request_accessibility,
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
            audio::set_vad_enabled,
            audio::is_vad_enabled,
            audio::get_vad_config_cmd,
            audio::set_vad_config_cmd,
            audio::set_vad_aggressiveness,
            audio::set_vad_frame_duration,
            audio::set_vad_speech_start_frames,
            audio::set_vad_speech_end_frames,
            audio::set_vad_pre_speech_padding,
            audio::set_vad_post_speech_padding,
            audio::get_vad_status,
            audio::start_recording_with_vad,
            audio::stop_recording_with_vad,
            audio::is_recording_with_vad,
            audio::was_vad_auto_stop_triggered,
            audio::set_vad_auto_stop_silence,
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
            database::transcription::search_transcriptions_text,
            database::transcription::count_transcriptions_filtered,
            database::transcription::get_transcription_stats_cmd,
            // Export
            export::search_history,
            export::export_to_json,
            export::export_to_csv,
            export::export_to_txt,
            export::get_transcriptions,
            // Config
            config::get_config,
            config::set_config,
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
            // Dictionary
            dictionary::get_dictionary_entries,
            dictionary::add_dictionary_entry,
            dictionary::update_dictionary_entry,
            dictionary::remove_dictionary_entry,
            dictionary::import_dictionary,
            dictionary::export_dictionary,
            dictionary::apply_dictionary_to_text,
            dictionary::get_vocabulary_for_context,
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
            // Push-to-talk
            ptt::ptt_key_down,
            ptt::ptt_key_up,
            ptt::is_ptt_active,
            ptt::set_ptt_mode_enabled,
            ptt::is_ptt_mode_enabled,
            ptt::ptt_cancel,
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
            pipeline::pipeline_cancel,
            pipeline::is_pipeline_running,
            pipeline::get_pipeline_state,
            // Hands-free (VAD-based automatic recording)
            handsfree::manager::set_handsfree_enabled,
            handsfree::manager::is_handsfree_enabled,
            handsfree::manager::get_handsfree_status,
            handsfree::manager::get_handsfree_state,
            handsfree::manager::handsfree_activate,
            handsfree::manager::handsfree_cancel,
            handsfree::manager::handsfree_acknowledge,
            handsfree::manager::handsfree_timeout,
            handsfree::manager::set_handsfree_timeout,
            handsfree::manager::get_handsfree_timeout,
            handsfree::manager::get_last_handsfree_transcription,
            handsfree::manager::reset_handsfree_state,
            // Recording indicator
            recording_indicator::show_recording_indicator,
            recording_indicator::hide_recording_indicator,
            // Tray
            tray::get_tray_state_cmd,
            tray::update_tray_recording_state,
            tray::refresh_tray_menu,
            // Keyboard capture (for shortcut recording)
            keyboard_capture::start_key_capture,
            keyboard_capture::stop_key_capture,
            keyboard_capture::is_key_capture_active,
            keyboard_capture::check_input_monitoring,
            keyboard_capture::request_input_monitoring,
            keyboard_capture::report_key_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
