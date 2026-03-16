<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
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

  interface CustomModel {
    id: string;
    name: string;
    path: string;
    backend: string;
    description: string | null;
  }

  interface DownloadProgress {
    current_file: string;
    bytes_downloaded: number;
    total_bytes: number | null;
    percentage: number;
    status: string;
  }

  type DownloadState = 'Idle' | 'Downloading' | 'Extracting' | 'Completed' | { Failed: string };

  // ── State ──────────────────────────────────────────────────────────
  let models = $state<ModelInfo[]>([]);
  let customModels = $state<CustomModel[]>([]);
  let downloadState = $state<DownloadState>('Idle');
  let progress = $state<DownloadProgress | null>(null);
  let showDeleteConfirm = $state(false);
  let modelToDelete = $state<ModelInfo | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let checking = $state(false);
  let lastChecked = $state<string | null>(null);
  let downloadingModelId = $state<string | null>(null);
  let initialisingModelId = $state<string | null>(null);

  // Lightning Whisper MLX state
  let lightningAvailable = $state(false);
  let lightningInstalling = $state(false);
  let lightningInstallDone = $state(false);
  let lightningModel = $state('large-v3');
  let lightningQuant = $state<string>('None');
  const lightningModels = [
    'tiny', 'small', 'distil-small.en', 'base', 'medium', 'distil-medium.en',
    'large', 'large-v2', 'distil-large-v2', 'large-v3', 'distil-large-v3',
  ];
  const lightningQuants = ['None', '4bit', '8bit'];

  // Collapsed state for each category section
  let collapsedSections = $state<Record<string, boolean>>({
    parakeet: false,
    whisperkit: false,
    lightning: false,
    custom: false,
  });

  function toggleSection(key: string) {
    collapsedSections[key] = !collapsedSections[key];
  }

  // Custom model form state
  let showCustomForm = $state(false);
  let customName = $state('');
  let customPath = $state('');
  let customBackend = $state('whisper_ggml');
  let customDescription = $state('');

  let unlistenProgress: UnlistenFn | null = null;
  let unlistenComplete: UnlistenFn | null = null;
  let unlistenError: UnlistenFn | null = null;

  onMount(() => {
    loadModels(false);
    loadLastChecked();
    checkLightningAvailable();
    loadCustomModels();
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

      if (completedModelId) {
        try {
          const completedModel = models.find((m) => m.id === completedModelId);
          if (completedModel?.model_type === 'fluidaudio_coreml') {
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
      if (time) lastChecked = formatLastChecked(time);
    } catch { /* ignore */ }
  }

  async function checkLightningAvailable() {
    try {
      lightningAvailable = await invoke<boolean>('is_lightning_whisper_available');
    } catch {
      lightningAvailable = false;
    }
  }

  async function loadCustomModels() {
    try {
      customModels = await invoke<CustomModel[]>('list_custom_models');
    } catch { /* ignore */ }
  }

  function formatLastChecked(isoTime: string): string {
    const date = new Date(isoTime);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffHours = Math.floor(diffMs / (1000 * 60 * 60));
    if (diffHours < 1) return 'Just now';
    if (diffHours < 24) return `${diffHours} hour${diffHours === 1 ? '' : 's'} ago`;
    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays} day${diffDays === 1 ? '' : 's'} ago`;
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
      if (model.model_type === 'fluidaudio_coreml') {
        await invoke('init_fluidaudio_transcription');
      } else if (model.model_type === 'custom_whisper_ggml' || model.model_type === 'custom_parakeet') {
        await invoke('init_transcription', { modelPath: model.path });
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

  async function useLightningWhisper() {
    error = null;
    initialisingModelId = '__lightning_whisper__';
    try {
      const quant = lightningQuant === 'None' ? null : lightningQuant;
      await invoke('init_lightning_whisper_transcription', {
        model: lightningModel,
        quant,
      });
      await invoke('set_selected_model_id', { modelId: `lightning-whisper-${lightningModel}` });
      await loadModels(false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      initialisingModelId = null;
    }
  }

  async function browseCustomPath() {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        title: 'Select Model File',
      });
      if (selected) {
        customPath = typeof selected === 'string' ? selected : selected.path;
      }
    } catch { /* cancelled */ }
  }

  async function addCustomModel() {
    if (!customName.trim() || !customPath.trim()) return;
    error = null;
    try {
      await invoke('add_custom_model', {
        name: customName.trim(),
        path: customPath.trim(),
        backend: customBackend,
        description: customDescription.trim() || null,
      });
      customName = '';
      customPath = '';
      customDescription = '';
      showCustomForm = false;
      await loadCustomModels();
      await loadModels(false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function removeCustomModel(id: string) {
    error = null;
    try {
      await invoke('remove_custom_model', { id });
      await loadCustomModels();
      await loadModels(false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function installLightningWhisper() {
    lightningInstalling = true;
    error = null;
    try {
      await invoke('install_lightning_whisper_mlx');
      lightningInstallDone = true;
      // Poll availability every 3s up to ~2 minutes so the button auto-updates
      const poll = setInterval(async () => {
        const avail = await invoke<boolean>('is_lightning_whisper_available').catch(() => false);
        if (avail) {
          lightningAvailable = true;
          lightningInstalling = false;
          lightningInstallDone = false;
          clearInterval(poll);
        }
      }, 3000);
      setTimeout(() => clearInterval(poll), 120_000);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      lightningInstalling = false;
      lightningInstallDone = false;
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
    if (typeof downloadState === 'object' && 'Failed' in downloadState) return downloadState.Failed;
    return '';
  }

  function formatLanguages(languages: string[]): string {
    if (languages.length === 0) return 'All languages';
    if (languages.length <= 3) return languages.map((l) => l.toUpperCase()).join(', ');
    return `${languages.slice(0, 3).map((l) => l.toUpperCase()).join(', ')} +${languages.length - 3} more`;
  }

  // ── Categorisation ─────────────────────────────────────────────────

  type Category = 'parakeet' | 'whisperkit' | 'lightning' | 'custom';

  function getCategory(model: ModelInfo): Category {
    if (model.model_type === 'custom_whisper_ggml' || model.model_type === 'custom_parakeet') return 'custom';
    if (model.model_type === 'nemo_transducer' || model.model_type === 'fluidaudio_coreml') return 'parakeet';
    if (model.model_type === 'whisper_ggml') return 'whisperkit';
    return 'whisperkit';
  }

  interface CategoryDef {
    key: Category;
    label: string;
    description: string;
    iconBg: string;
    iconText: string;
    iconEmoji?: string;
  }

  const categories: CategoryDef[] = [
    { key: 'parakeet', label: 'Parakeet', description: 'NVIDIA Parakeet + Apple Neural Engine', iconBg: '#76b900', iconText: 'NV' },
    { key: 'whisperkit', label: 'WhisperKit', description: 'OpenAI Whisper via whisper.cpp (Metal GPU)', iconBg: '#4a90d9', iconText: 'W' },
    { key: 'lightning', label: 'Lightning Whisper MLX', description: 'Fast Whisper on Apple Silicon via MLX', iconBg: '#9b59b6', iconText: '', iconEmoji: '\u26A1' },
    { key: 'custom', label: 'Custom Models', description: 'User-supplied local models', iconBg: '#6b7280', iconText: '', iconEmoji: '\u2699' },
  ];

  function modelsForCategory(cat: Category): ModelInfo[] {
    return models.filter((m) => getCategory(m) === cat);
  }

  /** Sort order within categories */
  function sortCategoryModels(list: ModelInfo[]): ModelInfo[] {
    return [...list].sort((a, b) => {
      if (a.selected && !b.selected) return -1;
      if (!a.selected && b.selected) return 1;
      if (a.recommended && !b.recommended) return -1;
      if (!a.recommended && b.recommended) return 1;
      if (a.downloaded && !b.downloaded) return -1;
      if (!a.downloaded && b.downloaded) return 1;
      return a.name.localeCompare(b.name);
    });
  }

  /** Get downloaded models across all categories for the "Downloaded" summary */
  function downloadedModels(): ModelInfo[] {
    return models
      .filter((m) => m.downloaded || m.selected)
      .sort((a, b) => {
        if (a.selected && !b.selected) return -1;
        if (!a.selected && b.selected) return 1;
        return a.name.localeCompare(b.name);
      });
  }

  function backendLabel(modelType: string): string {
    switch (modelType) {
      case 'fluidaudio_coreml': return 'Apple Neural Engine';
      case 'nemo_transducer': return 'Sherpa-ONNX (CPU)';
      case 'whisper_ggml': return 'whisper.cpp (Metal GPU)';
      case 'custom_whisper_ggml': return 'Custom (whisper.cpp)';
      case 'custom_parakeet': return 'Custom (Parakeet)';
      case 'lightning_whisper_mlx': return 'Lightning Whisper MLX';
      default: return modelType;
    }
  }

  function categoryIcon(cat: CategoryDef): { bg: string; content: string; isEmoji: boolean } {
    if (cat.iconEmoji) return { bg: cat.iconBg, content: cat.iconEmoji, isEmoji: true };
    return { bg: cat.iconBg, content: cat.iconText, isEmoji: false };
  }
</script>

<div class="model-manager">
  <!-- Header: Model Updates -->
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
    <button class="btn-small" onclick={checkForUpdates} disabled={checking || isDownloading()}>
      {checking ? 'Checking...' : 'Check for Updates'}
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

  <!-- Downloaded summary section -->
  {#if downloadedModels().length > 0}
    <div class="section-header">
      <span class="section-title">Downloaded</span>
    </div>
    <div class="downloaded-strip">
      {#each downloadedModels() as model (model.id)}
        {@const cat = categories.find((c) => c.key === getCategory(model))}
        <!-- svelte-ignore a11y_interactive_supports_focus -->
        <div
          class="downloaded-chip"
          class:active={model.selected}
          role="button"
          aria-pressed={model.selected}
          onclick={() => !model.selected && selectModel(model)}
          style:cursor={model.selected ? 'default' : 'pointer'}
          title={model.selected ? 'Active model' : `Switch to ${model.name}`}
        >
          {#if cat}
            <span class="cat-icon cat-icon-sm" style:background={cat.iconBg}>
              {#if cat.iconEmoji}{cat.iconEmoji}{:else}{cat.iconText}{/if}
            </span>
          {/if}
          <span class="chip-name">{model.name}</span>
          {#if model.selected}
            <span class="badge badge-active">Active</span>
          {:else if initialisingModelId === model.id}
            <span class="badge badge-loading">Loading…</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Category sections -->
  {#each categories as cat (cat.key)}
    {@const catModels = sortCategoryModels(modelsForCategory(cat.key))}
    {@const icon = categoryIcon(cat)}

    {#if cat.key !== 'lightning' && cat.key !== 'custom' && catModels.length > 0}
      <div class="category-section">
        <!-- svelte-ignore a11y_interactive_supports_focus -->
        <div class="category-header collapsible-header" role="button" onclick={() => toggleSection(cat.key)}>
          <span class="cat-icon" style:background={icon.bg}>
            {#if icon.isEmoji}{icon.content}{:else}{icon.content}{/if}
          </span>
          <div class="category-info">
            <span class="category-name">{cat.label}</span>
            <span class="category-desc">{cat.description}</span>
          </div>
          <span class="collapse-chevron" class:collapsed={collapsedSections[cat.key]}>›</span>
        </div>

        {#if !collapsedSections[cat.key]}
        <div class="model-list">
          {#each catModels as model (model.id)}
            <div
              class="model-card"
              class:downloaded={model.downloaded}
              class:selected={model.selected}
              class:unavailable={!model.backend_available}
            >
              <div class="model-row">
                <span class="cat-icon cat-icon-sm" style:background={icon.bg}>
                  {#if icon.isEmoji}{icon.content}{:else}{icon.content}{/if}
                </span>
                <div class="model-info">
                  <div class="model-title-row">
                    <span class="model-name">{model.name}</span>
                    {#if model.selected}
                      <span class="badge badge-active">Active</span>
                    {:else if model.recommended}
                      <span class="badge badge-recommended">Recommended</span>
                    {/if}
                    {#if model.update_available}
                      <span class="badge badge-update">Update</span>
                    {/if}
                  </div>
                  <p class="model-description">{model.description}</p>
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
                </div>
              </div>

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

              <div class="model-actions">
                {#if isDownloading(model.id)}
                  <div class="progress-section">
                    {#if progress}
                      <div class="progress-bar"><div class="progress-fill" style:width="{Math.min(progress.percentage, 100)}%"></div></div>
                      <span class="progress-text">{progress.status}</span>
                    {:else if downloadState === 'Extracting'}
                      <div class="progress-bar"><div class="progress-fill extracting"></div></div>
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
                    <div class="progress-bar"><div class="progress-fill extracting"></div></div>
                    <span class="progress-text">
                      {model.model_type === 'fluidaudio_coreml'
                        ? 'Compiling for Neural Engine\u2026 this may take a minute'
                        : 'Loading model\u2026'}
                    </span>
                  </div>
                {:else if model.downloaded && model.selected}
                  <button class="btn-outline btn-danger" onclick={() => confirmDelete(model)} disabled={isDownloading() || initialisingModelId !== null}>Delete</button>
                {:else if model.downloaded}
                  <button class="btn-primary" onclick={() => selectModel(model)} disabled={isDownloading() || initialisingModelId !== null || !model.backend_available}>
                    {model.backend_available ? 'Use Model' : 'Unavailable'}
                  </button>
                  <button class="btn-outline btn-danger" onclick={() => confirmDelete(model)} disabled={isDownloading() || initialisingModelId !== null}>Delete</button>
                {:else}
                  <button class="btn-primary" onclick={() => downloadModel(model)} disabled={isDownloading() || !model.backend_available}>
                    {#if !model.backend_available}Unavailable{:else if model.model_type === 'fluidaudio_coreml'}Initialise{:else}Download{/if}
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        </div>
        {/if}
      </div>
    {/if}
  {/each}

  <!-- Lightning Whisper MLX category -->
  <div class="category-section">
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="category-header collapsible-header" role="button" onclick={() => toggleSection('lightning')}>
      <span class="cat-icon" style:background="#9b59b6">{'\u26A1'}</span>
      <div class="category-info">
        <span class="category-name">Lightning Whisper MLX</span>
        <span class="category-desc">Fast Whisper on Apple Silicon via MLX</span>
      </div>
      <span class="collapse-chevron" class:collapsed={collapsedSections['lightning']}>›</span>
    </div>

    {#if !collapsedSections['lightning']}
    <div class="model-card lightning-card">
      <div class="model-row">
        <span class="cat-icon cat-icon-sm" style:background="#9b59b6">{'\u26A1'}</span>
        <div class="model-info" style="flex:1">
          <div class="model-title-row">
            <span class="model-name">Lightning Whisper MLX</span>
          </div>
          <p class="model-description">Python-based Whisper transcription optimised for Apple Silicon via MLX framework.</p>

          {#if !lightningAvailable}
            <div class="backend-warning install-warning">
              <span>Not installed.</span>
              <button
                class="btn-install"
                onclick={installLightningWhisper}
                disabled={lightningInstalling}
              >
                {#if lightningInstalling}
                  Installing… (check Terminal)
                {:else if lightningInstallDone}
                  Waiting for install…
                {:else}
                  Install
                {/if}
              </button>
            </div>
          {/if}

          <div class="lightning-controls">
            <div class="control-group">
              <label class="control-label" for="lw-model">Model</label>
              <select id="lw-model" class="control-select" bind:value={lightningModel}>
                {#each lightningModels as m}
                  <option value={m}>{m}</option>
                {/each}
              </select>
            </div>
            <div class="control-group">
              <label class="control-label" for="lw-quant">Quantization</label>
              <div class="segmented-control">
                {#each lightningQuants as q}
                  <button
                    class="seg-btn"
                    class:seg-active={lightningQuant === q}
                    onclick={() => (lightningQuant = q)}
                  >{q}</button>
                {/each}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="model-actions">
        {#if initialisingModelId === '__lightning_whisper__'}
          <div class="progress-section">
            <div class="progress-bar"><div class="progress-fill extracting"></div></div>
            <span class="progress-text">Loading Lightning Whisper MLX model...</span>
          </div>
        {:else}
          <button
            class="btn-primary"
            onclick={useLightningWhisper}
            disabled={!lightningAvailable || isDownloading() || initialisingModelId !== null}
          >Use</button>
        {/if}
      </div>
    </div>
    {/if}
  </div>

  <!-- Custom Models category -->
  <div class="category-section">
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="category-header collapsible-header" role="button" onclick={() => toggleSection('custom')}>
      <span class="cat-icon" style:background="#6b7280">{'\u2699'}</span>
      <div class="category-info">
        <span class="category-name">Custom Models</span>
        <span class="category-desc">User-supplied local models</span>
      </div>
      <span class="collapse-chevron" class:collapsed={collapsedSections['custom']}>›</span>
    </div>

    {#if !collapsedSections['custom']}
    {#each modelsForCategory('custom') as model (model.id)}
      <div class="model-card" class:selected={model.selected}>
        <div class="model-row">
          <span class="cat-icon cat-icon-sm" style:background="#6b7280">{'\u2699'}</span>
          <div class="model-info">
            <div class="model-title-row">
              <span class="model-name">{model.name}</span>
              {#if model.selected}
                <span class="badge badge-active">Active</span>
              {/if}
            </div>
            {#if model.description}
              <p class="model-description">{model.description}</p>
            {/if}
            <div class="model-meta">
              <span class="meta-item">{backendLabel(model.model_type)}</span>
              <span class="meta-sep"></span>
              <span class="meta-item path-text">{model.path}</span>
            </div>
          </div>
        </div>
        <div class="model-actions">
          {#if !model.selected}
            <button class="btn-primary" onclick={() => selectModel(model)} disabled={isDownloading() || initialisingModelId !== null || !model.backend_available}>
              Use Model
            </button>
          {/if}
          <button class="btn-outline btn-danger" onclick={() => removeCustomModel(model.id)} disabled={isDownloading() || initialisingModelId !== null}>
            Remove
          </button>
        </div>
      </div>
    {/each}

    {#if showCustomForm}
      <div class="custom-form card">
        <div class="form-row">
          <label class="control-label" for="cm-name">Name</label>
          <input id="cm-name" type="text" class="control-input" bind:value={customName} placeholder="My Custom Model" />
        </div>
        <div class="form-row">
          <label class="control-label" for="cm-path">File Path</label>
          <div class="path-row">
            <input id="cm-path" type="text" class="control-input" bind:value={customPath} placeholder="/path/to/model" />
            <button class="btn-small" onclick={browseCustomPath}>Browse</button>
          </div>
        </div>
        <div class="form-row">
          <label class="control-label" for="cm-backend">Backend</label>
          <select id="cm-backend" class="control-select" bind:value={customBackend}>
            <option value="whisper_ggml">whisper.cpp (GGML)</option>
            <option value="parakeet">Parakeet (ONNX)</option>
          </select>
        </div>
        <div class="form-row">
          <label class="control-label" for="cm-desc">Description (optional)</label>
          <input id="cm-desc" type="text" class="control-input" bind:value={customDescription} placeholder="Optional description" />
        </div>
        <div class="form-actions">
          <button class="btn-small" onclick={() => (showCustomForm = false)}>Cancel</button>
          <button class="btn-primary" onclick={addCustomModel} disabled={!customName.trim() || !customPath.trim()}>Add Model</button>
        </div>
      </div>
    {:else}
      <button class="btn-outline add-custom-btn" onclick={() => (showCustomForm = true)}>
        + Add Custom Model
      </button>
    {/if}
    {/if}
  </div>

  {#if models.length === 0 && !loading}
    <div class="empty-state">
      <p>No models available. Click "Check for Updates" to refresh the list.</p>
    </div>
  {/if}

  <!-- Delete confirmation dialog -->
  {#if showDeleteConfirm && modelToDelete}
    <div class="modal-overlay" role="presentation" onclick={() => (showDeleteConfirm = false)} onkeydown={(e) => e.key === 'Escape' && (showDeleteConfirm = false)}>
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="delete-dialog-title" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
        <h4 id="delete-dialog-title">Delete Model?</h4>
        <p>Are you sure you want to delete "{modelToDelete.name}"? You will need to download it again to use offline transcription.</p>
        <div class="modal-actions">
          <button class="cancel-btn" onclick={() => (showDeleteConfirm = false)}>Cancel</button>
          <button class="confirm-delete-btn" onclick={deleteModel}>Delete</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .model-manager {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  /* ── Loading / Error ──────────────────────────────────────────── */
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

  @keyframes spin { to { transform: rotate(360deg); } }

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

  /* ── Section headers ──────────────────────────────────────────── */
  .section-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .section-title {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-tertiary);
  }

  /* ── Downloaded strip ─────────────────────────────────────────── */
  .downloaded-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .downloaded-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 4px 4px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-full);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .downloaded-chip.active {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 8%, var(--color-bg-secondary));
  }

  .chip-name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }

  /* ── Category icons ───────────────────────────────────────────── */
  .cat-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 700;
    color: white;
    flex-shrink: 0;
    line-height: 1;
  }

  .cat-icon-sm {
    width: 28px;
    height: 28px;
    font-size: 12px;
    border-radius: 6px;
  }

  /* ── Category sections ────────────────────────────────────────── */
  .category-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .category-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 0;
  }

  .collapsible-header {
    cursor: pointer;
    user-select: none;
    border-radius: var(--radius-sm);
    padding: 6px 4px;
    margin: -2px -4px;
    transition: background var(--transition-fast);
  }

  .collapsible-header:hover {
    background: var(--color-bg-secondary);
  }

  .collapse-chevron {
    margin-left: auto;
    font-size: 18px;
    color: var(--color-text-tertiary);
    line-height: 1;
    transform: rotate(90deg);
    transition: transform 0.2s ease;
    flex-shrink: 0;
  }

  .collapse-chevron.collapsed {
    transform: rotate(0deg);
  }

  .install-warning {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .btn-install {
    padding: 4px 12px;
    font-size: var(--text-xs);
    font-weight: 600;
    border-radius: var(--radius-md);
    background: var(--color-accent);
    color: white;
    border: none;
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  .btn-install:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }

  .btn-install:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .badge-loading {
    background: color-mix(in srgb, var(--color-text-tertiary) 15%, transparent);
    color: var(--color-text-tertiary);
  }

  .category-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .category-name {
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .category-desc {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  /* ── Model list ───────────────────────────────────────────────── */
  .model-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .model-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
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

  .model-row {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .model-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
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

  /* ── Badges ───────────────────────────────────────────────────── */
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

  /* ── Description / Meta ───────────────────────────────────────── */
  .model-description {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.45;
  }

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

  .path-text {
    font-family: monospace;
    font-size: 11px;
    opacity: 0.7;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 250px;
  }

  .meta-sep::after {
    content: '\00b7';
    padding: 0 6px;
    opacity: 0.4;
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
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 11px;
  }

  /* ── Actions ──────────────────────────────────────────────────── */
  .model-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 6px;
  }

  .btn-primary, .btn-outline, .retry-btn {
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

  .btn-primary:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-primary:disabled { opacity: 0.45; cursor: not-allowed; }

  .btn-outline {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .btn-outline:hover:not(:disabled) { border-color: var(--color-text-tertiary); }

  .btn-outline.btn-danger {
    border-color: color-mix(in srgb, var(--color-error) 40%, transparent);
    color: var(--color-error);
  }

  .btn-outline.btn-danger:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-error) 8%, transparent);
    border-color: var(--color-error);
  }

  .btn-outline:disabled, .btn-outline.btn-danger:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .retry-btn {
    background: var(--color-bg-tertiary);
    border: none;
  }

  .add-custom-btn {
    align-self: flex-start;
    padding: 8px 16px;
  }

  /* ── Progress ─────────────────────────────────────────────────── */
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

  @keyframes pulse { 0%, 100% { opacity: 0.6; } 50% { opacity: 1; } }

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

  /* ── Lightning Whisper controls ───────────────────────────────── */
  .lightning-controls {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 8px;
  }

  .control-group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .control-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-tertiary);
  }

  .control-select {
    padding: 5px 8px;
    font-size: var(--text-sm);
    background: var(--color-bg-tertiary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-primary);
    min-width: 140px;
  }

  .segmented-control {
    display: flex;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .seg-btn {
    padding: 4px 12px;
    font-size: var(--text-xs);
    font-weight: 500;
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    border: none;
    border-right: 1px solid var(--color-border);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .seg-btn:last-child { border-right: none; }

  .seg-btn.seg-active {
    background: var(--color-accent);
    color: white;
  }

  /* ── Custom model form ────────────────────────────────────────── */
  .custom-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }

  .form-row {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .path-row {
    display: flex;
    gap: 6px;
  }

  .control-input {
    padding: 6px 10px;
    font-size: var(--text-sm);
    background: var(--color-bg-tertiary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-primary);
    flex: 1;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 4px;
  }

  /* ── Empty state ──────────────────────────────────────────────── */
  .empty-state {
    padding: 32px;
    text-align: center;
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }

  /* ── Modal ────────────────────────────────────────────────────── */
  .modal-overlay {
    position: fixed;
    top: 0; left: 0; right: 0; bottom: 0;
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
