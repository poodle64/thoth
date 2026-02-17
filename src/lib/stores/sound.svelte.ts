/**
 * Sound feedback service for Thoth using Svelte 5 runes.
 *
 * Provides audio feedback for recording events (start, stop, transcription complete).
 * Sounds can be enabled/disabled via settings.
 */

import { invoke } from '@tauri-apps/api/core';

/** Sound event types matching the Rust SoundEvent enum */
export type SoundEvent = 'recording_start' | 'recording_stop' | 'transcription_complete' | 'error';

/** Create the sound service store with reactive state */
function createSoundStore() {
  let isEnabled = $state<boolean>(true);
  let isLoading = $state<boolean>(false);
  let error = $state<string | null>(null);

  /**
   * Load the current sound enabled state from the backend
   */
  async function load(): Promise<void> {
    isLoading = true;
    error = null;

    try {
      isEnabled = await invoke<boolean>('are_sounds_enabled');
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load sound settings';
      console.error('Failed to load sound settings:', e);
    } finally {
      isLoading = false;
    }
  }

  /**
   * Set whether sounds are enabled
   */
  async function setEnabled(enabled: boolean): Promise<boolean> {
    isLoading = true;
    error = null;

    try {
      await invoke('set_sounds_enabled', { enabled });
      isEnabled = enabled;
      return true;
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to update sound settings';
      console.error('Failed to update sound settings:', e);
      return false;
    } finally {
      isLoading = false;
    }
  }

  /**
   * Toggle sounds enabled state
   */
  async function toggle(): Promise<boolean> {
    return setEnabled(!isEnabled);
  }

  /**
   * Play the recording start sound
   */
  async function playRecordingStart(): Promise<void> {
    try {
      await invoke('play_recording_start_sound');
    } catch (e) {
      console.error('Failed to play recording start sound:', e);
    }
  }

  /**
   * Play the recording stop sound
   */
  async function playRecordingStop(): Promise<void> {
    try {
      await invoke('play_recording_stop_sound');
    } catch (e) {
      console.error('Failed to play recording stop sound:', e);
    }
  }

  /**
   * Play the transcription complete sound
   */
  async function playTranscriptionComplete(): Promise<void> {
    try {
      await invoke('play_transcription_complete_sound');
    } catch (e) {
      console.error('Failed to play transcription complete sound:', e);
    }
  }

  /**
   * Play the error sound
   */
  async function playError(): Promise<void> {
    try {
      await invoke('play_error_sound');
    } catch (e) {
      console.error('Failed to play error sound:', e);
    }
  }

  /**
   * Play a sound for a specific event
   */
  async function play(event: SoundEvent): Promise<void> {
    switch (event) {
      case 'recording_start':
        return playRecordingStart();
      case 'recording_stop':
        return playRecordingStop();
      case 'transcription_complete':
        return playTranscriptionComplete();
      case 'error':
        return playError();
    }
  }

  /**
   * Clear error state
   */
  function clearError(): void {
    error = null;
  }

  return {
    // State (getters for reactive access)
    get isEnabled() {
      return isEnabled;
    },
    get isLoading() {
      return isLoading;
    },
    get error() {
      return error;
    },

    // Actions
    load,
    setEnabled,
    toggle,
    play,
    playRecordingStart,
    playRecordingStop,
    playTranscriptionComplete,
    playError,
    clearError,
  };
}

/** Singleton sound store instance */
export const soundStore = createSoundStore();
