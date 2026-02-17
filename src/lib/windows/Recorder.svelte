<script lang="ts">
  // Recorder panel - compact recording interface
  // This will show as a small floating window during recording
  // Wired to the transcription pipeline for complete recording -> transcription -> output flow
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { moveWindow, Position } from '@tauri-apps/plugin-positioner';
  import AudioVisualizer from '../components/AudioVisualizer.svelte';
  import { configStore, type RecorderPosition } from '../stores/config.svelte';
  import { pipelineStore, type PipelineResult } from '../stores/pipeline.svelte';
  import { settingsStore } from '../stores/settings.svelte';

  // Local state for audio level (updated via events)
  let audioLevel = $state(0);
  let lastResultText = $state<string | null>(null);
  let showResult = $state(false);

  // Derive visualizer state from pipeline state
  let visualizerState = $derived(
    pipelineStore.isProcessing ? 'processing' : pipelineStore.isRecording ? 'recording' : 'idle'
  ) as 'idle' | 'recording' | 'processing';

  // Track event listeners for cleanup
  let unlisteners: UnlistenFn[] = [];

  // Map RecorderPosition to positioner plugin Position
  function mapPosition(pos: RecorderPosition): Position | null {
    switch (pos) {
      case 'top-left':
        return Position.TopLeft;
      case 'top-right':
        return Position.TopRight;
      case 'bottom-left':
        return Position.BottomLeft;
      case 'bottom-right':
        return Position.BottomRight;
      case 'centre':
        return Position.Center;
      case 'tray-icon':
        return Position.TrayCenter;
      case 'cursor':
        // Cursor positioning handled by backend
        return null;
      default:
        return Position.TopRight;
    }
  }

  // Position the window based on config settings
  async function positionWindow(): Promise<void> {
    try {
      // First ensure config is loaded
      if (!configStore.isInitialised) {
        await configStore.load();
      }

      const position = configStore.recorder.position;

      if (position === 'cursor') {
        // Use backend command for cursor positioning (requires platform-specific code)
        await invoke('position_recorder_window');
      } else {
        // Use frontend positioner plugin for standard positions
        const pluginPosition = mapPosition(position);
        if (pluginPosition !== null) {
          await moveWindow(pluginPosition);
        }
      }

      // Position successful
    } catch (e) {
      console.error('Failed to position recorder window:', e);
    }
  }

  // Set up audio level listening
  async function setupAudioLevelListener(): Promise<void> {
    try {
      const unlisten = await listen<{ level: number }>('audio-level', (event) => {
        audioLevel = event.payload.level;
      });
      unlisteners.push(unlisten);
    } catch (e) {
      console.error('Failed to set up audio level listener:', e);
    }
  }

  // Handle pipeline completion
  function handlePipelineComplete(result: PipelineResult): void {
    if (result.success && result.text) {
      lastResultText = result.text;
      showResult = true;

      // Auto-hide result after delay if configured
      const hideDelay = configStore.recorder.autoHideDelay;
      if (hideDelay > 0) {
        setTimeout(() => {
          showResult = false;
          lastResultText = null;
          // Optionally hide the recorder window
          invoke('hide_recorder').catch(() => {
            // Ignore errors
          });
        }, hideDelay);
      }
    }
  }

  // Watch for pipeline completion
  $effect(() => {
    const result = pipelineStore.lastResult;
    if (result && pipelineStore.state === 'completed') {
      handlePipelineComplete(result);
    }
  });

  // Position window and initialise on mount
  onMount(async () => {
    // Initialise stores
    await settingsStore.initialise();
    await pipelineStore.initialise();

    // Position window
    await positionWindow();

    // Set up audio level listener
    await setupAudioLevelListener();
  });

  // Cleanup on destroy
  onDestroy(() => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
    pipelineStore.cleanup();
  });

  async function toggleRecording(): Promise<void> {
    const result = await pipelineStore.toggleRecording();

    if (!result.success && result.error) {
      console.error('Toggle recording failed:', result.error);
      // Could show error toast here
    }
  }

  async function cancelRecording(): Promise<void> {
    await pipelineStore.cancel();
    showResult = false;
    lastResultText = null;
  }

  function dismissResult(): void {
    showResult = false;
    lastResultText = null;
    pipelineStore.reset();
  }
</script>

<div class="recorder-panel">
  {#if showResult && lastResultText}
    <!-- Result display -->
    <div class="result-container">
      <div class="result-text">{lastResultText}</div>
      <button class="dismiss-btn" onclick={dismissResult} title="Dismiss">
        <span class="check-icon"></span>
      </button>
    </div>
  {:else}
    <!-- Recording interface -->
    <div class="visualizer">
      <AudioVisualizer {visualizerState} level={audioLevel} size={80} />
    </div>

    <div class="controls">
      <span class="time">{pipelineStore.formattedDuration}</span>

      {#if pipelineStore.isRecording}
        <button class="cancel-btn" onclick={cancelRecording} title="Cancel">
          <span class="cancel-icon"></span>
        </button>
      {/if}

      <button
        class="record-btn"
        class:recording={pipelineStore.isRecording}
        class:processing={pipelineStore.isProcessing}
        onclick={toggleRecording}
        disabled={pipelineStore.isProcessing}
      >
        {#if pipelineStore.isProcessing}
          <span class="spinner-icon"></span>
        {:else if pipelineStore.isRecording}
          <span class="stop-icon"></span>
        {:else}
          <span class="mic-icon"></span>
        {/if}
      </button>
    </div>

    {#if pipelineStore.isProcessing}
      <div class="status-message">{pipelineStore.message || 'Processing...'}</div>
    {/if}

    {#if pipelineStore.error}
      <div class="error-message">{pipelineStore.error}</div>
    {/if}
  {/if}
</div>

<style>
  .recorder-panel {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    background: rgba(28, 27, 26, 0.9);
    border-radius: 16px;
    padding: 8px;
    gap: 8px;
    -webkit-app-region: drag;
  }

  .visualizer {
    width: 100%;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 12px;
    -webkit-app-region: no-drag;
  }

  .time {
    font-family: var(--font-mono);
    font-size: 14px;
    color: var(--color-text-secondary);
    min-width: 40px;
    text-align: right;
  }

  .record-btn {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    border: none;
    background: var(--color-accent);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition:
      transform 0.1s ease,
      background 0.2s ease;
  }

  .record-btn:hover {
    transform: scale(1.05);
  }

  .record-btn:active {
    transform: scale(0.95);
  }

  .record-btn.recording {
    background: var(--color-error);
  }

  .record-btn.processing {
    background: var(--color-accent);
    opacity: 0.7;
    cursor: wait;
  }

  .record-btn:disabled {
    cursor: not-allowed;
  }

  .cancel-btn {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    border: none;
    background: rgba(255, 255, 255, 0.1);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition:
      transform 0.1s ease,
      background 0.2s ease;
  }

  .cancel-btn:hover {
    background: rgba(255, 255, 255, 0.2);
    transform: scale(1.05);
  }

  .mic-icon {
    width: 20px;
    height: 20px;
    background: white;
    border-radius: 50%;
  }

  .stop-icon {
    width: 16px;
    height: 16px;
    background: white;
    border-radius: 3px;
  }

  .cancel-icon {
    width: 12px;
    height: 12px;
    position: relative;
  }

  .cancel-icon::before,
  .cancel-icon::after {
    content: '';
    position: absolute;
    width: 12px;
    height: 2px;
    background: white;
    top: 50%;
    left: 50%;
  }

  .cancel-icon::before {
    transform: translate(-50%, -50%) rotate(45deg);
  }

  .cancel-icon::after {
    transform: translate(-50%, -50%) rotate(-45deg);
  }

  .spinner-icon {
    width: 20px;
    height: 20px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: white;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .status-message {
    font-size: 11px;
    color: var(--color-text-secondary);
    margin-top: 4px;
  }

  .error-message {
    font-size: 11px;
    color: var(--color-error);
    margin-top: 4px;
    max-width: 200px;
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Result display */
  .result-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px;
    max-width: 100%;
    -webkit-app-region: no-drag;
  }

  .result-text {
    font-size: 13px;
    color: var(--color-text-primary);
    max-height: 60px;
    overflow-y: auto;
    text-align: center;
    word-break: break-word;
  }

  .dismiss-btn {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    border: none;
    background: var(--color-success);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition:
      transform 0.1s ease,
      background 0.2s ease;
  }

  .dismiss-btn:hover {
    transform: scale(1.05);
  }

  .check-icon {
    width: 14px;
    height: 10px;
    position: relative;
  }

  .check-icon::before {
    content: '';
    position: absolute;
    width: 6px;
    height: 2px;
    background: white;
    bottom: 2px;
    left: 0;
    transform: rotate(45deg);
  }

  .check-icon::after {
    content: '';
    position: absolute;
    width: 10px;
    height: 2px;
    background: white;
    bottom: 4px;
    right: 0;
    transform: rotate(-45deg);
  }
</style>
