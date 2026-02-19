<script lang="ts">
  /**
   * Audio Device Selector Component
   *
   * Allows users to select an audio input device and preview the audio level.
   * Shows clear visual feedback about which device is selected and whether
   * it's available.
   */

  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { settingsStore } from '../stores/settings.svelte';

  interface AudioLevelEvent {
    rms: number;
    peak: number;
  }

  /** Minimum dB value for the meter (silence floor) */
  const MIN_DB = -60;
  /** Maximum dB value for the meter (0 dB = full scale) */
  const MAX_DB = 0;

  let isPreviewActive = $state(false);
  let currentLevel = $state(0);
  let peakLevel = $state(0);
  let unlisten: UnlistenFn | null = null;

  /** Whether the selected device is available */
  let selectedDeviceAvailable = $derived.by(() => {
    const deviceId = settingsStore.selectedDeviceId;
    if (!deviceId) {
      // System default - check if any default device exists
      return settingsStore.audioDevices.some((d) => d.is_default);
    }
    return settingsStore.audioDevices.some((d) => d.id === deviceId);
  });

  /** Get the display name for the currently selected device */
  let selectedDeviceName = $derived.by(() => {
    const deviceId = settingsStore.selectedDeviceId;
    if (!deviceId) {
      const defaultDevice = settingsStore.audioDevices.find((d) => d.is_default);
      return defaultDevice ? `${defaultDevice.name} (System Default)` : 'System Default';
    }
    const device = settingsStore.audioDevices.find((d) => d.id === deviceId);
    return device ? device.name : 'Unknown Device';
  });

  /**
   * Convert linear amplitude to a 0-1 meter position
   * Maps the dB range (-60 to 0) to 0-1
   */
  function linearToMeterPosition(linear: number): number {
    if (linear <= 0) return 0;
    const db = 20 * Math.log10(linear);
    // Map db from MIN_DB..MAX_DB to 0..1
    const normalized = (db - MIN_DB) / (MAX_DB - MIN_DB);
    return Math.max(0, Math.min(1, normalized));
  }

  /** Current level as meter position (0-1) */
  let meterLevel = $derived(linearToMeterPosition(currentLevel));

  /** Peak level as meter position (0-1) */
  let meterPeak = $derived(linearToMeterPosition(peakLevel));

  onMount(async () => {
    // Load audio devices on mount
    await settingsStore.loadAudioDevices();
  });

  onDestroy(async () => {
    // Stop preview and clean up listener when component unmounts
    await stopPreview();
  });

  /**
   * Handle device selection change
   */
  async function handleDeviceChange(event: Event): Promise<void> {
    const select = event.target as HTMLSelectElement;
    const value = select.value === '' ? null : select.value;
    await settingsStore.selectAudioDevice(value);

    // Restart preview if active to use new device
    if (isPreviewActive) {
      await restartPreview();
    }
  }

  /**
   * Start audio preview for the selected device
   */
  async function startPreview(): Promise<void> {
    if (isPreviewActive) return;

    try {
      // Set up event listener for audio levels
      unlisten = await listen<AudioLevelEvent>('audio-level', (event) => {
        currentLevel = event.payload.rms;
        peakLevel = event.payload.peak;
      });

      // Start the preview stream
      await invoke('start_audio_preview', {
        device_id: settingsStore.selectedDeviceId,
      });

      isPreviewActive = true;
    } catch (e) {
      console.error('Failed to start audio preview:', e);
      await stopPreview();
    }
  }

  /**
   * Stop audio preview
   */
  async function stopPreview(): Promise<void> {
    if (unlisten) {
      unlisten();
      unlisten = null;
    }

    try {
      await invoke('stop_audio_preview');
    } catch (e) {
      console.error('Failed to stop audio preview:', e);
    }

    isPreviewActive = false;
    currentLevel = 0;
    peakLevel = 0;
  }

  /**
   * Toggle audio preview on/off
   */
  async function togglePreview(): Promise<void> {
    if (isPreviewActive) {
      await stopPreview();
    } else {
      await startPreview();
    }
  }

  /**
   * Restart preview with potentially new device
   */
  async function restartPreview(): Promise<void> {
    await stopPreview();
    await startPreview();
  }

  /**
   * Refresh the device list
   */
  async function refreshDevices(): Promise<void> {
    await settingsStore.loadAudioDevices();
  }
</script>

<div class="audio-device-selector">
  <!-- Active Device Status Banner -->
  <div class="device-status" class:warning={!selectedDeviceAvailable}>
    <div class="status-icon">
      {#if selectedDeviceAvailable}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M19 10v2a7 7 0 0 1-14 0v-2M12 19v4M8 23h8" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M1 1l22 22M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2a7 7 0 0 1-.11 1.23M12 19v4M8 23h8" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      {/if}
    </div>
    <div class="status-content">
      <span class="status-label">{selectedDeviceAvailable ? 'Active Device' : 'Device Unavailable'}</span>
      <span class="status-device">{selectedDeviceName}</span>
    </div>
    {#if !selectedDeviceAvailable}
      <button class="status-action" onclick={refreshDevices}>
        Refresh
      </button>
    {/if}
  </div>

  <div class="selector-row">
    <label for="audio-device-select" class="label">Input Device</label>
    <div class="select-wrapper">
      {#key settingsStore.audioDevices.length}
        <select
          id="audio-device-select"
          value={settingsStore.selectedDeviceId ?? ''}
          onchange={handleDeviceChange}
          disabled={settingsStore.isLoadingDevices}
          class:device-missing={!selectedDeviceAvailable}
        >
          <option value="">System Default{settingsStore.audioDevices.find(d => d.is_default) ? ` (${settingsStore.audioDevices.find(d => d.is_default)?.name})` : ''}</option>
          {#each settingsStore.audioDevices as device (device.id)}
            <option value={device.id}>
              {device.name}{device.is_default ? ' (Default)' : ''}
            </option>
          {/each}
        </select>
      {/key}
      <button
        class="icon-btn refresh-btn"
        onclick={refreshDevices}
        disabled={settingsStore.isLoadingDevices}
        title="Refresh device list"
      >
        <svg
          class="icon"
          class:spinning={settingsStore.isLoadingDevices}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <path
            d="M21 12a9 9 0 11-3-6.7M21 3v5h-5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
    </div>
  </div>

  <div class="preview-section">
    <div class="preview-header">
      <span class="label">Audio Preview</span>
      <button class="preview-btn" class:active={isPreviewActive} onclick={togglePreview}>
        {isPreviewActive ? 'Stop' : 'Test'}
      </button>
    </div>

    <div class="level-meter">
      <div class="meter-track">
        <div
          class="meter-fill"
          class:active={isPreviewActive}
          style:width="{meterLevel * 100}%"
        ></div>
        <div
          class="meter-peak"
          class:visible={isPreviewActive && meterPeak > 0.01}
          style:left="{meterPeak * 100}%"
        ></div>
      </div>
      <div class="meter-labels">
        <span>-60 dB</span>
        <span>-30 dB</span>
        <span>0 dB</span>
      </div>
    </div>

    {#if isPreviewActive}
      <p class="preview-hint">Speak into your microphone to see audio levels</p>
    {/if}
  </div>

  {#if settingsStore.error}
    <p class="error-message">{settingsStore.error}</p>
  {/if}
</div>

<style>
  .audio-device-selector {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* Device Status Banner */
  .device-status {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-success);
    border-radius: var(--radius-md);
  }

  .device-status.warning {
    border-color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
  }

  .status-icon {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    color: var(--color-success);
  }

  .device-status.warning .status-icon {
    color: var(--color-warning);
  }

  .status-icon svg {
    width: 100%;
    height: 100%;
  }

  .status-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .status-label {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--color-success);
  }

  .device-status.warning .status-label {
    color: var(--color-warning);
  }

  .status-device {
    font-size: 13px;
    font-weight: 500;
    color: var(--color-text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .status-action {
    flex-shrink: 0;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    background: var(--color-warning);
    border: none;
    border-radius: var(--radius-sm);
    color: #000;
    cursor: pointer;
    transition: opacity 0.15s ease;
  }

  .status-action:hover {
    opacity: 0.9;
  }

  .selector-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .label {
    font-size: 13px;
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  .select-wrapper {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  select {
    flex: 1;
    padding: 8px 12px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-primary);
    font-size: 14px;
    cursor: pointer;
    appearance: none;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M2 4l4 4 4-4'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 12px center;
    padding-right: 32px;
  }

  select.device-missing {
    border-color: var(--color-warning);
  }

  select:hover {
    border-color: var(--color-bg-hover);
  }

  select:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  select:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .icon-btn {
    width: 36px;
    height: 36px;
    padding: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .icon-btn:hover {
    background: var(--color-bg-tertiary);
  }

  .icon-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .icon {
    width: 16px;
    height: 16px;
    color: var(--color-text-secondary);
  }

  .icon.spinning {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  .preview-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
  }

  .preview-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .preview-btn {
    padding: 6px 16px;
    font-size: 13px;
    font-weight: 500;
    background: var(--color-bg-tertiary);
    border: none;
    border-radius: var(--radius-md);
    color: var(--color-text-primary);
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .preview-btn:hover {
    background: var(--color-bg-hover);
  }

  .preview-btn.active {
    background: var(--color-accent);
    color: white;
  }

  .preview-btn.active:hover {
    background: var(--color-accent-hover);
  }

  .level-meter {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .meter-track {
    position: relative;
    height: 8px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .meter-fill {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    background: var(--color-bg-hover);
    border-radius: var(--radius-full);
    transition: width 0.05s linear;
  }

  .meter-fill.active {
    background: linear-gradient(
      90deg,
      var(--color-success) 0%,
      var(--color-success) 70%,
      var(--color-warning) 85%,
      var(--color-error) 100%
    );
  }

  .meter-peak {
    position: absolute;
    top: 0;
    width: 2px;
    height: 100%;
    background: var(--color-text-primary);
    opacity: 0;
    transition:
      left 0.05s linear,
      opacity 0.3s ease;
  }

  .meter-peak.visible {
    opacity: 0.8;
  }

  .meter-labels {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    color: var(--color-text-tertiary);
    padding: 0 2px;
  }

  .preview-hint {
    margin: 0;
    font-size: 12px;
    color: var(--color-text-tertiary);
    font-style: italic;
    text-align: center;
  }

  .error-message {
    margin: 0;
    padding: 8px 12px;
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-error) 30%, transparent);
    border-radius: var(--radius-md);
    color: var(--color-error);
    font-size: 13px;
  }
</style>
