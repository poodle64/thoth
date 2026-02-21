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
  /** Model currently being initialised (loading into memory / compiling) */
  let initialisingModelId = $state<string | null>(null);

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
    initialisingModelId = model.id;
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
    } finally {
      initialisingModelId = null;
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

  /** Sort models: selected first, then recommended, then downloaded, then by name */
  function sortedModels(list: ModelInfo[]): ModelInfo[] {
    return [...list].sort((a, b) => {
      // Selected always first
      if (a.selected && !b.selected) return -1;
      if (!a.selected && b.selected) return 1;
      // Recommended next
      if (a.recommended && !b.recommended) return -1;
      if (!a.recommended && b.recommended) return 1;
      // Downloaded before not-downloaded
      if (a.downloaded && !b.downloaded) return -1;
      if (!a.downloaded && b.downloaded) return 1;
      // Alphabetical within the same tier
      return a.name.localeCompare(b.name);
    });
  }

  /** Friendly backend label for the detail row */
  function backendLabel(modelType: string): string {
    switch (modelType) {
      case 'fluidaudio_coreml':
        return 'Apple Neural Engine';
      case 'nemo_transducer':
        return 'Sherpa-ONNX (CPU)';
      case 'whisper_ggml':
        return 'whisper.cpp (Metal GPU)';
      default:
        return modelType;
    }
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
    {#each sortedModels(models) as model (model.id)}
      <div
        class="model-card"
        class:downloaded={model.downloaded}
        class:selected={model.selected}
        class:unavailable={!model.backend_available}
      >
        <!-- Top row: name + primary badge -->
        <div class="model-header">
          <div class="model-title-row">
            <span class="model-name">{model.name}</span>
            {#if model.selected}
              <span class="badge badge-active">Active</span>
            {:else if model.recommended}
              <span class="badge badge-recommended">Recommended</span>
            {/if}
          </div>
          {#if model.update_available}
            <span class="badge badge-update">Update</span>
          {/if}
        </div>

        <!-- Description -->
        <p class="model-description">{model.description}</p>

        <!-- Metadata row -->
        <div class="model-meta">
          <span class="meta-item">{backendLabel(model.model_type)}</span>
          <span class="meta-sep"></span>
          <span class="meta-item">{formatLanguages(model.languages)}</span>
          <span class="meta-sep"></span>
          {#if model.downloaded && model.disk_size}
            <span class="meta-item">{formatBytes(model.disk_size)}</span>
          {:else}
            <span class="meta-item">~{model.size_mb} MB</span>
          {/if}
        </div>

        <!-- Backend warning -->
        {#if !model.backend_available}
          <p class="backend-warning">
            {#if model.model_type === 'fluidaudio_coreml'}
              Requires macOS with Apple Silicon (M1 or later).
            {:else if model.model_type === 'nemo_transducer'}
              Requires the Parakeet backend (not available in this build).
            {:else}
              Backend not available in this build.
            {/if}
          </p>
        {/if}

        <!-- Actions -->
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
          {:else if initialisingModelId === model.id}
            <div class="progress-section">
              <div class="progress-bar">
                <div class="progress-fill extracting"></div>
              </div>
              <span class="progress-text">
                {model.model_type === 'fluidaudio_coreml'
                  ? 'Compiling for Neural Engine\u2026 this may take a minute'
                  : 'Loading model\u2026'}
              </span>
            </div>
          {:else if model.downloaded && model.selected}
            <button
              class="btn-outline btn-danger"
              onclick={() => confirmDelete(model)}
              disabled={isDownloading() || initialisingModelId !== null}
            >
              Delete
            </button>
          {:else if model.downloaded}
            <button
              class="btn-primary"
              onclick={() => selectModel(model)}
              disabled={isDownloading() || initialisingModelId !== null || !model.backend_available}
            >
              {model.backend_available ? 'Use Model' : 'Unavailable'}
            </button>
            <button
              class="btn-outline btn-danger"
              onclick={() => confirmDelete(model)}
              disabled={isDownloading() || initialisingModelId !== null}
            >
              Delete
            </button>
          {:else}
            <button
              class="btn-primary"
              onclick={() => downloadModel(model)}
              disabled={isDownloading() || !model.backend_available}
            >
              {#if !model.backend_available}
                Unavailable
              {:else if model.model_type === 'fluidaudio_coreml'}
                Initialise
              {:else}
                Download
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

  /* ── Card ───────────────────────────────────────────────────────── */
  .model-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 16px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    transition: border-color var(--transition-fast);
  }

  .model-card.selected {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 4%, var(--color-bg-secondary));
  }

  .model-card.downloaded:not(.selected) {
    border-color: var(--color-border);
  }

  .model-card.unavailable {
    opacity: 0.55;
  }

  /* ── Header ────────────────────────────────────────────────────── */
  .model-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .model-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .model-name {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── Badges ────────────────────────────────────────────────────── */
  .badge {
    flex-shrink: 0;
    padding: 1px 8px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    border-radius: var(--radius-full);
    white-space: nowrap;
  }

  .badge-active {
    background: var(--color-accent);
    color: white;
  }

  .badge-recommended {
    background: color-mix(in srgb, var(--color-accent) 15%, transparent);
    color: var(--color-accent);
  }

  .badge-update {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  /* ── Description ───────────────────────────────────────────────── */
  .model-description {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.45;
  }

  /* ── Metadata row ──────────────────────────────────────────────── */
  .model-meta {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px 0;
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .meta-item {
    white-space: nowrap;
  }

  .meta-sep::after {
    content: '\00b7';
    padding: 0 6px;
    opacity: 0.4;
  }

  /* ── Backend warning ───────────────────────────────────────────── */
  .backend-warning {
    margin: 2px 0 0;
    padding: 6px 10px;
    font-size: var(--text-xs);
    color: var(--color-warning);
    background: color-mix(in srgb, var(--color-warning) 8%, transparent);
    border-radius: var(--radius-sm);
    line-height: 1.4;
  }

  /* ── Actions ───────────────────────────────────────────────────── */
  .model-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 8px;
  }

  .btn-primary,
  .btn-outline,
  .retry-btn {
    padding: 6px 14px;
    font-size: var(--text-sm);
    font-weight: 500;
    border-radius: var(--radius-md);
    transition: all var(--transition-fast);
    cursor: pointer;
  }

  .btn-primary {
    background: var(--color-accent);
    color: white;
    border: none;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .btn-primary:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .btn-outline {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .btn-outline:hover:not(:disabled) {
    border-color: var(--color-text-tertiary);
  }

  .btn-outline.btn-danger {
    border-color: color-mix(in srgb, var(--color-error) 40%, transparent);
    color: var(--color-error);
  }

  .btn-outline.btn-danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-error) 8%, transparent);
    border-color: var(--color-error);
  }

  .btn-outline:disabled,
  .btn-outline.btn-danger:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .retry-btn {
    background: var(--color-bg-tertiary);
    border: none;
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
