<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { Button } from '$components/ui/button';
  import { Badge } from '$components/ui/badge';
  import * as Card from '$components/ui/card';
  import * as AlertDialog from '$components/ui/alert-dialog';
  import * as Alert from '$components/ui/alert';

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

<div class="flex flex-col gap-6">
  <div class="flex items-center justify-between rounded-lg border p-4">
    <div class="flex flex-col gap-1">
      <span class="text-sm font-medium">Model Updates</span>
      <span class="text-muted-foreground text-xs">
        {#if lastChecked}
          Last checked: {lastChecked}
        {:else}
          Check for new or updated transcription models
        {/if}
      </span>
    </div>
    <Button
      variant="outline"
      size="sm"
      onclick={checkForUpdates}
      disabled={checking || isDownloading()}
    >
      {#if checking}
        Checking...
      {:else}
        Check for Updates
      {/if}
    </Button>
  </div>

  {#if loading}
    <div class="text-muted-foreground flex items-center gap-3 p-6">
      <div
        class="border-muted-foreground border-t-primary h-5 w-5 animate-spin rounded-full border-2"
      ></div>
      <span class="text-sm">Loading models...</span>
    </div>
  {:else if error}
    <Alert.Root variant="destructive">
      <Alert.Description class="flex items-center justify-between gap-3">
        <span>{error}</span>
        <Button variant="ghost" size="sm" onclick={() => (error = null)}>Dismiss</Button>
      </Alert.Description>
    </Alert.Root>
  {/if}

  <div class="flex flex-col gap-3">
    {#each sortedModels(models) as model (model.id)}
      <Card.Root
        class={model.selected
          ? 'border-primary'
          : !model.backend_available
            ? 'opacity-55'
            : ''}
      >
        <Card.Content class="flex flex-col gap-2 p-4">
          <!-- Top row: name + primary badge -->
          <div class="flex items-center justify-between gap-2">
            <div class="flex min-w-0 items-center gap-2">
              <span class="truncate text-sm font-semibold">{model.name}</span>
              {#if model.selected}
                <Badge>Active</Badge>
              {:else if model.recommended}
                <Badge variant="secondary">Recommended</Badge>
              {/if}
            </div>
            {#if model.update_available}
              <Badge variant="outline">Update</Badge>
            {/if}
          </div>

          <!-- Description -->
          <p class="text-muted-foreground text-xs leading-relaxed">{model.description}</p>

          <!-- Metadata row -->
          <div class="text-muted-foreground flex flex-wrap items-center gap-0 text-xs">
            <span class="whitespace-nowrap">{backendLabel(model.model_type)}</span>
            <span class="px-1.5 opacity-40">&middot;</span>
            <span class="whitespace-nowrap">{formatLanguages(model.languages)}</span>
            <span class="px-1.5 opacity-40">&middot;</span>
            {#if model.downloaded && model.disk_size}
              <span class="whitespace-nowrap">{formatBytes(model.disk_size)}</span>
            {:else}
              <span class="whitespace-nowrap">~{model.size_mb} MB</span>
            {/if}
          </div>

          <!-- Backend warning -->
          {#if !model.backend_available}
            <p class="bg-warning/10 text-warning rounded-sm px-2.5 py-1.5 text-xs leading-snug">
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
          <div class="flex items-center gap-2 pt-2">
            {#if isDownloading(model.id)}
              <div class="flex flex-1 flex-col gap-2">
                {#if progress}
                  <!-- Custom progress bar: bespoke because it shows download % with animation -->
                  <div class="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                    <div
                      class="bg-primary h-full rounded-full transition-[width] duration-300"
                      style:width="{Math.min(progress.percentage, 100)}%"
                    ></div>
                  </div>
                  <span class="text-muted-foreground text-xs">{progress.status}</span>
                {:else if downloadState === 'Extracting'}
                  <div class="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                    <div class="bg-primary h-full w-full animate-pulse rounded-full"></div>
                  </div>
                  <span class="text-muted-foreground text-xs">Extracting model files...</span>
                {:else}
                  <span class="text-muted-foreground text-xs">Preparing download...</span>
                {/if}
              </div>
            {:else if isFailed() && downloadingModelId === model.id}
              <div class="flex flex-1 items-center gap-3">
                <span class="text-destructive flex-1 text-xs">
                  Download failed: {getFailedMessage()}
                </span>
                <Button variant="secondary" size="sm" onclick={() => resetState()}>Reset</Button>
              </div>
            {:else if initialisingModelId === model.id}
              <div class="flex flex-1 flex-col gap-2">
                <!-- Custom progress bar: bespoke indeterminate pulse for compile/load -->
                <div class="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                  <div class="bg-primary h-full w-full animate-pulse rounded-full"></div>
                </div>
                <span class="text-muted-foreground text-xs">
                  {model.model_type === 'fluidaudio_coreml'
                    ? 'Compiling for Neural Engine… this may take a minute'
                    : 'Loading model…'}
                </span>
              </div>
            {:else if model.downloaded && model.selected}
              <Button
                variant="destructive"
                size="sm"
                onclick={() => confirmDelete(model)}
                disabled={isDownloading() || initialisingModelId !== null}
              >
                Delete
              </Button>
            {:else if model.downloaded}
              <Button
                size="sm"
                onclick={() => selectModel(model)}
                disabled={isDownloading() ||
                  initialisingModelId !== null ||
                  !model.backend_available}
              >
                {model.backend_available ? 'Use Model' : 'Unavailable'}
              </Button>
              <Button
                variant="destructive"
                size="sm"
                onclick={() => confirmDelete(model)}
                disabled={isDownloading() || initialisingModelId !== null}
              >
                Delete
              </Button>
            {:else}
              <Button
                size="sm"
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
              </Button>
            {/if}
          </div>
        </Card.Content>
      </Card.Root>
    {/each}
  </div>

  {#if models.length === 0 && !loading}
    <div class="text-muted-foreground p-8 text-center text-sm">
      <p>No models available. Click "Check for Updates" to refresh the list.</p>
    </div>
  {/if}
</div>

<!-- Delete confirmation dialog -->
<AlertDialog.Root bind:open={showDeleteConfirm}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Delete Model?</AlertDialog.Title>
      <AlertDialog.Description>
        Are you sure you want to delete "{modelToDelete?.name}"? You will need to download it again
        to use offline transcription.
      </AlertDialog.Description>
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action
        class="bg-destructive text-destructive-foreground hover:bg-destructive/90"
        onclick={deleteModel}
      >
        Delete
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
