/**
 * Pipeline state management for the recording to transcription to output flow.
 *
 * This store orchestrates the complete transcription pipeline:
 * 1. Recording audio
 * 2. Transcribing audio to text
 * 3. Filtering and dictionary replacements
 * 4. Optional AI enhancement
 * 5. Output (clipboard copy and/or paste at cursor)
 * 6. Saving to history
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { configStore } from './config.svelte';
import { settingsStore } from './settings.svelte';

/** Debug logging — only active in development builds */
const debug = import.meta.env.DEV
  ? (...args: unknown[]) => console.log('[Pipeline]', ...args)
  : () => {};
import { soundStore } from './sound.svelte';

/** Pipeline execution states */
export type PipelineState =
  | 'idle'
  | 'recording'
  | 'converting'
  | 'transcribing'
  | 'filtering'
  | 'enhancing'
  | 'outputting'
  | 'completed'
  | 'failed';

/** Pipeline configuration for execution */
export interface PipelineConfig {
  /** Whether to apply dictionary replacements */
  applyDictionary: boolean;
  /** Whether to apply output filtering (filler words, formatting) */
  applyFiltering: boolean;
  /** Whether AI enhancement is enabled */
  enhancementEnabled: boolean;
  /** Ollama model for enhancement */
  enhancementModel: string;
  /** Enhancement prompt template */
  enhancementPrompt: string;
  /** Whether to auto-copy to clipboard */
  autoCopy: boolean;
  /** Whether to auto-paste at cursor */
  autoPaste: boolean;
  /** Insertion method: "typing" or "paste" */
  insertionMethod: string;
}

/** Pipeline execution result */
export interface PipelineResult {
  /** Whether the pipeline completed successfully */
  success: boolean;
  /** Final transcribed text (after all processing) */
  text: string;
  /** Raw transcription text (before filtering/enhancement) */
  rawText: string;
  /** Whether the text was enhanced by AI */
  isEnhanced: boolean;
  /** Duration of the audio in seconds */
  durationSeconds: number | null;
  /** Path to the audio file */
  audioPath: string | null;
  /** Error message if the pipeline failed */
  error: string | null;
  /** ID of the saved transcription record */
  transcriptionId: string | null;
}

/** Progress event from the backend */
interface PipelineProgress {
  state: PipelineState;
  message: string;
}

/** Default enhancement prompt */
const DEFAULT_ENHANCEMENT_PROMPT = `Fix grammar and punctuation in the following text.
Keep the original meaning and tone. Output only the corrected text, nothing else.

Text: {text}`;

/** Resolve the configured prompt ID to its template text */
async function resolveEnhancementPrompt(promptId: string): Promise<string> {
  try {
    const prompt = await invoke<{ template: string } | null>('get_prompt_by_id', {
      promptId,
    });
    if (prompt?.template) {
      return prompt.template;
    }
  } catch (e) {
    console.warn('[Pipeline] Failed to resolve prompt by ID, using default:', e);
  }
  return DEFAULT_ENHANCEMENT_PROMPT;
}

/** Create the default pipeline configuration based on app settings */
async function getDefaultConfig(): Promise<PipelineConfig> {
  const config = configStore.config;
  const enhancementPrompt = config.enhancement.enabled
    ? await resolveEnhancementPrompt(config.enhancement.promptId)
    : DEFAULT_ENHANCEMENT_PROMPT;

  return {
    applyDictionary: true,
    applyFiltering: true,
    enhancementEnabled: config.enhancement.enabled,
    enhancementModel: config.enhancement.model,
    enhancementPrompt,
    autoCopy: config.transcription.autoCopy,
    autoPaste: config.transcription.autoPaste && settingsStore.autoPaste,
    insertionMethod: 'paste',
  };
}

/** Create the pipeline store */
function createPipelineStore() {
  // Reactive state
  let state = $state<PipelineState>('idle');
  let message = $state<string>('');
  let isRunning = $state<boolean>(false);
  let lastResult = $state<PipelineResult | null>(null);
  let error = $state<string | null>(null);
  let audioPath = $state<string | null>(null);
  let recordingStartTime = $state<number | null>(null);
  let recordingDuration = $state<number>(0);

  // Event listeners
  let unlisteners: UnlistenFn[] = [];
  let isInitialised = false;

  // Toggle cooldown: after a state change (start/stop), ignore further toggles
  // for this duration. Prevents accidental double-toggles from immediately reversing the action.
  // Key bounce is handled by the Rust shortcut debounce (50ms); this is a higher-level guard.
  const TOGGLE_COOLDOWN_MS = 300;
  let lastToggleTime = 0;
  let isToggling = false;

  /**
   * Initialise the pipeline store and set up event listeners
   */
  async function initialise(): Promise<void> {
    debug('initialise() called, isInitialised:', isInitialised, 'unlisteners:', unlisteners.length);
    // Always clean up existing listeners first (handles HMR where state may be stale)
    if (unlisteners.length > 0) {
      debug(' Cleaning up existing listeners first');
      cleanup();
    }
    isInitialised = true;

    // Listen for progress events
    const progressUnlisten = await listen<PipelineProgress>('pipeline-progress', (event) => {
      debug(' Progress event received:', event.payload);
      state = event.payload.state;
      message = event.payload.message;
      isRunning = state !== 'idle' && state !== 'completed' && state !== 'failed';
      debug(' State updated to:', state, 'isRunning:', isRunning);
    });
    unlisteners.push(progressUnlisten);

    // Listen for completion events
    const completeUnlisten = await listen<PipelineResult>('pipeline-complete', (event) => {
      debug(' pipeline-complete event received:', event.payload);
      lastResult = event.payload;
      state = 'completed';
      isRunning = false;
      recordingStartTime = null;
      debug(' State set to completed from event');
    });
    unlisteners.push(completeUnlisten);

    // Listen for cancellation events
    const cancelUnlisten = await listen('pipeline-cancelled', () => {
      state = 'idle';
      isRunning = false;
      recordingStartTime = null;
      recordingDuration = 0;
    });
    unlisteners.push(cancelUnlisten);

    // Listen for shortcut events to trigger recording
    debug(' Setting up shortcut-triggered listener...');
    const shortcutUnlisten = await listen<string>('shortcut-triggered', async (event) => {
      const shortcutId = event.payload;
      const timestamp = new Date().toISOString();
      debug(
        `${timestamp} Shortcut event received:`,
        shortcutId,
        'current state:',
        state,
        'isRunning:',
        isRunning
      );

      // Handle toggle recording shortcuts
      if (shortcutId === 'toggle_recording' || shortcutId === 'toggle_recording_alt') {
        debug(`${timestamp} Calling toggleRecording...`);
        await toggleRecording();
        debug(`${timestamp} toggleRecording completed, new state:`, state);
      }
    });
    debug(' shortcut-triggered listener registered');
    unlisteners.push(shortcutUnlisten);

    // Sync with backend state (e.g., after hot reload)
    try {
      const running = await invoke<boolean>('is_pipeline_running');
      if (running) {
        // Backend is running, sync our state
        const backendState = await invoke<string>('get_pipeline_state');
        debug(' Syncing with backend state:', backendState, 'running:', running);
        state = backendState as PipelineState;
        isRunning = true;
      }
    } catch (e) {
      console.warn('[Pipeline] Failed to sync with backend state:', e);
    }

    debug(' Initialization complete, state:', state, 'isRunning:', isRunning);
  }

  /**
   * Clean up event listeners
   */
  function cleanup(): void {
    for (const unlisten of unlisteners) {
      unlisten();
    }
    unlisteners = [];
    isInitialised = false;
  }

  /**
   * Start recording
   */
  async function startRecording(): Promise<{ success: boolean; error?: string }> {
    debug(' startRecording() called, isRunning:', isRunning);
    if (isRunning) {
      debug(' Already running, returning early');
      return { success: false, error: 'Pipeline is already running' };
    }

    error = null;
    lastResult = null;

    // NOTE: Recording indicator is shown directly from the Rust shortcut handler
    // for instant response. No need to call it from here.

    // Play the dictation-style begin tone when starting
    soundStore.playRecordingStart();

    try {
      debug(' Calling pipeline_start_recording...');
      const path = await invoke<string>('pipeline_start_recording');
      debug(' Recording started at path:', path);
      audioPath = path;
      state = 'recording';
      isRunning = true;
      recordingStartTime = Date.now();
      recordingDuration = 0;

      // Start duration timer
      startDurationTimer();

      return { success: true };
    } catch (e) {
      const errorMsg = `${e}`;
      error = errorMsg;
      state = 'failed';
      // Hide indicator on failure
      invoke('hide_recording_indicator').catch(() => {});
      // Play error sound
      soundStore.playError();
      return { success: false, error: errorMsg };
    }
  }

  /**
   * Stop recording and process the audio
   */
  async function stopAndProcess(
    config?: Partial<PipelineConfig>
  ): Promise<{ success: boolean; result?: PipelineResult; error?: string }> {
    debug(' stopAndProcess() called, state:', state, 'isRunning:', isRunning);
    if (!isRunning || state !== 'recording') {
      debug(' Not recording, returning early');
      return { success: false, error: 'Not currently recording' };
    }

    // Build the full config
    const defaultConfig = await getDefaultConfig();
    const fullConfig: PipelineConfig = {
      ...defaultConfig,
      ...config,
    };

    debug(' Built config:', fullConfig);

    // Hide recording indicator
    debug(' Hiding recording indicator');
    try {
      await invoke('hide_recording_indicator');
    } catch (e) {
      console.warn('[Pipeline] Failed to hide recording indicator:', e);
    }

    // Play the stop tone immediately on button press
    soundStore.playRecordingStop();

    try {
      debug(' Calling pipeline_stop_and_process...');
      const result = await invoke<PipelineResult>('pipeline_stop_and_process', {
        config: fullConfig,
      });
      debug(' Received result from backend:', result);
      lastResult = result;

      if (result.success) {
        debug(' Setting state to completed');
        state = 'completed';
        debug(' State after update:', state);
        return { success: true, result };
      } else {
        debug(' Result was not successful:', result.error);
        state = 'failed';
        error = result.error ?? 'Unknown error';
        // Play error sound on failure
        soundStore.playError();
        return { success: false, error: result.error ?? 'Unknown error' };
      }
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('[Pipeline] Exception in stopAndProcess:', errorMsg);
      error = errorMsg;
      state = 'failed';
      // Play error sound on exception
      soundStore.playError();
      return { success: false, error: errorMsg };
    } finally {
      debug(' finally block - setting isRunning=false');
      isRunning = false;
      recordingStartTime = null;
    }
  }

  /**
   * Toggle recording (start if idle, stop and process if recording)
   */
  async function toggleRecording(
    config?: Partial<PipelineConfig>
  ): Promise<{ success: boolean; result?: PipelineResult; error?: string }> {
    // Cooldown: ignore toggles that arrive too soon after the last state change.
    // This prevents key bounce from immediately reversing a start or stop.
    const now = Date.now();
    const elapsed = now - lastToggleTime;
    if (elapsed < TOGGLE_COOLDOWN_MS) {
      debug(`Toggle cooldown active (${elapsed}ms < ${TOGGLE_COOLDOWN_MS}ms), ignoring`);
      return { success: false, error: 'Toggle cooldown active' };
    }

    // Prevent concurrent toggle calls
    if (isToggling) {
      debug(' Toggle already in progress, ignoring');
      return { success: false, error: 'Toggle already in progress' };
    }
    isToggling = true;
    lastToggleTime = now;

    try {
      // Check recording mode from settings
      const isPttMode = settingsStore.isPttMode;

      debug(' toggleRecording called:', {
        state,
        isPttMode,
        isRunning,
        isInitialised: settingsStore.isInitialised,
      });

      // In toggle mode, we start or stop based on current state
      if (!isPttMode) {
        if (state === 'recording') {
          debug(' Stopping recording (toggle mode)');
          return await stopAndProcess(config);
        } else if (state === 'idle' || state === 'completed' || state === 'failed') {
          debug(' Starting recording (toggle mode)');
          const startResult = await startRecording();
          return { success: startResult.success, error: startResult.error };
        } else {
          // Processing in progress
          debug(' Processing in progress, ignoring toggle');
          return { success: false, error: 'Processing in progress' };
        }
      }

      // In PTT mode, toggle is handled differently (key down/up events)
      // This function will act as a toggle for manual button clicks
      if (state === 'recording') {
        return await stopAndProcess(config);
      } else if (state === 'idle' || state === 'completed' || state === 'failed') {
        const startResult = await startRecording();
        return { success: startResult.success, error: startResult.error };
      }

      return { success: false, error: 'Invalid state for toggle' };
    } finally {
      isToggling = false;
    }
  }

  /**
   * Cancel the current pipeline execution
   */
  async function cancel(): Promise<{ success: boolean; error?: string }> {
    if (!isRunning) {
      return { success: true }; // Nothing to cancel
    }

    try {
      // Hide recording indicator
      try {
        await invoke('hide_recording_indicator');
      } catch (e) {
        console.warn('[Pipeline] Failed to hide recording indicator:', e);
      }
      await invoke('pipeline_cancel');
      state = 'idle';
      isRunning = false;
      recordingStartTime = null;
      recordingDuration = 0;
      return { success: true };
    } catch (e) {
      const errorMsg = `${e}`;
      return { success: false, error: errorMsg };
    }
  }

  /**
   * Reset the pipeline state (after viewing result)
   */
  function reset(): void {
    if (!isRunning) {
      state = 'idle';
      message = '';
      error = null;
      recordingDuration = 0;
    }
  }

  /**
   * Transcribe an imported audio file (WAV, MP3, M4A, OGG, FLAC)
   */
  async function transcribeFile(
    filePath: string
  ): Promise<{ success: boolean; result?: PipelineResult; error?: string }> {
    debug(' transcribeFile() called:', filePath);
    if (isRunning) {
      return { success: false, error: 'Pipeline is already running' };
    }

    error = null;
    lastResult = null;
    isRunning = true;

    try {
      const defaultConfig = await getDefaultConfig();
      const config: PipelineConfig = {
        ...defaultConfig,
        autoCopy: false,
        autoPaste: false,
      };

      const result = await invoke<PipelineResult>('pipeline_transcribe_file', {
        filePath,
        config,
      });

      lastResult = result;

      if (result.success) {
        state = 'completed';
        return { success: true, result };
      } else {
        state = 'failed';
        error = result.error ?? 'Unknown error';
        soundStore.playError();
        return { success: false, error: result.error ?? 'Unknown error' };
      }
    } catch (e) {
      const errorMsg = `${e}`;
      console.error('[Pipeline] Exception in transcribeFile:', errorMsg);
      error = errorMsg;
      state = 'failed';
      soundStore.playError();
      return { success: false, error: errorMsg };
    } finally {
      isRunning = false;
    }
  }

  /**
   * Force reset the pipeline state (for recovery from stuck states)
   * This should only be called when the pipeline is known to be stuck
   */
  async function forceReset(): Promise<void> {
    debug(' Force reset called');
    try {
      // Cancel any running pipeline on the backend
      await invoke('pipeline_cancel');
    } catch (e) {
      console.warn('[Pipeline] Failed to cancel backend pipeline:', e);
    }
    // Reset all frontend state
    state = 'idle';
    message = '';
    error = null;
    isRunning = false;
    lastResult = null;
    audioPath = null;
    recordingStartTime = null;
    recordingDuration = 0;
    debug(' Force reset complete');
  }

  /**
   * Start the duration timer for recording
   */
  function startDurationTimer(): void {
    const interval = setInterval(() => {
      if (recordingStartTime && state === 'recording') {
        recordingDuration = Math.floor((Date.now() - recordingStartTime) / 1000);
      } else {
        clearInterval(interval);
      }
    }, 100);
  }

  /**
   * Format duration as MM:SS
   */
  function formatDuration(seconds: number): string {
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  return {
    // Reactive state (getters)
    get state() {
      return state;
    },
    get message() {
      return message;
    },
    get isRunning() {
      return isRunning;
    },
    get isRecording() {
      return state === 'recording';
    },
    get isProcessing() {
      return (
        state === 'converting' ||
        state === 'transcribing' ||
        state === 'filtering' ||
        state === 'enhancing' ||
        state === 'outputting'
      );
    },
    get lastResult() {
      return lastResult;
    },
    get error() {
      return error;
    },
    get audioPath() {
      return audioPath;
    },
    get recordingDuration() {
      return recordingDuration;
    },
    get formattedDuration() {
      return formatDuration(recordingDuration);
    },

    // Actions
    initialise,
    cleanup,
    startRecording,
    stopAndProcess,
    toggleRecording,
    transcribeFile,
    cancel,
    reset,
    forceReset,
    getDefaultConfig,
  };
}

/** Singleton pipeline store instance */
export const pipelineStore = createPipelineStore();
