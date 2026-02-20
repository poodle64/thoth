<script lang="ts">
  /**
   * FilterSettings - Configuration UI for transcription output filtering
   *
   * Provides toggles for each filter option with a live preview showing
   * the effect of filters on sample text.
   */

  import { invoke } from '@tauri-apps/api/core';

  /** Filter options matching the Rust FilterOptions struct */
  interface FilterOptions {
    remove_fillers: boolean;
    normalise_whitespace: boolean;
    cleanup_punctuation: boolean;
    sentence_case: boolean;
  }

  interface Props {
    /** Initial filter options */
    initialOptions?: FilterOptions;
    /** Callback when options change */
    onchange?: (options: FilterOptions) => void;
    /** Callback to navigate to dictionary settings */
    onOpenDictionary?: () => void;
  }

  let { initialOptions, onchange, onOpenDictionary }: Props = $props();

  /** Default filter options matching Rust defaults */
  const defaultOptions: FilterOptions = {
    remove_fillers: true,
    normalise_whitespace: true,
    cleanup_punctuation: true,
    sentence_case: false,
  };

  /** Current filter options state - intentionally captures initialOptions once */
  // svelte-ignore state_referenced_locally
  let options = $state<FilterOptions>(initialOptions ?? { ...defaultOptions });

  /** Sample text for preview */
  let sampleText = $state(
    'um, so like, I think that uh, this is, you know, working...what do you think ??'
  );

  /** Whether the sample text input is expanded */
  let isSampleExpanded = $state(false);

  /** Filtered preview text */
  let filteredText = $state('');

  /** Whether preview is loading */
  let isLoading = $state(false);

  /** Error message if preview fails */
  let error = $state<string | null>(null);

  /**
   * Update the preview by calling the Rust filter_transcription command
   */
  async function updatePreview() {
    isLoading = true;
    error = null;

    try {
      filteredText = await invoke<string>('filter_transcription', {
        text: sampleText,
        options,
      });
    } catch (e) {
      error = `Failed to filter text: ${e}`;
      filteredText = sampleText;
    } finally {
      isLoading = false;
    }
  }

  /**
   * Toggle a filter option
   */
  function toggleOption(key: keyof FilterOptions) {
    options = { ...options, [key]: !options[key] };
    onchange?.(options);
  }

  /**
   * Reset all options to defaults
   */
  function resetToDefaults() {
    options = { ...defaultOptions };
    onchange?.(options);
  }

  /** Whether any option differs from default */
  let hasChanges = $derived(
    options.remove_fillers !== defaultOptions.remove_fillers ||
      options.normalise_whitespace !== defaultOptions.normalise_whitespace ||
      options.cleanup_punctuation !== defaultOptions.cleanup_punctuation ||
      options.sentence_case !== defaultOptions.sentence_case
  );

  /** Filter option definitions for rendering */
  const filterDefinitions = [
    {
      key: 'remove_fillers' as const,
      label: 'Remove filler words',
      description: 'Removes common filler words like um, uh, er, ah, like, you know',
    },
    {
      key: 'normalise_whitespace' as const,
      label: 'Normalise whitespace',
      description: 'Collapses multiple spaces and trims leading/trailing whitespace',
    },
    {
      key: 'cleanup_punctuation' as const,
      label: 'Clean up punctuation',
      description: 'Removes duplicate punctuation and fixes spacing around punctuation',
    },
    {
      key: 'sentence_case' as const,
      label: 'Apply sentence case',
      description: 'Capitalises the first letter of each sentence',
    },
  ];

  // Update preview when options or sample text change
  $effect(() => {
    // Track all dependencies
    void options;
    void sampleText;
    updatePreview();
  });
</script>

<div class="filter-settings">
  <div class="setting-group">
    <h3>Output Filters</h3>

    <div class="filter-options">
      {#each filterDefinitions as filter}
        <div class="setting-row card">
          <div class="setting-info">
            <span class="setting-label">{filter.label}</span>
            <span class="setting-description">{filter.description}</span>
          </div>
          <label class="toggle-switch">
            <input
              type="checkbox"
              checked={options[filter.key]}
              onchange={() => toggleOption(filter.key)}
            />
            <span class="toggle-slider"></span>
          </label>
        </div>
      {/each}
    </div>

    {#if hasChanges}
      <button type="button" class="reset-btn" onclick={resetToDefaults}> Reset to defaults </button>
    {/if}
  </div>

  <div class="setting-group">
    <div class="preview-header">
      <h3>Preview</h3>
      <button
        type="button"
        class="expand-btn"
        onclick={() => (isSampleExpanded = !isSampleExpanded)}
        aria-expanded={isSampleExpanded}
      >
        {isSampleExpanded ? 'Hide sample input' : 'Edit sample input'}
      </button>
    </div>

    {#if isSampleExpanded}
      <div class="sample-input-container">
        <label for="sample-text" class="sample-label">Sample text:</label>
        <textarea
          id="sample-text"
          class="sample-input"
          bind:value={sampleText}
          rows="3"
          placeholder="Enter sample text to test filters..."
        ></textarea>
      </div>
    {/if}

    <div class="preview-container">
      <div class="preview-box">
        <span class="preview-label">Before:</span>
        <p class="preview-text original">{sampleText}</p>
      </div>

      <div class="preview-arrow">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
        >
          <line x1="5" y1="12" x2="19" y2="12"></line>
          <polyline points="12 5 19 12 12 19"></polyline>
        </svg>
      </div>

      <div class="preview-box">
        <span class="preview-label">After:</span>
        {#if isLoading}
          <p class="preview-text loading">Processing...</p>
        {:else if error}
          <p class="preview-text error">{error}</p>
        {:else}
          <p class="preview-text filtered">{filteredText || '(empty)'}</p>
        {/if}
      </div>
    </div>
  </div>

  <div class="setting-group">
    <h3>Custom Word Replacements</h3>
    <p class="hint">
      Configure custom word replacements in the Dictionary settings.
    </p>
    <button type="button" class="btn-outline" onclick={() => onOpenDictionary?.()}>
      Open Dictionary Settings
    </button>
  </div>
</div>

<style>
  .filter-settings {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .filter-options {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .reset-btn {
    align-self: flex-start;
    padding: 6px 12px;
    font-size: var(--text-sm);
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .reset-btn:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
  }

  .preview-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .expand-btn {
    padding: 4px 10px;
    font-size: var(--text-xs);
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .expand-btn:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
  }

  .sample-input-container {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .sample-label {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
  }

  .sample-input {
    resize: vertical;
    min-height: 60px;
    font-family: var(--font-sans);
    line-height: 1.5;
  }

  .preview-container {
    display: flex;
    align-items: stretch;
    gap: 12px;
    padding: 12px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border-subtle);
  }

  .preview-box {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .preview-label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .preview-text {
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.5;
    word-break: break-word;
  }

  .preview-text.original {
    color: var(--color-text-secondary);
  }

  .preview-text.filtered {
    color: var(--color-text-primary);
  }

  .preview-text.loading {
    color: var(--color-text-tertiary);
    font-style: italic;
  }

  .preview-text.error {
    color: var(--color-error);
    font-size: var(--text-xs);
  }

  .preview-arrow {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: var(--color-text-tertiary);
  }

</style>
