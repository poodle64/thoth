<script lang="ts">
  /**
   * Storage pane - disk usage overview and cleanup tools.
   *
   * Shows storage breakdown by category (models, recordings, logs, database,
   * config, FluidAudio cache) with selective cleanup actions and a full reset.
   */

  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  interface StorageUsage {
    modelsBytes: number;
    recordingsBytes: number;
    logsBytes: number;
    databaseBytes: number;
    configBytes: number;
    fluidaudioBytes: number;
    totalBytes: number;
    recordingCount: number;
    logCount: number;
  }

  let usage = $state<StorageUsage | null>(null);
  let isLoading = $state(true);
  let error = $state<string | null>(null);

  /** Confirmation state for destructive actions */
  let confirmAction = $state<string | null>(null);
  let actionInProgress = $state<string | null>(null);

  async function loadUsage() {
    isLoading = true;
    error = null;
    try {
      usage = await invoke<StorageUsage>('get_storage_usage');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  /** Calculate percentage of total for the bar chart */
  function pct(bytes: number): number {
    if (!usage || usage.totalBytes === 0) return 0;
    return (bytes / usage.totalBytes) * 100;
  }

  async function handleDeleteRecordings() {
    if (confirmAction !== 'recordings') {
      confirmAction = 'recordings';
      return;
    }
    confirmAction = null;
    actionInProgress = 'recordings';
    try {
      const deleted = await invoke<number>('delete_all_recordings');
      console.log(`Deleted ${deleted} recordings`);
      await loadUsage();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      actionInProgress = null;
    }
  }

  async function handleDeleteLogs() {
    if (confirmAction !== 'logs') {
      confirmAction = 'logs';
      return;
    }
    confirmAction = null;
    actionInProgress = 'logs';
    try {
      const deleted = await invoke<number>('delete_all_logs');
      console.log(`Deleted ${deleted} log files`);
      await loadUsage();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      actionInProgress = null;
    }
  }

  async function handleDeleteFluidaudio() {
    if (confirmAction !== 'fluidaudio') {
      confirmAction = 'fluidaudio';
      return;
    }
    confirmAction = null;
    actionInProgress = 'fluidaudio';
    try {
      await invoke('delete_fluidaudio_cache');
      await loadUsage();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      actionInProgress = null;
    }
  }

  async function handleDeleteAll() {
    if (confirmAction !== 'all') {
      confirmAction = 'all';
      return;
    }
    confirmAction = null;
    actionInProgress = 'all';
    try {
      await invoke('delete_all_data');
      await loadUsage();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      actionInProgress = null;
    }
  }

  function cancelConfirm() {
    confirmAction = null;
  }

  onMount(() => {
    loadUsage();
  });
</script>

{#if isLoading}
  <div class="loading">Calculating storage usage...</div>
{:else if error}
  <div class="error-message">
    <p>{error}</p>
    <button class="btn-small" onclick={loadUsage}>Retry</button>
  </div>
{:else if usage}
  <!-- Storage overview -->
  <section class="settings-section">
    <div class="section-header">
      <h2 class="section-title">Disk Usage</h2>
      <p class="section-description">
        Thoth is using {formatBytes(usage.totalBytes)} of disk space
      </p>
    </div>
    <div class="section-content">
      <!-- Bar chart -->
      <div class="storage-bar">
        {#if usage.modelsBytes + usage.fluidaudioBytes > 0}
          <div
            class="bar-segment models"
            style="width: {Math.max(pct(usage.modelsBytes + usage.fluidaudioBytes), 1)}%"
            title="Models: {formatBytes(usage.modelsBytes + usage.fluidaudioBytes)}"
          ></div>
        {/if}
        {#if usage.recordingsBytes > 0}
          <div
            class="bar-segment recordings"
            style="width: {Math.max(pct(usage.recordingsBytes), 1)}%"
            title="Recordings: {formatBytes(usage.recordingsBytes)}"
          ></div>
        {/if}
        {#if usage.databaseBytes > 0}
          <div
            class="bar-segment database"
            style="width: {Math.max(pct(usage.databaseBytes), 1)}%"
            title="Database: {formatBytes(usage.databaseBytes)}"
          ></div>
        {/if}
        {#if usage.logsBytes > 0}
          <div
            class="bar-segment logs"
            style="width: {Math.max(pct(usage.logsBytes), 1)}%"
            title="Logs: {formatBytes(usage.logsBytes)}"
          ></div>
        {/if}
        {#if usage.configBytes > 0}
          <div
            class="bar-segment config"
            style="width: {Math.max(pct(usage.configBytes), 1)}%"
            title="Config: {formatBytes(usage.configBytes)}"
          ></div>
        {/if}
      </div>

      <!-- Legend -->
      <div class="storage-legend">
        <div class="legend-item">
          <span class="legend-dot models"></span>
          <span class="legend-label">Models</span>
          <span class="legend-value">{formatBytes(usage.modelsBytes + usage.fluidaudioBytes)}</span>
        </div>
        <div class="legend-item">
          <span class="legend-dot recordings"></span>
          <span class="legend-label">Recordings</span>
          <span class="legend-value">
            {formatBytes(usage.recordingsBytes)}
            {#if usage.recordingCount > 0}
              <span class="legend-count">({usage.recordingCount} files)</span>
            {/if}
          </span>
        </div>
        <div class="legend-item">
          <span class="legend-dot database"></span>
          <span class="legend-label">Database</span>
          <span class="legend-value">{formatBytes(usage.databaseBytes)}</span>
        </div>
        <div class="legend-item">
          <span class="legend-dot logs"></span>
          <span class="legend-label">Logs</span>
          <span class="legend-value">
            {formatBytes(usage.logsBytes)}
            {#if usage.logCount > 0}
              <span class="legend-count">({usage.logCount} files)</span>
            {/if}
          </span>
        </div>
        <div class="legend-item">
          <span class="legend-dot config"></span>
          <span class="legend-label">Config</span>
          <span class="legend-value">{formatBytes(usage.configBytes)}</span>
        </div>
      </div>
    </div>
  </section>

  <!-- Selective cleanup -->
  <section class="settings-section">
    <div class="section-header">
      <h2 class="section-title">Cleanup</h2>
      <p class="section-description">Selectively remove data to free up disk space</p>
    </div>
    <div class="section-content">
      <div class="cleanup-list">
        <!-- Recordings -->
        <div class="cleanup-row">
          <div class="cleanup-info">
            <span class="cleanup-label">Recordings</span>
            <span class="cleanup-description">
              {usage.recordingCount} audio files ({formatBytes(usage.recordingsBytes)})
            </span>
          </div>
          {#if confirmAction === 'recordings'}
            <div class="confirm-group">
              <span class="confirm-text">Delete all recordings?</span>
              <button class="btn-danger-sm" onclick={handleDeleteRecordings}>Delete</button>
              <button class="btn-cancel-sm" onclick={cancelConfirm}>Cancel</button>
            </div>
          {:else}
            <button
              class="btn-cleanup"
              disabled={usage.recordingsBytes === 0 || actionInProgress !== null}
              onclick={handleDeleteRecordings}
            >
              {actionInProgress === 'recordings' ? 'Deleting...' : 'Delete'}
            </button>
          {/if}
        </div>

        <!-- Logs -->
        <div class="cleanup-row">
          <div class="cleanup-info">
            <span class="cleanup-label">Logs</span>
            <span class="cleanup-description">
              {usage.logCount} log files ({formatBytes(usage.logsBytes)})
            </span>
          </div>
          {#if confirmAction === 'logs'}
            <div class="confirm-group">
              <span class="confirm-text">Delete all logs?</span>
              <button class="btn-danger-sm" onclick={handleDeleteLogs}>Delete</button>
              <button class="btn-cancel-sm" onclick={cancelConfirm}>Cancel</button>
            </div>
          {:else}
            <button
              class="btn-cleanup"
              disabled={usage.logsBytes === 0 || actionInProgress !== null}
              onclick={handleDeleteLogs}
            >
              {actionInProgress === 'logs' ? 'Deleting...' : 'Delete'}
            </button>
          {/if}
        </div>

        <!-- FluidAudio cache -->
        {#if usage.fluidaudioBytes > 0}
          <div class="cleanup-row">
            <div class="cleanup-info">
              <span class="cleanup-label">Neural Engine cache</span>
              <span class="cleanup-description">
                CoreML compiled models ({formatBytes(usage.fluidaudioBytes)})
              </span>
            </div>
            {#if confirmAction === 'fluidaudio'}
              <div class="confirm-group">
                <span class="confirm-text">Delete cache? Will recompile on next use.</span>
                <button class="btn-danger-sm" onclick={handleDeleteFluidaudio}>Delete</button>
                <button class="btn-cancel-sm" onclick={cancelConfirm}>Cancel</button>
              </div>
            {:else}
              <button
                class="btn-cleanup"
                disabled={actionInProgress !== null}
                onclick={handleDeleteFluidaudio}
              >
                {actionInProgress === 'fluidaudio' ? 'Deleting...' : 'Delete'}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </section>

  <!-- Full reset -->
  <section class="settings-section">
    <div class="section-header">
      <h2 class="section-title">Reset</h2>
      <p class="section-description">
        Remove all Thoth data from this machine. This cannot be undone.
      </p>
    </div>
    <div class="section-content">
      {#if confirmAction === 'all'}
        <div class="reset-confirm">
          <p class="reset-warning">
            This will permanently delete all Thoth data including models, recordings,
            transcription history, settings, and the Neural Engine cache.
          </p>
          <div class="reset-actions">
            <button class="btn-danger" onclick={handleDeleteAll}>
              Delete Everything
            </button>
            <button class="btn-cancel" onclick={cancelConfirm}>Cancel</button>
          </div>
        </div>
      {:else}
        <button
          class="btn-danger-outline"
          disabled={actionInProgress !== null}
          onclick={handleDeleteAll}
        >
          {actionInProgress === 'all' ? 'Deleting...' : 'Delete All Thoth Data'}
        </button>
      {/if}
    </div>
  </section>
{/if}

<style>
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--spacing-xl);
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }

  .error-message {
    padding: var(--spacing-lg);
    color: var(--color-error);
    font-size: var(--text-sm);
    text-align: center;
  }

  .error-message p {
    margin: 0 0 12px;
  }

  /* Storage bar */
  .storage-bar {
    display: flex;
    height: 12px;
    border-radius: var(--radius-full);
    overflow: hidden;
    background: var(--color-bg-tertiary);
    gap: 1px;
  }

  .bar-segment {
    min-width: 3px;
    transition: width var(--transition-normal);
  }

  .bar-segment.models {
    background: var(--color-accent);
  }

  .bar-segment.fluidaudio {
    background: #8b5cf6;
  }

  .bar-segment.recordings {
    background: #10b981;
  }

  .bar-segment.database {
    background: #f59e0b;
  }

  .bar-segment.logs {
    background: #6b7280;
  }

  .bar-segment.config {
    background: #94a3b8;
  }

  /* Legend */
  .storage-legend {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 16px;
  }

  .legend-item {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--text-sm);
  }

  .legend-dot {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    flex-shrink: 0;
  }

  .legend-dot.models {
    background: var(--color-accent);
  }

  .legend-dot.fluidaudio {
    background: #8b5cf6;
  }

  .legend-dot.recordings {
    background: #10b981;
  }

  .legend-dot.database {
    background: #f59e0b;
  }

  .legend-dot.logs {
    background: #6b7280;
  }

  .legend-dot.config {
    background: #94a3b8;
  }

  .legend-label {
    color: var(--color-text-secondary);
    flex: 1;
  }

  .legend-value {
    color: var(--color-text-primary);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .legend-count {
    color: var(--color-text-tertiary);
    font-weight: 400;
    font-size: var(--text-xs);
    margin-left: 4px;
  }

  /* Cleanup list */
  .cleanup-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 0;
  }

  .cleanup-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .cleanup-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .cleanup-label {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
  }

  .cleanup-description {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .btn-cleanup {
    padding: 5px 14px;
    font-size: var(--text-xs);
    font-weight: 500;
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .btn-cleanup:hover:not(:disabled) {
    background: var(--color-bg-secondary);
    color: var(--color-error);
    border-color: var(--color-error);
  }

  .btn-cleanup:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* Confirm inline */
  .confirm-group {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .confirm-text {
    font-size: var(--text-xs);
    color: var(--color-warning);
    white-space: nowrap;
  }

  .btn-danger-sm {
    padding: 4px 10px;
    font-size: var(--text-xs);
    font-weight: 500;
    background: var(--color-error);
    color: white;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .btn-danger-sm:hover {
    filter: brightness(1.1);
  }

  .btn-cancel-sm {
    padding: 4px 10px;
    font-size: var(--text-xs);
    font-weight: 500;
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  /* Full reset */
  .reset-confirm {
    padding: 16px;
    background: color-mix(in srgb, var(--color-error) 5%, var(--color-bg-secondary));
    border: 1px solid color-mix(in srgb, var(--color-error) 30%, var(--color-border));
    border-radius: var(--radius-md);
  }

  .reset-warning {
    margin: 0 0 14px;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .reset-actions {
    display: flex;
    gap: 10px;
  }

  .btn-danger {
    padding: 8px 18px;
    font-size: var(--text-sm);
    font-weight: 500;
    background: var(--color-error);
    color: white;
    border: none;
    border-radius: var(--radius-md);
    cursor: pointer;
    transition: filter var(--transition-fast);
  }

  .btn-danger:hover {
    filter: brightness(1.1);
  }

  .btn-cancel {
    padding: 8px 18px;
    font-size: var(--text-sm);
    font-weight: 500;
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
  }

  .btn-danger-outline {
    padding: 8px 18px;
    font-size: var(--text-sm);
    font-weight: 500;
    background: transparent;
    color: var(--color-error);
    border: 1px solid var(--color-error);
    border-radius: var(--radius-md);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
  }

  .btn-danger-outline:hover:not(:disabled) {
    background: var(--color-error);
    color: white;
  }

  .btn-danger-outline:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
