<script lang="ts">
  /**
   * Performance Analysis dialog showing transcription statistics.
   * Displays summary cards, model performance, and enhancement metrics.
   */
  import { invoke } from '@tauri-apps/api/core';
  import { formatDuration, formatTotalDuration, formatSpeedFactor } from '../utils/format';

  interface ModelStats {
    name: string;
    count: number;
    avgAudioDuration: number;
    avgProcessingTime: number;
    speedFactor: number;
  }

  interface TranscriptionStats {
    totalCount: number;
    analysableCount: number;
    enhancedCount: number;
    totalAudioDuration: number;
    transcriptionModels: ModelStats[];
    enhancementModels: ModelStats[];
  }

  interface Props {
    /** Whether the dialog is visible */
    open: boolean;
  }

  let { open = $bindable() }: Props = $props();

  let stats = $state<TranscriptionStats | null>(null);
  let isLoading = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (open) {
      loadStats();
    }
  });

  async function loadStats() {
    isLoading = true;
    error = null;
    try {
      stats = await invoke<TranscriptionStats>('get_transcription_stats_cmd');
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      isLoading = false;
    }
  }

  function close() {
    open = false;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') close();
  }

</script>

<svelte:window onkeydown={open ? handleKeydown : undefined} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="modal-overlay" onclick={close} role="presentation">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <dialog class="modal" open aria-labelledby="perf-title" onclick={(e) => e.stopPropagation()} role="dialog">
      <header class="modal-header">
        <h2 id="perf-title" class="modal-title">Performance Analysis</h2>
        <button class="close-btn" onclick={close} type="button" aria-label="Close">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6L6 18M6 6l12 12"></path>
          </svg>
        </button>
      </header>

      <div class="modal-body">
        {#if isLoading}
          <div class="loading">Loading statistics...</div>
        {:else if error}
          <div class="error">{error}</div>
        {:else if stats}
          <!-- Summary Cards -->
          <div class="cards">
            <div class="card">
              <span class="card-value">{stats.totalCount}</span>
              <span class="card-label">Total transcriptions</span>
            </div>
            <div class="card">
              <span class="card-value">{stats.analysableCount}</span>
              <span class="card-label">With performance data</span>
            </div>
            <div class="card">
              <span class="card-value">{stats.enhancedCount}</span>
              <span class="card-label">Enhanced</span>
            </div>
            <div class="card">
              <span class="card-value">{formatTotalDuration(stats.totalAudioDuration)}</span>
              <span class="card-label">Total audio</span>
            </div>
          </div>

          <!-- Transcription Model Performance -->
          {#if stats.transcriptionModels.length > 0}
            <section class="section">
              <h3 class="section-title">Transcription models</h3>
              <div class="model-list">
                {#each stats.transcriptionModels as model}
                  <div class="model-row">
                    <div class="model-name">{model.name}</div>
                    <div class="model-metrics">
                      <span class="metric">
                        <span class="metric-value">{model.count}</span>
                        <span class="metric-label">uses</span>
                      </span>
                      <span class="metric">
                        <span class="metric-value">{formatSpeedFactor(model.speedFactor)}</span>
                        <span class="metric-label">RTFX</span>
                      </span>
                      <span class="metric">
                        <span class="metric-value">{formatDuration(model.avgAudioDuration)}</span>
                        <span class="metric-label">avg audio</span>
                      </span>
                      <span class="metric">
                        <span class="metric-value">{formatDuration(model.avgProcessingTime)}</span>
                        <span class="metric-label">avg time</span>
                      </span>
                    </div>
                  </div>
                {/each}
              </div>
            </section>
          {/if}

          <!-- Enhancement Model Performance -->
          {#if stats.enhancementModels.length > 0}
            <section class="section">
              <h3 class="section-title">Enhancement models</h3>
              <div class="model-list">
                {#each stats.enhancementModels as model}
                  <div class="model-row">
                    <div class="model-name">{model.name}</div>
                    <div class="model-metrics">
                      <span class="metric">
                        <span class="metric-value">{model.count}</span>
                        <span class="metric-label">uses</span>
                      </span>
                      <span class="metric">
                        <span class="metric-value">{formatDuration(model.avgProcessingTime)}</span>
                        <span class="metric-label">avg time</span>
                      </span>
                    </div>
                  </div>
                {/each}
              </div>
            </section>
          {/if}

          {#if stats.transcriptionModels.length === 0 && stats.enhancementModels.length === 0}
            <div class="empty">
              <p>No performance data recorded yet.</p>
              <p class="empty-hint">Statistics will appear after transcriptions are created with the updated app.</p>
            </div>
          {/if}
        {/if}
      </div>
    </dialog>
  </div>
{/if}

<style>
  .modal-overlay {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    z-index: 1000;
    animation: fadeIn 0.15s ease;
  }

  .modal {
    width: 90%;
    max-width: 560px;
    max-height: 80vh;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    animation: scaleIn 0.15s ease;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--color-border);
  }

  .modal-title {
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text-primary);
    margin: 0;
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
  }

  .close-btn:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }

  .close-btn svg {
    width: 16px;
    height: 16px;
  }

  .modal-body {
    padding: var(--spacing-lg);
    overflow-y: auto;
  }

  .loading, .error, .empty {
    text-align: center;
    padding: var(--spacing-xl);
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }

  .error {
    color: var(--color-error);
  }

  .empty-hint {
    margin-top: var(--spacing-sm);
    font-size: var(--text-xs);
    opacity: 0.7;
  }

  /* Summary cards */
  .cards {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: var(--spacing-sm);
    margin-bottom: var(--spacing-lg);
  }

  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--spacing-md);
    background: var(--color-bg-primary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .card-value {
    font-size: var(--text-xl, 20px);
    font-weight: 600;
    color: var(--color-text-primary);
    font-variant-numeric: tabular-nums;
  }

  .card-label {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    margin-top: var(--spacing-xs);
  }

  /* Sections */
  .section {
    margin-bottom: var(--spacing-lg);
  }

  .section:last-child {
    margin-bottom: 0;
  }

  .section-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0 0 var(--spacing-sm) 0;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-sm);
  }

  .model-row {
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--color-bg-primary);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
  }

  .model-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
    margin-bottom: var(--spacing-xs);
    font-family: var(--font-mono, monospace);
  }

  .model-metrics {
    display: flex;
    gap: var(--spacing-lg);
    flex-wrap: wrap;
  }

  .metric {
    display: flex;
    align-items: baseline;
    gap: var(--spacing-xs);
  }

  .metric-value {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-accent);
    font-variant-numeric: tabular-nums;
  }

  .metric-label {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes scaleIn {
    from { opacity: 0; transform: scale(0.95); }
    to { opacity: 1; transform: scale(1); }
  }
</style>
