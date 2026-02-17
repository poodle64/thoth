<script lang="ts">
  /**
   * Export dialog component for exporting transcription history.
   * Supports JSON, CSV, and TXT formats with date range filtering.
   */
  import { invoke } from '@tauri-apps/api/core';
  import { save } from '@tauri-apps/plugin-dialog';

  interface Props {
    /** Whether the dialog is visible */
    open: boolean;
    /** Selected transcription IDs to export (empty for all) */
    selectedIds?: string[];
    /** Callback when dialog is closed */
    onclose?: () => void;
  }

  type ExportFormat = 'json' | 'csv' | 'txt';

  let { open = $bindable(), selectedIds = [], onclose }: Props = $props();

  let format = $state<ExportFormat>('json');
  let useSearchFilter = $state(false);
  let searchQuery = $state('');
  let fromDate = $state('');
  let toDate = $state('');
  let enhancedOnly = $state(false);
  let isExporting = $state(false);
  let exportError = $state<string | null>(null);
  let exportSuccess = $state<string | null>(null);

  // Computed description of what will be exported
  let exportDescription = $derived(() => {
    if (selectedIds.length > 0) {
      return `${selectedIds.length} selected transcription${selectedIds.length === 1 ? '' : 's'}`;
    }
    if (useSearchFilter) {
      const filters: string[] = [];
      if (searchQuery) filters.push(`matching "${searchQuery}"`);
      if (fromDate) filters.push(`from ${formatDateForDisplay(fromDate)}`);
      if (toDate) filters.push(`until ${formatDateForDisplay(toDate)}`);
      if (enhancedOnly) filters.push('enhanced only');
      return filters.length > 0 ? `All transcriptions ${filters.join(', ')}` : 'All transcriptions';
    }
    return 'All transcriptions';
  });

  function formatDateForDisplay(dateStr: string): string {
    if (!dateStr) return '';
    const date = new Date(dateStr);
    return date.toLocaleDateString('en-AU', { day: '2-digit', month: '2-digit', year: 'numeric' });
  }

  function dateToTimestamp(dateStr: string, isEndOfDay: boolean = false): number | null {
    if (!dateStr) return null;
    const date = new Date(dateStr);
    if (isEndOfDay) {
      date.setHours(23, 59, 59, 999);
    } else {
      date.setHours(0, 0, 0, 0);
    }
    return Math.floor(date.getTime() / 1000);
  }

  function getFileExtension(fmt: ExportFormat): string {
    switch (fmt) {
      case 'json':
        return 'json';
      case 'csv':
        return 'csv';
      case 'txt':
        return 'txt';
    }
  }

  function getFormatName(fmt: ExportFormat): string {
    switch (fmt) {
      case 'json':
        return 'JSON';
      case 'csv':
        return 'CSV';
      case 'txt':
        return 'Plain Text';
    }
  }

  async function handleExport() {
    exportError = null;
    exportSuccess = null;
    isExporting = true;

    try {
      // Show save dialog
      const extension = getFileExtension(format);
      const filePath = await save({
        defaultPath: `thoth-export.${extension}`,
        filters: [
          {
            name: getFormatName(format),
            extensions: [extension],
          },
        ],
      });

      if (!filePath) {
        isExporting = false;
        return; // User cancelled
      }

      // Build search params if filtering
      const searchParams =
        useSearchFilter && selectedIds.length === 0
          ? {
              query: searchQuery || null,
              from_date: dateToTimestamp(fromDate, false),
              to_date: dateToTimestamp(toDate, true),
              enhanced_only: enhancedOnly || null,
              limit: 10000,
              offset: 0,
            }
          : null;

      // Call the appropriate export command
      let exportedCount: number;
      switch (format) {
        case 'json':
          exportedCount = await invoke<number>('export_to_json', {
            ids: selectedIds,
            path: filePath,
            searchParams,
          });
          break;
        case 'csv':
          exportedCount = await invoke<number>('export_to_csv', {
            ids: selectedIds,
            path: filePath,
            searchParams,
          });
          break;
        case 'txt':
          exportedCount = await invoke<number>('export_to_txt', {
            ids: selectedIds,
            path: filePath,
            searchParams,
          });
          break;
      }

      exportSuccess = `Successfully exported ${exportedCount} transcription${exportedCount === 1 ? '' : 's'}`;

      // Close dialog after short delay
      setTimeout(() => {
        handleClose();
      }, 1500);
    } catch (error) {
      exportError = error instanceof Error ? error.message : String(error);
    } finally {
      isExporting = false;
    }
  }

  function handleClose() {
    open = false;
    exportError = null;
    exportSuccess = null;
    onclose?.();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      handleClose();
    }
  }
</script>

{#if open}
  <div class="dialog-overlay" role="presentation" onclick={handleClose} onkeydown={handleKeydown}>
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="dialog-title"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
    >
      <header class="dialog-header">
        <h2 id="dialog-title">Export Transcriptions</h2>
        <button class="close-button" onclick={handleClose} aria-label="Close dialog">
          <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
            <path
              d="M4.646 4.646a.5.5 0 0 1 .708 0L8 7.293l2.646-2.647a.5.5 0 0 1 .708.708L8.707 8l2.647 2.646a.5.5 0 0 1-.708.708L8 8.707l-2.646 2.647a.5.5 0 0 1-.708-.708L7.293 8 4.646 5.354a.5.5 0 0 1 0-.708z"
            />
          </svg>
        </button>
      </header>

      <div class="dialog-content">
        <!-- Export summary -->
        <div class="export-summary">
          <span class="summary-label">Exporting:</span>
          <span class="summary-value">{exportDescription()}</span>
        </div>

        <!-- Format selection -->
        <fieldset class="field-group">
          <legend>Export Format</legend>
          <div class="format-options">
            <label class="format-option">
              <input type="radio" name="format" value="json" bind:group={format} />
              <span class="format-label">
                <strong>JSON</strong>
                <small>Full data, machine-readable</small>
              </span>
            </label>
            <label class="format-option">
              <input type="radio" name="format" value="csv" bind:group={format} />
              <span class="format-label">
                <strong>CSV</strong>
                <small>Spreadsheet compatible</small>
              </span>
            </label>
            <label class="format-option">
              <input type="radio" name="format" value="txt" bind:group={format} />
              <span class="format-label">
                <strong>Plain Text</strong>
                <small>Human readable</small>
              </span>
            </label>
          </div>
        </fieldset>

        <!-- Filter options (only if no specific selection) -->
        {#if selectedIds.length === 0}
          <fieldset class="field-group">
            <legend>
              <label class="checkbox-label">
                <input type="checkbox" bind:checked={useSearchFilter} />
                Filter Results
              </label>
            </legend>

            {#if useSearchFilter}
              <div class="filter-fields">
                <div class="field">
                  <label for="search-query">Search text</label>
                  <input
                    type="text"
                    id="search-query"
                    placeholder="Search in transcriptions..."
                    bind:value={searchQuery}
                  />
                </div>

                <div class="date-range">
                  <div class="field">
                    <label for="from-date">From date</label>
                    <input type="date" id="from-date" bind:value={fromDate} />
                  </div>
                  <div class="field">
                    <label for="to-date">To date</label>
                    <input type="date" id="to-date" bind:value={toDate} />
                  </div>
                </div>

                <label class="checkbox-label enhanced-filter">
                  <input type="checkbox" bind:checked={enhancedOnly} />
                  Enhanced transcriptions only
                </label>
              </div>
            {/if}
          </fieldset>
        {/if}

        <!-- Status messages -->
        {#if exportError}
          <div class="status-message error" role="alert">
            {exportError}
          </div>
        {/if}

        {#if exportSuccess}
          <div class="status-message success" role="status">
            {exportSuccess}
          </div>
        {/if}
      </div>

      <footer class="dialog-footer">
        <button class="btn btn-secondary" onclick={handleClose} disabled={isExporting}>
          Cancel
        </button>
        <button class="btn btn-primary" onclick={handleExport} disabled={isExporting}>
          {#if isExporting}
            Exporting...
          {:else}
            Export
          {/if}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    backdrop-filter: blur(2px);
  }

  .dialog {
    background: var(--color-bg-primary);
    border-radius: 12px;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.3);
    width: 100%;
    max-width: 480px;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--color-border);
  }

  .dialog-header h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .close-button {
    background: none;
    border: none;
    padding: 4px;
    cursor: pointer;
    color: var(--color-text-tertiary);
    border-radius: 4px;
    transition:
      background 0.15s ease,
      color 0.15s ease;
  }

  .close-button:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
  }

  .dialog-content {
    padding: 20px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .export-summary {
    background: var(--color-bg-secondary);
    padding: 12px 16px;
    border-radius: 8px;
    display: flex;
    gap: 8px;
  }

  .summary-label {
    color: var(--color-text-tertiary);
    font-size: 14px;
  }

  .summary-value {
    color: var(--color-text-primary);
    font-size: 14px;
    font-weight: 500;
  }

  .field-group {
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 16px;
    margin: 0;
  }

  .field-group legend {
    padding: 0 8px;
    font-size: 13px;
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  .format-options {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .format-option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 12px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .format-option:hover {
    background: var(--color-bg-secondary);
  }

  .format-option input[type='radio'] {
    margin-top: 3px;
    accent-color: var(--color-accent);
  }

  .format-label {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .format-label strong {
    color: var(--color-text-primary);
    font-size: 14px;
  }

  .format-label small {
    color: var(--color-text-tertiary);
    font-size: 12px;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 13px;
    color: var(--color-text-secondary);
  }

  .checkbox-label input[type='checkbox'] {
    accent-color: var(--color-accent);
  }

  .filter-fields {
    margin-top: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field label {
    font-size: 13px;
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  .field input[type='text'],
  .field input[type='date'] {
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: var(--color-bg-secondary);
    color: var(--color-text-primary);
    font-size: 14px;
  }

  .field input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 2px rgba(var(--color-accent-rgb), 0.2);
  }

  .field input::placeholder {
    color: var(--color-text-tertiary);
  }

  .date-range {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .enhanced-filter {
    margin-top: 4px;
  }

  .status-message {
    padding: 12px 16px;
    border-radius: 8px;
    font-size: 14px;
  }

  .status-message.error {
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
    color: var(--color-error);
    border: 1px solid color-mix(in srgb, var(--color-error) 30%, transparent);
  }

  .status-message.success {
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    color: var(--color-success);
    border: 1px solid color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 16px 20px;
    border-top: 1px solid var(--color-border);
  }

  .btn {
    padding: 10px 20px;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition:
      background 0.15s ease,
      border-color 0.15s ease;
  }

  .btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .btn-secondary {
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    color: var(--color-text-primary);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
  }

  .btn-primary {
    background: var(--color-accent);
    border: 1px solid var(--color-accent);
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>
