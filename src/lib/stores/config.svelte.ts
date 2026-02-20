/**
 * Configuration state management store using Svelte 5 runes.
 *
 * Manages application settings with persistence via the Tauri backend.
 * Settings are stored in ~/.thoth/config.json and support schema migrations.
 */

import { invoke } from '@tauri-apps/api/core';

/** Audio recording configuration */
export interface AudioConfig {
  /** Selected audio input device ID (null for system default) */
  deviceId: string | null;
  /** Sample rate in Hz */
  sampleRate: number;
  /** Whether to play audio feedback sounds */
  playSounds: boolean;
}

/** Transcription engine configuration */
export interface TranscriptionConfig {
  /** Transcription language code (e.g., "en", "auto") */
  language: string;
  /** Whether to automatically copy transcription to clipboard */
  autoCopy: boolean;
  /** Whether to automatically paste transcription at cursor */
  autoPaste: boolean;
  /** Whether to add space before pasted text */
  addLeadingSpace: boolean;
}

/** Recording mode options */
export type RecordingMode = 'toggle' | 'push_to_talk';

/** Keyboard shortcut configuration */
export interface ShortcutConfig {
  /** Toggle recording shortcut (e.g., "F13") */
  toggleRecording: string;
  /** Alternative toggle recording shortcut */
  toggleRecordingAlt: string | null;
  /** Copy last transcription shortcut */
  copyLast: string | null;
  /** Recording mode: toggle or push-to-talk */
  recordingMode: RecordingMode;
}

/** AI enhancement configuration */
export interface EnhancementConfig {
  /** Whether AI enhancement is enabled */
  enabled: boolean;
  /** Ollama model to use for enhancement */
  model: string;
  /** Selected prompt template ID */
  promptId: string;
  /** Ollama server URL */
  ollamaUrl: string;
}

/** General application settings */
export interface GeneralConfig {
  /** Launch application on system startup */
  launchAtLogin: boolean;
  /** Show menu bar icon */
  showInMenuBar: boolean;
  /** Show dock icon (macOS) */
  showInDock: boolean;
  /** Automatically check for updates on launch */
  checkForUpdates: boolean;
  /** Show the floating recording indicator during recording */
  showRecordingIndicator: boolean;
}

/** Recorder window position options */
export type RecorderPosition =
  | 'cursor'
  | 'tray-icon'
  | 'top-left'
  | 'top-right'
  | 'bottom-left'
  | 'bottom-right'
  | 'centre';

/** Recorder window configuration */
export interface RecorderConfig {
  /** Window position preference */
  position: RecorderPosition;
  /** Horizontal offset from position anchor (in pixels) */
  offsetX: number;
  /** Vertical offset from position anchor (in pixels) */
  offsetY: number;
  /** Auto-hide delay in milliseconds after transcription completes (0 = no auto-hide) */
  autoHideDelay: number;
}

/** Main configuration structure */
export interface Config {
  /** Schema version for migrations */
  version: number;
  /** Audio recording settings */
  audio: AudioConfig;
  /** Transcription settings */
  transcription: TranscriptionConfig;
  /** Keyboard shortcut settings */
  shortcuts: ShortcutConfig;
  /** AI enhancement settings */
  enhancement: EnhancementConfig;
  /** General application settings */
  general: GeneralConfig;
  /** Recorder window settings */
  recorder: RecorderConfig;
}

/** Raw config from backend (snake_case fields) */
interface ConfigRaw {
  version: number;
  audio: {
    device_id: string | null;
    sample_rate: number;
    play_sounds: boolean;
  };
  transcription: {
    language: string;
    auto_copy: boolean;
    auto_paste: boolean;
    add_leading_space: boolean;
  };
  shortcuts: {
    toggle_recording: string;
    toggle_recording_alt: string | null;
    copy_last: string | null;
    recording_mode: RecordingMode;
  };
  enhancement: {
    enabled: boolean;
    model: string;
    prompt_id: string;
    ollama_url: string;
  };
  general: {
    launch_at_login: boolean;
    show_in_menu_bar: boolean;
    show_in_dock: boolean;
    check_for_updates: boolean;
    show_recording_indicator: boolean;
  };
  recorder: {
    position: RecorderPosition;
    offset_x: number;
    offset_y: number;
    auto_hide_delay: number;
  };
}

/** Convert raw backend config to frontend format (snake_case to camelCase) */
function parseConfig(raw: ConfigRaw): Config {
  return {
    version: raw.version,
    audio: {
      deviceId: raw.audio.device_id,
      sampleRate: raw.audio.sample_rate,
      playSounds: raw.audio.play_sounds,
    },
    transcription: {
      language: raw.transcription.language,
      autoCopy: raw.transcription.auto_copy,
      autoPaste: raw.transcription.auto_paste,
      addLeadingSpace: raw.transcription.add_leading_space,
    },
    shortcuts: {
      toggleRecording: raw.shortcuts.toggle_recording,
      toggleRecordingAlt: raw.shortcuts.toggle_recording_alt,
      copyLast: raw.shortcuts.copy_last,
      recordingMode: raw.shortcuts.recording_mode,
    },
    enhancement: {
      enabled: raw.enhancement.enabled,
      model: raw.enhancement.model,
      promptId: raw.enhancement.prompt_id,
      ollamaUrl: raw.enhancement.ollama_url,
    },
    general: {
      launchAtLogin: raw.general.launch_at_login,
      showInMenuBar: raw.general.show_in_menu_bar,
      showInDock: raw.general.show_in_dock,
      checkForUpdates: raw.general.check_for_updates,
      showRecordingIndicator: raw.general.show_recording_indicator,
    },
    recorder: {
      position: raw.recorder.position,
      offsetX: raw.recorder.offset_x,
      offsetY: raw.recorder.offset_y,
      autoHideDelay: raw.recorder.auto_hide_delay,
    },
  };
}

/** Convert frontend config to backend format (camelCase to snake_case) */
function serialiseConfig(config: Config): ConfigRaw {
  return {
    version: config.version,
    audio: {
      device_id: config.audio.deviceId,
      sample_rate: config.audio.sampleRate,
      play_sounds: config.audio.playSounds,
    },
    transcription: {
      language: config.transcription.language,
      auto_copy: config.transcription.autoCopy,
      auto_paste: config.transcription.autoPaste,
      add_leading_space: config.transcription.addLeadingSpace,
    },
    shortcuts: {
      toggle_recording: config.shortcuts.toggleRecording,
      toggle_recording_alt: config.shortcuts.toggleRecordingAlt,
      copy_last: config.shortcuts.copyLast,
      recording_mode: config.shortcuts.recordingMode,
    },
    enhancement: {
      enabled: config.enhancement.enabled,
      model: config.enhancement.model,
      prompt_id: config.enhancement.promptId,
      ollama_url: config.enhancement.ollamaUrl,
    },
    general: {
      launch_at_login: config.general.launchAtLogin,
      show_in_menu_bar: config.general.showInMenuBar,
      show_in_dock: config.general.showInDock,
      check_for_updates: config.general.checkForUpdates,
      show_recording_indicator: config.general.showRecordingIndicator,
    },
    recorder: {
      position: config.recorder.position,
      offset_x: config.recorder.offsetX,
      offset_y: config.recorder.offsetY,
      auto_hide_delay: config.recorder.autoHideDelay,
    },
  };
}

/** Default configuration values */
function getDefaultConfig(): Config {
  return {
    version: 1,
    audio: {
      deviceId: null,
      sampleRate: 16000,
      playSounds: true,
    },
    transcription: {
      language: 'en',
      autoCopy: false,
      autoPaste: true,
      addLeadingSpace: false,
    },
    shortcuts: {
      toggleRecording: 'F13',
      toggleRecordingAlt: 'CommandOrControl+Shift+Space',
      copyLast: 'F14',
      recordingMode: 'toggle',
    },
    enhancement: {
      enabled: false,
      model: 'llama3.2',
      promptId: 'fix-grammar',
      ollamaUrl: 'http://localhost:11434',
    },
    general: {
      launchAtLogin: false,
      showInMenuBar: true,
      showInDock: false,
      checkForUpdates: true,
      showRecordingIndicator: true,
    },
    recorder: {
      position: 'top-right',
      offsetX: -20,
      offsetY: 20,
      autoHideDelay: 3000,
    },
  };
}

/** Create the configuration store with reactive state */
function createConfigStore() {
  let config = $state<Config>(getDefaultConfig());
  let isLoading = $state<boolean>(false);
  let isSaving = $state<boolean>(false);
  let error = $state<string | null>(null);
  let isInitialised = $state<boolean>(false);

  /**
   * Load configuration from the backend
   */
  async function load(): Promise<void> {
    isLoading = true;
    error = null;

    try {
      const rawConfig = await invoke<ConfigRaw>('get_config');
      config = parseConfig(rawConfig);
      isInitialised = true;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load configuration';
      console.error('Failed to load config:', e);
    } finally {
      isLoading = false;
    }
  }

  /**
   * Save configuration to the backend
   *
   * Refuses to save if the config hasn't been loaded from the backend yet,
   * preventing accidental overwrite of persisted settings with in-memory defaults.
   */
  async function save(): Promise<boolean> {
    if (!isInitialised) {
      console.warn(
        '[ConfigStore] save() called before config was loaded — ignoring to prevent overwriting persisted settings with defaults'
      );
      return false;
    }

    isSaving = true;
    error = null;

    try {
      const rawConfig = serialiseConfig(config);
      await invoke('set_config', { config: rawConfig });
      return true;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to save configuration';
      console.error('Failed to save config:', e);
      return false;
    } finally {
      isSaving = false;
    }
  }

  /**
   * Reset configuration to defaults
   */
  async function reset(): Promise<boolean> {
    isLoading = true;
    error = null;

    try {
      const rawConfig = await invoke<ConfigRaw>('reset_config');
      config = parseConfig(rawConfig);
      return true;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to reset configuration';
      console.error('Failed to reset config:', e);
      return false;
    } finally {
      isLoading = false;
    }
  }

  /**
   * Get the configuration file path
   */
  async function getConfigPath(): Promise<string> {
    return invoke<string>('get_config_path_cmd');
  }

  /**
   * Update a specific audio config field
   */
  function updateAudio<K extends keyof AudioConfig>(key: K, value: AudioConfig[K]): void {
    config.audio[key] = value;
  }

  /**
   * Update a specific transcription config field
   */
  function updateTranscription<K extends keyof TranscriptionConfig>(
    key: K,
    value: TranscriptionConfig[K]
  ): void {
    config.transcription[key] = value;
  }

  /**
   * Update a specific shortcut config field
   */
  function updateShortcuts<K extends keyof ShortcutConfig>(key: K, value: ShortcutConfig[K]): void {
    config.shortcuts[key] = value;
  }

  /**
   * Update a specific enhancement config field
   */
  function updateEnhancement<K extends keyof EnhancementConfig>(
    key: K,
    value: EnhancementConfig[K]
  ): void {
    config.enhancement[key] = value;
  }

  /**
   * Update a specific general config field
   */
  function updateGeneral<K extends keyof GeneralConfig>(key: K, value: GeneralConfig[K]): void {
    config.general[key] = value;
  }

  /**
   * Update a specific recorder config field
   */
  function updateRecorder<K extends keyof RecorderConfig>(key: K, value: RecorderConfig[K]): void {
    config.recorder[key] = value;
  }

  /**
   * Clear error state
   */
  function clearError(): void {
    error = null;
  }

  return {
    // State (getters for reactive access)
    get config() {
      return config;
    },
    get isLoading() {
      return isLoading;
    },
    get isSaving() {
      return isSaving;
    },
    get error() {
      return error;
    },
    get isInitialised() {
      return isInitialised;
    },

    // Shorthand accessors for common config sections
    get audio() {
      return config.audio;
    },
    get transcription() {
      return config.transcription;
    },
    get shortcuts() {
      return config.shortcuts;
    },
    get enhancement() {
      return config.enhancement;
    },
    get general() {
      return config.general;
    },
    get recorder() {
      return config.recorder;
    },

    // Actions
    load,
    save,
    reset,
    getConfigPath,
    updateAudio,
    updateTranscription,
    updateShortcuts,
    updateEnhancement,
    updateGeneral,
    updateRecorder,
    clearError,
  };
}

/** Singleton configuration store instance */
export const configStore = createConfigStore();
