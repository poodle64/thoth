<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  interface ModelInfo {
    id: string;
    name: string;
    description: string;
    version: string;
    size_mb: number;
    downloaded: boolean;
    path: string;
    disk_size: number | null;
    recommended: boolean;
    languages: string[];
    update_available: boolean;
    selected: boolean;
    model_type: string;
    backend_available: boolean;
  }

  interface DownloadProgress {
    current_file: string;
    bytes_downloaded: number;
    total_bytes: number | null;
    percentage: number;
    status: string;
  }

  type DownloadState = 'Idle' | 'Downloading' | 'Extracting' | 'Completed' | { Failed: string };

  let models = $state<ModelInfo[]>([]);
  let downloadState = $state<DownloadState>('Idle');
  let progress = $state<DownloadProgress | null>(null);
  let showDeleteConfirm = $state(false);
  let modelToDelete = $state<ModelInfo | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let checking = $state(false);
  let lastChecked = $state<string | null>(null);
  let downloadingModelId = $state<string | null>(null);

  let unlistenProgress: UnlistenFn | null = null;
  let unlistenComplete: UnlistenFn | null = null;
  let unlistenError: UnlistenFn | null = null;

  onMount(() => {
    loadModels(false);
    loadLastChecked();
    setupEventListeners();

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenComplete) unlistenComplete();
      if (unlistenError) unlistenError();
    };
  });

  async function setupEventListeners() {
    unlistenProgress = await listen<DownloadProgress>('model-download-progress', (event) => {
      progress = event.payload;
    });

    unlistenComplete = await listen<string>('model-download-complete', async (event) => {
      downloadState = 'Completed';
      progress = null;
      const completedModelId = downloadingModelId;
      downloadingModelId = null;
      await loadModels(false);

      // Auto-initialise transcription engine with the downloaded model
      if (completedModelId) {
        try {
          const completedModel = models.find((m) => m.id === completedModelId);
          if (completedModel?.model_type === 'fluidaudio_coreml') {
            // FluidAudio already initialised during download; just set selected
            await invoke('set_selected_model_id', { modelId: completedModelId });
          } else {
            const modelDir = await invoke<string>('get_model_directory');
            await invoke('init_transcription', { modelPath: modelDir });
          }
        } catch (e) {
          console.warn('[ModelManager] Failed to initialise transcription after download:', e);
        }
      }
    });

    unlistenError = await listen<string>('model-download-error', (event) => {
      downloadState = { Failed: event.payload };
      error = event.payload;
      progress = null;
      downloadingModelId = null;
    });
  }

  async function loadModels(forceRefresh: boolean) {
    loading = true;
    error = null;
    try {
      // Use the new manifest-based API
      models = await invoke<ModelInfo[]>('fetch_model_manifest', { forceRefresh });
      downloadState = await invoke<DownloadState>('get_download_progress');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function loadLastChecked() {
    try {
      const time = await invoke<string | null>('get_manifest_update_time');
      if (time) {
        lastChecked = formatLastChecked(time);
      }
    } catch {
      // Ignore errors for last checked time
    }
  }

  function formatLastChecked(isoTime: string): string {
    const date = new Date(isoTime);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffHours = Math.floor(diffMs / (1000 * 60 * 60));

    if (diffHours < 1) {
      return 'Just now';
    } else if (diffHours < 24) {
      return `${diffHours} hour${diffHours === 1 ? '' : 's'} ago`;
    } else {
      const diffDays = Math.floor(diffHours / 24);
      return `${diffDays} day${diffDays === 1 ? '' : 's'} ago`;
    }
  }

  async function checkForUpdates() {
    checking = true;
    error = null;
    try {
      await loadModels(true);
      await loadLastChecked();
    } finally {
      checking = false;
    }
  }

  async function downloadModel(model: ModelInfo) {
    error = null;
    try {
      downloadState = 'Downloading';
      downloadingModelId = model.id;
      await invoke('download_model', { modelId: model.id });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      downloadState = { Failed: error };
      downloadingModelId = null;
    }
  }

  async function confirmDelete(model: ModelInfo) {
    modelToDelete = model;
    showDeleteConfirm = true;
  }

  async function deleteModel() {
    if (!modelToDelete) return;

    error = null;
    try {
      await invoke('delete_model', { modelId: modelToDelete.id });
      showDeleteConfirm = false;
      modelToDelete = null;
      await loadModels(false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function resetState() {
    error = null;
    try {
      await invoke('reset_download_state');
      downloadState = 'Idle';
      progress = null;
      downloadingModelId = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function selectModel(model: ModelInfo) {
    error = null;
    try {
      await invoke('set_selected_model_id', { modelId: model.id });
      // Re-initialise transcription with the newly selected model
      if (model.model_type === 'fluidaudio_coreml') {
        await invoke('init_fluidaudio_transcription');
      } else {
        const modelDir = await invoke<string>('get_model_directory');
        await invoke('init_transcription', { modelPath: modelDir });
      }
      await loadModels(false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function isDownloading(modelId?: string): boolean {
    if (modelId && downloadingModelId !== modelId) return false;
    return downloadState === 'Downloading' || downloadState === 'Extracting';
  }

  function isFailed(): boolean {
    return typeof downloadState === 'object' && 'Failed' in downloadState;
  }

  function getFailedMessage(): string {
    if (typeof downloadState === 'object' && 'Failed' in downloadState) {
      return downloadState.Failed;
    }
    return '';
  }

  function formatLanguages(languages: string[]): string {
    if (languages.length === 0) return 'All languages';
    if (languages.length <= 3) return languages.map((l) => l.toUpperCase()).join(', ');
    return `${languages
      .slice(0, 3)
      .map((l) => l.toUpperCase())
      .join(', ')} +${languages.length - 3} more`;
  }
</script>

<div class="model-manager">
  <div class="setting-row card">
    <div class="setting-info">
      <span class="setting-label">Model Updates</span>
      <span class="setting-description">
        {#if lastChecked}
          Last checked: {lastChecked}
        {:else}
          Check for new or updated transcription models
        {/if}
      </span>
    </div>
    <button
      class="btn-small"
      onclick={checkForUpdates}
      disabled={checking || isDownloading()}
    >
      {#if checking}
        Checking...
      {:else}
        Check for Updates
      {/if}
    </button>
  </div>

  {#if loading}
    <div class="loading">
      <div class="spinner"></div>
      <span>Loading models...</span>
    </div>
  {:else if error}
    <div class="error-row">
      <span class="error-message">{error}</span>
      <button class="btn-small" onclick={() => (error = null)}>Dismiss</button>
    </div>
  {/if}

  <div class="model-list">
    {#each models as model (model.id)}
      <div
        class="model-card"
        class:downloaded={model.downloaded}
        class:recommended={model.recommended}
        class:selected={model.selected}
      >
        <div class="model-info">
          <div class="model-header">
            <span class="model-name">{model.name}</span>
            <div class="badges">
              {#if model.model_type === 'fluidaudio_coreml'}
                <span class="status-badge backend-fluidaudio">Neural Engine</span>
              {:else if model.model_type === 'nemo_transducer'}
                <span class="status-badge backend-parakeet">Parakeet</span>
              {:else}
                <span class="status-badge backend-whisper">Whisper</span>
              {/if}
              {#if model.recommended}
                <span class="status-badge recommended">Recommended</span>
              {/if}
              {#if model.selected}
                <span class="status-badge selected">Selected</span>
              {:else if model.downloaded}
                <span class="status-badge downloaded">Downloaded</span>
              {:else}
                <span class="status-badge not-downloaded">Not Downloaded</span>
              {/if}
              {#if model.update_available}
                <span class="status-badge update">Update Available</span>
              {/if}
            </div>
          </div>
          <p class="model-description">{model.description}</p>
          {#if !model.backend_available}
            <p class="backend-warning">
              {#if model.model_type === 'fluidaudio_coreml'}
                Requires Apple Silicon and the FluidAudio backend. Build with <code>--features fluidaudio</code> to enable.
              {:else}
                This model requires the Parakeet backend which is not included in this build.
                Build with <code>--features parakeet</code> to enable.
              {/if}
            </p>
          {/if}
          <div class="model-details">
            <span class="detail">Version: {model.version}</span>
            <span class="detail">Languages: {formatLanguages(model.languages)}</span>
            {#if model.downloaded && model.disk_size}
              <span class="detail">Size on disk: {formatBytes(model.disk_size)}</span>
            {:else}
              <span class="detail">Download size: ~{model.size_mb} MB</span>
            {/if}
          </div>
        </div>

        <div class="model-actions">
          {#if isDownloading(model.id)}
            <div class="progress-section">
              {#if progress}
                <div class="progress-bar">
                  <div
                    class="progress-fill"
                    style:width="{Math.min(progress.percentage, 100)}%"
                  ></div>
                </div>
                <span class="progress-text">{progress.status}</span>
              {:else if downloadState === 'Extracting'}
                <div class="progress-bar">
                  <div class="progress-fill extracting"></div>
                </div>
                <span class="progress-text">Extracting model files...</span>
              {:else}
                <span class="progress-text">Preparing download...</span>
              {/if}
            </div>
          {:else if isFailed() && downloadingModelId === model.id}
            <div class="failed-section">
              <span class="failed-text">Download failed: {getFailedMessage()}</span>
              <button class="retry-btn" onclick={() => resetState()}>Reset</button>
            </div>
          {:else if model.downloaded && model.selected}
            <button
              class="delete-btn"
              onclick={() => confirmDelete(model)}
              disabled={isDownloading()}
            >
              Delete Model
            </button>
          {:else if model.downloaded}
            <button
              class="select-btn"
              onclick={() => selectModel(model)}
              disabled={isDownloading() || !model.backend_available}
            >
              {model.backend_available ? 'Use Model' : 'Backend Unavailable'}
            </button>
            <button
              class="delete-btn"
              onclick={() => confirmDelete(model)}
              disabled={isDownloading()}
            >
              Delete
            </button>
          {:else}
            <button
              class="download-btn primary"
              onclick={() => downloadModel(model)}
              disabled={isDownloading() || !model.backend_available}
            >
              {#if !model.backend_available}
                Backend Unavailable
              {:else if model.model_type === 'fluidaudio_coreml'}
                Initialise Model
              {:else}
                Download Model
              {/if}
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  {#if models.length === 0 && !loading}
    <div class="empty-state">
      <p>No models available. Click "Check for Updates" to refresh the list.</p>
    </div>
  {/if}

  <!-- Delete confirmation dialog -->
  {#if showDeleteConfirm && modelToDelete}
    <div
      class="modal-overlay"
      role="presentation"
      onclick={() => (showDeleteConfirm = false)}
      onkeydown={(e) => e.key === 'Escape' && (showDeleteConfirm = false)}
    >
      <div
        class="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="delete-dialog-title"
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => e.stopPropagation()}
      >
        <h4 id="delete-dialog-title">Delete Model?</h4>
        <p>
          Are you sure you want to delete "{modelToDelete.name}"? You will need to download it again
          to use offline transcription.
        </p>
        <div class="modal-actions">
          <button class="cancel-btn" onclick={() => (showDeleteConfirm = false)}> Cancel </button>
          <button class="confirm-delete-btn" onclick={deleteModel}> Delete </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .model-manager {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .loading {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 24px;
    color: var(--color-text-secondary);
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .error-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 14px;
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
    border-radius: var(--radius-md);
  }

  .error-row .error-message {
    margin: 0;
    padding: 0;
    background: none;
  }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .model-card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    transition: border-color var(--transition-fast);
  }

  .model-card.selected {
    border-color: var(--color-accent);
  }

  .model-card.downloaded:not(.selected) {
    border-color: var(--color-border);
  }

  .model-card.recommended:not(.downloaded) {
    border-color: var(--color-accent);
  }

  .model-info {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .model-header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .model-name {
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .status-badge {
    padding: 2px 8px;
    font-size: var(--text-xs);
    font-weight: 500;
    border-radius: var(--radius-full);
  }

  .status-badge.selected {
    background: color-mix(in srgb, var(--color-accent) 15%, transparent);
    color: var(--color-accent);
  }

  .status-badge.downloaded {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .status-badge.not-downloaded {
    background: color-mix(in srgb, var(--color-text-secondary) 15%, transparent);
    color: var(--color-text-secondary);
  }

  .status-badge.recommended {
    background: color-mix(in srgb, var(--color-accent) 15%, transparent);
    color: var(--color-accent);
  }

  .status-badge.update {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  .status-badge.backend-whisper {
    background: color-mix(in srgb, var(--color-text-secondary) 10%, transparent);
    color: var(--color-text-tertiary);
  }

  .status-badge.backend-parakeet {
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    color: var(--color-warning);
  }

  .status-badge.backend-fluidaudio {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .model-description {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.4;
  }

  .backend-warning {
    margin: 4px 0 0;
    padding: 6px 10px;
    font-size: var(--text-xs);
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
    border-radius: var(--radius-sm);
    line-height: 1.4;
  }

  .backend-warning code {
    font-size: var(--text-xs);
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    padding: 1px 4px;
    border-radius: 2px;
  }

  .model-details {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
  }

  .detail {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .model-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-top: 8px;
    border-top: 1px solid var(--color-border-subtle);
  }

  .download-btn,
  .select-btn,
  .delete-btn,
  .retry-btn {
    padding: 8px 16px;
    font-size: var(--text-sm);
    font-weight: 500;
    border-radius: var(--radius-md);
    transition: all var(--transition-fast);
  }

  .download-btn,
  .select-btn {
    background: var(--color-accent);
    color: white;
  }

  .download-btn:hover:not(:disabled),
  .select-btn:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .download-btn:disabled,
  .select-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .delete-btn {
    background: transparent;
    border: 1px solid var(--color-error);
    color: var(--color-error);
  }

  .delete-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
  }

  .delete-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .retry-btn {
    background: var(--color-bg-tertiary);
  }

  .progress-section {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .progress-bar {
    height: 6px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-full);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: var(--radius-full);
    transition: width 0.3s ease;
  }

  .progress-fill.extracting {
    width: 100%;
    animation: pulse 1.5s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.6;
    }
    50% {
      opacity: 1;
    }
  }

  .progress-text {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .failed-section {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .failed-text {
    flex: 1;
    font-size: var(--text-xs);
    color: var(--color-error);
  }

  .empty-state {
    padding: 32px;
    text-align: center;
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }

  /* Modal styles */
  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    width: 90%;
    max-width: 400px;
    padding: 24px;
    background: var(--color-bg-primary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
  }

  .modal h4 {
    margin: 0 0 12px 0;
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .modal p {
    margin: 0 0 20px 0;
    font-size: var(--text-base);
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
  }

  .cancel-btn {
    padding: 8px 16px;
    font-size: var(--text-sm);
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
    border-radius: var(--radius-md);
  }

  .confirm-delete-btn {
    padding: 8px 16px;
    font-size: var(--text-sm);
    background: var(--color-error);
    color: white;
    border-radius: var(--radius-md);
  }

  .confirm-delete-btn:hover {
    background: color-mix(in srgb, var(--color-error) 85%, white);
  }
</style>
