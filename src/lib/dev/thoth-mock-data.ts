/**
 * Thoth-specific Tauri command mock map for browser-only dev mode.
 *
 * Casing notes (verified against store source files):
 * - get_config returns snake_case (ConfigRaw in config.svelte.ts)
 * - list_all_transcriptions returns camelCase (TranscriptionRecordRaw in history.svelte.ts)
 * - list_audio_devices returns snake_case id/name/is_default (AudioDevice in settings.svelte.ts)
 * - get_dictionary_entries returns DictionaryEntry with camelCase caseSensitive
 * - list_registered_shortcuts / get_default_shortcuts return ShortcutInfo with
 *   is_enabled (snake_case from Rust); shortcuts.svelte.ts remaps it to isEnabled
 * - fetch_model_manifest returns ModelInfo with all snake_case fields (ModelManager.svelte)
 * - get_all_prompts returns PromptTemplate with isBuiltin camelCase (AIEnhancementSettings.svelte)
 * - get_transcription_stats_cmd returns camelCase (OverviewPane.svelte)
 * - get_gpu_info returns mixed snake/camel (OverviewPane.svelte)
 * - get_storage_usage returns camelCase (StoragePane.svelte)
 */

import type { CommandMap } from './tauri-mock';

// ---------------------------------------------------------------------------
// Seed data
// ---------------------------------------------------------------------------

const MOCK_CONFIG = {
  version: 1,
  audio: { device_id: null, sample_rate: 16000, play_sounds: true },
  transcription: {
    language: 'en',
    auto_copy: false,
    auto_paste: true,
    add_leading_space: false,
    remove_fillers: true,
    australian_spelling: true,
    spoken_numbers_to_digits: false,
  },
  shortcuts: {
    toggle_recording: 'F13',
    toggle_recording_alt: 'CommandOrControl+Shift+Space',
    copy_last: 'F14',
    recording_mode: 'toggle' as const,
  },
  enhancement: {
    enabled: false,
    model: 'llama3.2',
    prompt_id: 'fix-grammar',
    ollama_url: 'http://localhost:11434',
  },
  general: {
    launch_at_login: false,
    show_in_menu_bar: true,
    show_in_dock: false,
    check_for_updates: true,
    show_recording_indicator: true,
    indicator_style: 'cursor-dot' as const,
  },
  recorder: { position: 'top-right' as const, offset_x: -20, offset_y: 20, auto_hide_delay: 3000 },
  integrations: {
    api_enabled: false,
    api_port: 8765,
    mcp_enabled: false,
    api_token: null,
  },
};

const MOCK_MODELS = [
  {
    id: 'whisper-base-en',
    name: 'Whisper Base (English)',
    description: 'Fast, lightweight English-only model',
    version: '1.0.0',
    size_mb: 148,
    downloaded: true,
    path: '/Users/dev/.thoth/models/whisper-base.en',
    disk_size: 155000000,
    recommended: false,
    languages: ['en'],
    update_available: false,
    selected: true,
    model_type: 'whisper',
    backend_available: true,
  },
  {
    id: 'whisper-small-en',
    name: 'Whisper Small (English)',
    description: 'Recommended balance of speed and accuracy',
    version: '1.0.0',
    size_mb: 461,
    downloaded: false,
    path: null,
    disk_size: null,
    recommended: true,
    languages: ['en'],
    update_available: false,
    selected: false,
    model_type: 'whisper',
    backend_available: true,
  },
];

const MOCK_TRANSCRIPTIONS = [
  {
    id: '1',
    text: 'Hello world, this is a mock transcription.',
    createdAt: new Date(Date.now() - 3600 * 1000).toISOString(),
    durationSeconds: 2.4,
    audioPath: null,
    isEnhanced: false,
    rawText: 'Hello world, this is a mock transcription.',
    enhancementPrompt: null,
    transcriptionModelName: 'Whisper Base',
    transcriptionDurationSeconds: 0.8,
    enhancementModelName: null,
    enhancementDurationSeconds: null,
  },
  {
    id: '2',
    text: 'Another example transcription entry in the history pane.',
    createdAt: new Date(Date.now() - 7200 * 1000).toISOString(),
    durationSeconds: 3.1,
    audioPath: null,
    isEnhanced: true,
    rawText: 'another example transcription entry in the history pane',
    enhancementPrompt: 'Fix grammar and punctuation.',
    transcriptionModelName: 'Whisper Base',
    transcriptionDurationSeconds: 1.1,
    enhancementModelName: 'llama3.2',
    enhancementDurationSeconds: 0.9,
  },
];

const MOCK_AUDIO_DEVICES = [
  { id: 'default', name: 'MacBook Pro Microphone', is_default: true },
  { id: 'usb-1', name: 'DJI MIC MINI', is_default: false },
];

const MOCK_DICTIONARY_ENTRIES = [{ from: 'im', to: "I'm", caseSensitive: false }];

const MOCK_SHORTCUTS = [
  { id: 'toggle_recording', accelerator: 'F13', description: 'Toggle recording', is_enabled: true },
  {
    id: 'copy_last',
    accelerator: 'F14',
    description: 'Copy last transcription',
    is_enabled: true,
  },
];

const MOCK_PROMPTS = [
  {
    id: 'fix-grammar',
    name: 'Fix Grammar',
    template: 'Fix grammar and punctuation in the following text.\n\nText: {text}',
    isBuiltin: true,
  },
  {
    id: 'summarise',
    name: 'Summarise',
    template: 'Summarise the following text in one sentence.\n\nText: {text}',
    isBuiltin: true,
  },
];

const MOCK_GPU_INFO = {
  compiled_backend: 'metal',
  gpu_available: true,
  gpu_name: 'Apple M-series',
  vram_mb: null,
  detected_gpus: [{ backend: 'metal', name: 'Apple M-series', vram_mb: null }],
};

const MOCK_STORAGE_USAGE = {
  modelsBytes: 155000000,
  recordingsBytes: 0,
  logsBytes: 4096,
  databaseBytes: 32768,
  configBytes: 1024,
  fluidaudioBytes: 0,
  totalBytes: 155037888,
  recordingCount: 0,
  logCount: 2,
};

const MOCK_TRANSCRIPTION_STATS = {
  totalCount: 2,
  analysableCount: 2,
  enhancedCount: 1,
  totalAudioDuration: 5.5,
  transcriptionModels: [{ name: 'Whisper Base', count: 2 }],
  enhancementModels: [{ name: 'llama3.2', count: 1 }],
};

const MOCK_CLIPBOARD_SETTINGS = {
  enabled: false,
  maxHistory: 20,
  persistAcrossRestarts: false,
};

// ---------------------------------------------------------------------------
// Command map
// ---------------------------------------------------------------------------

export const thothMockCommands: CommandMap = {
  // -- Startup-critical (must resolve for initialise() to complete) --
  get_config: () => MOCK_CONFIG,
  init_database: () => undefined,
  is_transcription_ready: () => true,
  get_model_directory: () => '/Users/dev/.thoth/models',
  check_model_downloaded: () => true,
  init_transcription: () => undefined,
  get_pipeline_state: () => 'idle',

  // -- Sounds (called during soundStore.load()) --
  are_sounds_enabled: () => true,

  // -- Shortcuts store initialise() --
  get_default_shortcuts: () => MOCK_SHORTCUTS,
  list_registered_shortcuts: () => MOCK_SHORTCUTS,

  // -- Settings store initialise() (loads audio devices) --
  list_audio_devices: () => MOCK_AUDIO_DEVICES,

  // -- Model manager --
  fetch_model_manifest: () => MOCK_MODELS,
  get_download_progress: () => 'Idle',
  get_manifest_update_time: () => new Date(Date.now() - 3600 * 1000).toISOString(),

  // -- History pane --
  list_all_transcriptions: () => MOCK_TRANSCRIPTIONS,

  // -- Dictionary pane --
  get_dictionary_entries: () => MOCK_DICTIONARY_ENTRIES,

  // -- AI enhancement pane --
  get_all_prompts: () => MOCK_PROMPTS,
  check_ollama_available: () => false,
  list_ollama_models: () => [],

  // -- Overview / performance panes --
  get_transcription_stats_cmd: () => MOCK_TRANSCRIPTION_STATS,
  get_gpu_info: () => MOCK_GPU_INFO,

  // -- Storage pane --
  get_storage_usage: () => MOCK_STORAGE_USAGE,

  // -- Permissions pane --
  check_accessibility: () => true,
  check_input_monitoring: () => true,
  verify_accessibility_functional: () => true,
  check_microphone_permission: () => 'granted',

  // -- Config path (settings pane) --
  get_config_path_cmd: () => '/Users/dev/.thoth/config.json',
  get_show_in_dock: () => false,
  reset_config: () => MOCK_CONFIG,

  // -- Clipboard store --
  get_clipboard_settings: () => MOCK_CLIPBOARD_SETTINGS,
  get_clipboard_history: () => [],

  // -- Prompt resolution (pipeline) --
  get_prompt_by_id: (args) => {
    const id = (args as { promptId?: string } | undefined)?.promptId;
    return MOCK_PROMPTS.find((p) => p.id === id) ?? null;
  },

  // -- No-ops: write commands (set_*, delete_*, play_*, etc.) --
  set_config: () => undefined,
  set_audio_device: () => undefined,
  set_sounds_enabled: () => undefined,
  set_selected_model_id: () => undefined,
  set_show_in_dock: () => undefined,
  set_clipboard_settings: () => undefined,
  set_shortcut_config: () => undefined,
  register_shortcut: () => undefined,
  unregister_shortcut: () => undefined,
  register_default_shortcuts: () => undefined,
  unregister_all_shortcuts: () => undefined,
  try_register_shortcut: () => ({
    type: 'Success',
    shortcut: 'F13',
    shortcut_id: 'toggle_recording',
  }),
  check_shortcut_available: () => true,
  get_shortcut_suggestions: () => [],
  reregister_shortcuts: () => undefined,
  add_dictionary_entry: () => undefined,
  update_dictionary_entry: () => undefined,
  remove_dictionary_entry: () => undefined,
  import_dictionary: () => 0,
  export_dictionary: () => '[]',
  apply_dictionary_to_text: (args) => (args as { text?: string } | undefined)?.text ?? '',
  save_custom_prompt_cmd: () => undefined,
  delete_custom_prompt_cmd: () => undefined,
  download_model: () => undefined,
  delete_model: () => undefined,
  reset_download_state: () => undefined,
  delete_fluidaudio_cache: () => undefined,
  init_fluidaudio_transcription: () => undefined,
  delete_transcription_by_id: () => undefined,
  delete_all_transcriptions_cmd: () => undefined,
  copy_transcription: () => true,
  paste_transcription: () => undefined,
  insert_text: () => undefined,
  insert_text_by_paste: () => undefined,
  insert_text_by_typing: () => undefined,
  copy_to_clipboard: () => undefined,
  copy_from_history: () => undefined,
  remove_clipboard_history_entry: () => false,
  restore_clipboard: () => false,
  clear_clipboard_history: () => undefined,
  play_recording_start_sound: () => undefined,
  play_recording_stop_sound: () => undefined,
  play_transcription_complete_sound: () => undefined,
  play_error_sound: () => undefined,
  pipeline_toggle_recording: () => ({ action: 'started', path: '/tmp/mock.wav' }),
  pipeline_cancel: () => undefined,
  pipeline_transcribe_file: () => ({
    success: true,
    text: 'Mock file transcription.',
    rawText: 'Mock file transcription.',
    isEnhanced: false,
    durationSeconds: 2.0,
    audioPath: null,
    error: null,
    transcriptionId: null,
  }),
  show_recording_indicator: () => undefined,
  hide_recording_indicator: () => undefined,
  show_window: () => undefined,
  hide_recorder: () => undefined,
  position_recorder_window: () => undefined,
  refresh_tray_menu: () => undefined,
  open_url: () => undefined,
  open_privacy_pane: () => undefined,
  relaunch_app: () => undefined,
  delete_all_data: () => undefined,
  delete_all_recordings: () => 0,
  delete_all_logs: () => 0,
  export_to_csv: () => 0,
  export_to_json: () => 0,
  export_to_txt: () => 0,
  filter_transcription: (args) => (args as { text?: string } | undefined)?.text ?? '',
  enter_capture_mode: () => 'F13',
  exit_capture_mode: () => undefined,
  report_key_event: () => undefined,
  try_start_keyboard_service: () => undefined,
  request_input_monitoring: () => undefined,
  reset_tcc_permissions: () => '',
  remove_quarantine: () => undefined,
  start_audio_preview: () => undefined,
  stop_audio_preview: () => undefined,

  // -- Integrations (Local Control API + MCP server) --
  get_integrations_status: () => ({
    apiEnabled: false,
    apiRunning: false,
    apiPort: 8765,
    mcpEnabled: false,
    hasToken: true,
  }),
  get_api_token: () => 'thoth-dev-0000-1111-2222-3333-444455556666',
  set_api_enabled: () => undefined,
  set_mcp_enabled: () => undefined,
  set_api_port: () => undefined,
  rotate_api_token: () => 'thoth-dev-rotated-aaaa-bbbb-cccc-ddddeeeeffff',
};
