<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { pipelineStore } from '../stores/pipeline.svelte';
  import { Upload, FileAudio, Copy, Check, X, Loader2 } from 'lucide-svelte';

  let isDragOver = $state(false);
  let importedFileName = $state<string | null>(null);
  let copied = $state(false);

  const AUDIO_EXTENSIONS = ['wav', 'mp3', 'm4a', 'ogg', 'flac'];
  const AUDIO_REGEX = /\.(wav|mp3|m4a|ogg|flac)$/i;

  const isProcessing = $derived(
    importedFileName !== null &&
      (pipelineStore.state === 'converting' ||
        pipelineStore.state === 'transcribing' ||
        pipelineStore.state === 'filtering' ||
        pipelineStore.state === 'enhancing')
  );

  const hasResult = $derived(
    importedFileName !== null &&
      pipelineStore.state === 'completed' &&
      pipelineStore.lastResult !== null
  );

  const hasError = $derived(
    importedFileName !== null && pipelineStore.state === 'failed' && pipelineStore.error !== null
  );

  async function handleFilePicker() {
    if (isProcessing) return;

    const selected = await open({
      multiple: false,
      filters: [{ name: 'Audio', extensions: AUDIO_EXTENSIONS }],
    });

    if (selected) {
      importedFileName = selected.split('/').pop() ?? selected;
      pipelineStore.transcribeFile(selected);
    }
  }

  async function handleTranscribeFile(filePath: string) {
    importedFileName = filePath.split('/').pop() ?? filePath;
    await pipelineStore.transcribeFile(filePath);
  }

  async function handleCopy() {
    const text = pipelineStore.lastResult?.text;
    if (!text) return;

    try {
      await writeText(text);
      copied = true;
      setTimeout(() => {
        copied = false;
      }, 2000);
    } catch (e) {
      console.error('Failed to copy:', e);
    }
  }

  function handleCancel() {
    pipelineStore.cancel();
    importedFileName = null;
  }

  function handleReset() {
    pipelineStore.reset();
    importedFileName = null;
  }

  $effect(() => {
    let unlisten: (() => void) | undefined;

    getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        if (event.payload.type === 'enter' || event.payload.type === 'over') {
          isDragOver = true;
        } else if (event.payload.type === 'leave') {
          isDragOver = false;
        } else if (event.payload.type === 'drop') {
          isDragOver = false;
          const paths = event.payload.paths.filter((p: string) => AUDIO_REGEX.test(p));
          if (paths.length > 0) {
            handleTranscribeFile(paths[0]);
          }
        }
      })
      .then((fn) => {
        unlisten = fn;
      });

    return () => {
      unlisten?.();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div
  class="transcribe-dropzone"
  class:drag-over={isDragOver}
  class:disabled={isProcessing}
  onclick={handleFilePicker}
>
  {#if isProcessing}
    <span class="dropzone-icon-wrapper">
      <Loader2 size={36} class="spinner" />
    </span>
    <p class="dropzone-text">{pipelineStore.message || 'Processing...'}</p>
    {#if importedFileName}
      <p class="dropzone-hint">{importedFileName}</p>
    {/if}
  {:else}
    <span class="dropzone-icon-wrapper">
      <Upload size={36} />
    </span>
    <p class="dropzone-text">Drop audio files here or click to browse</p>
    <p class="dropzone-hint">Supports WAV, MP3, M4A, OGG, FLAC</p>
  {/if}
</div>

{#if isProcessing}
  <div class="import-actions">
    <button onclick={handleCancel}>
      <X size={14} />
      Cancel
    </button>
  </div>
{/if}

{#if hasResult}
  <div class="import-result">
    <div class="result-header">
      <span class="result-label">
        <FileAudio size={14} />
        {importedFileName ?? 'Transcription'}
      </span>
      <div class="result-actions">
        <button class="primary btn-sm" onclick={handleCopy}>
          {#if copied}
            <Check size={14} />
            Copied
          {:else}
            <Copy size={14} />
            Copy
          {/if}
        </button>
        <button class="btn-sm" onclick={handleReset}>
          New Import
        </button>
      </div>
    </div>
    <div class="result-text">{pipelineStore.lastResult?.text}</div>
  </div>
{/if}

{#if hasError}
  <div class="import-error">
    <p class="error-message">{pipelineStore.error}</p>
    <button class="btn-small" onclick={handleReset}>
      Dismiss
    </button>
  </div>
{/if}

<style>
  /* Dropzone */
  .transcribe-dropzone {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 48px 24px;
    border: 2px dashed var(--color-border);
    border-radius: var(--radius-lg);
    cursor: pointer;
    transition:
      border-color var(--transition-fast),
      background var(--transition-fast);
  }

  .transcribe-dropzone:hover:not(.disabled) {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 5%, transparent);
  }

  .transcribe-dropzone.drag-over {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    border-style: solid;
  }

  .transcribe-dropzone.disabled {
    cursor: default;
    opacity: 0.8;
  }

  .dropzone-icon-wrapper {
    margin-bottom: 12px;
    color: var(--color-text-secondary);
  }

  .dropzone-text {
    margin: 0;
    font-size: var(--text-base);
    color: var(--color-text-primary);
  }

  .dropzone-hint {
    margin: 4px 0 0;
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  /* Spinner animation */
  :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Actions below dropzone */
  .import-actions {
    display: flex;
    justify-content: center;
    margin-top: var(--spacing-sm);
  }

  /* Small button variant for result actions */
  .btn-sm {
    display: inline-flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: 4px 8px;
    font-size: var(--text-xs);
  }

  /* Result display */
  .import-result {
    margin-top: var(--spacing-md);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .result-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--color-bg-secondary);
    border-bottom: 1px solid var(--color-border-subtle);
  }

  .result-label {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .result-actions {
    display: flex;
    gap: var(--spacing-xs);
    flex-shrink: 0;
  }

  .result-text {
    padding: var(--spacing-md);
    font-size: var(--text-sm);
    line-height: 1.6;
    color: var(--color-text-primary);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 300px;
    overflow-y: auto;
  }

  /* Error display - uses global .error-message for text, custom layout for row */
  .import-error {
    margin-top: var(--spacing-md);
    padding: var(--spacing-md);
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--spacing-md);
  }

  .error-message {
    margin: 0;
    padding: 0;
    background: none;
  }
</style>
