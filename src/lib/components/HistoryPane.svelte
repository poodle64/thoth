<script lang="ts">
  /**
   * History pane - 3-panel layout for managing transcription history.
   *
   * Reusable component that can be embedded in the Settings window or
   * used standalone in the History window. Contains the full history UI
   * without window chrome (title bar, drag region).
   *
   * Layout:
   * - Left pane: Scrollable list of transcriptions
   * - Main pane: Selected transcription details
   * - Right pane (collapsible): Search and filter controls
   */

  import { onMount } from 'svelte';
  import type { TranscriptionRecord } from '../stores/history.svelte';
  import { historyStore } from '../stores/history.svelte';
  import HistoryList from './HistoryList.svelte';
  import HistoryFilterPanel, { type FilterState } from './HistoryFilterPanel.svelte';
  import ExportDialog from './ExportDialog.svelte';
  import PerformanceDialog from './PerformanceDialog.svelte';
  import AudioPlayer from './AudioPlayer.svelte';

  /** Default filter state */
  const defaultFilters: FilterState = {
    searchQuery: '',
    fromDate: '',
    toDate: '',
    minDuration: null,
    maxDuration: null,
    showEnhancedOnly: false,
    showUnenhancedOnly: false,
  };

  let copyFeedback = $state<string | null>(null);
  let deleteConfirm = $state<TranscriptionRecord | null>(null);
  let showExportDialog = $state(false);
  let showPerformanceDialog = $state(false);
  let showFilterPanel = $state(false);
  let showMetadata = $state(false);
  let filters = $state<FilterState>({ ...defaultFilters });
  let bulkSelectedIds = $state(new Set<string>());
  let bulkDeleteConfirm = $state(false);
  let bulkExportIds = $state<string[]>([]);

  /** Apply advanced filters to records */
  const filteredRecords = $derived.by(() => {
    let records = historyStore.records;

    // Text search
    if (filters.searchQuery.trim()) {
      const query = filters.searchQuery.toLowerCase();
      records = records.filter(
        (record) =>
          record.text.toLowerCase().includes(query) ||
          historyStore.formatDate(record.timestamp).toLowerCase().includes(query)
      );
    }

    // Date range filter
    if (filters.fromDate) {
      const fromDate = new Date(filters.fromDate);
      fromDate.setHours(0, 0, 0, 0);
      records = records.filter((record) => record.timestamp >= fromDate);
    }

    if (filters.toDate) {
      const toDate = new Date(filters.toDate);
      toDate.setHours(23, 59, 59, 999);
      records = records.filter((record) => record.timestamp <= toDate);
    }

    // Duration filter
    if (filters.minDuration !== null) {
      records = records.filter((record) => record.duration >= filters.minDuration!);
    }

    if (filters.maxDuration !== null) {
      records = records.filter((record) => record.duration <= filters.maxDuration!);
    }

    // Enhancement status filter
    if (filters.showEnhancedOnly) {
      records = records.filter((record) => record.enhanced === true);
    } else if (filters.showUnenhancedOnly) {
      records = records.filter((record) => !record.enhanced);
    }

    return records;
  });

  /** Check if any filters are active */
  const hasActiveFilters = $derived(
    filters.searchQuery !== '' ||
      filters.fromDate !== '' ||
      filters.toDate !== '' ||
      filters.minDuration !== null ||
      filters.maxDuration !== null ||
      filters.showEnhancedOnly ||
      filters.showUnenhancedOnly
  );

  // Load initial data on mount
  onMount(() => {
    historyStore.loadRecords();
  });

  /** Handle filter changes from panel */
  function handleFilterChange(newFilters: FilterState) {
    filters = { ...newFilters };
  }

  /** Toggle filter panel visibility */
  function toggleFilterPanel() {
    showFilterPanel = !showFilterPanel;
  }

  /** Clear all filters */
  function clearFilters() {
    filters = { ...defaultFilters };
  }

  /** Handle item selection */
  function handleSelect(item: TranscriptionRecord) {
    historyStore.selectRecord(item.id);
  }

  /** Handle copy action */
  async function handleCopy(item: TranscriptionRecord) {
    const success = await historyStore.copyToClipboard(item.text);
    if (success) {
      copyFeedback = 'Copied to clipboard';
      setTimeout(() => {
        copyFeedback = null;
      }, 2000);
    }
  }

  /** Handle copy from detail view */
  async function handleCopySelected() {
    if (historyStore.selectedRecord) {
      await handleCopy(historyStore.selectedRecord);
    }
  }

  /** Handle delete request */
  function handleDeleteRequest(item: TranscriptionRecord) {
    deleteConfirm = item;
  }

  /** Confirm deletion */
  async function confirmDelete() {
    if (deleteConfirm) {
      await historyStore.deleteRecord(deleteConfirm.id);
      deleteConfirm = null;
    }
  }

  /** Cancel deletion */
  function cancelDelete() {
    deleteConfirm = null;
  }

  /** Handle delete from detail view */
  function handleDeleteSelected() {
    if (historyStore.selectedRecord) {
      handleDeleteRequest(historyStore.selectedRecord);
    }
  }

  /** Load more records for infinite scroll */
  function handleLoadMore() {
    historyStore.loadMore();
  }

  /** Handle keyboard navigation in modal */
  function handleModalKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      cancelDelete();
    }
  }

  /** Handle bulk delete modal keyboard */
  function handleBulkDeleteKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      cancelBulkDelete();
    }
  }

  /** Handle global keyboard shortcuts */
  function handleGlobalKeydown(event: KeyboardEvent) {
    // Cmd+A to select all (when not in modal or input)
    if ((event.metaKey || event.ctrlKey) && event.key === 'a') {
      const target = event.target as HTMLElement;
      if (target.tagName !== 'INPUT' && target.tagName !== 'TEXTAREA') {
        event.preventDefault();
        selectAll();
      }
    }
    // Escape to clear bulk selection
    if (event.key === 'Escape' && bulkSelectedIds.size > 0 && !deleteConfirm && !bulkDeleteConfirm) {
      deselectAll();
    }
    // Backspace/Delete for bulk delete when items selected
    if ((event.key === 'Backspace' || event.key === 'Delete') && bulkSelectedIds.size > 0) {
      const target = event.target as HTMLElement;
      if (target.tagName !== 'INPUT' && target.tagName !== 'TEXTAREA') {
        event.preventDefault();
        handleBulkDeleteRequest();
      }
    }
  }

  /** Format seconds as "X.Xs" for processing times */
  function formatProcessingTime(seconds: number): string {
    return `${seconds.toFixed(1)}s`;
  }

  /** Check if selected record has any metadata to show */
  function hasMetadata(record: TranscriptionRecord): boolean {
    return !!(
      record.transcriptionModelName ||
      record.transcriptionDurationSeconds ||
      record.enhancementModelName ||
      record.enhancementDurationSeconds ||
      record.audioPath ||
      record.enhancementPrompt
    );
  }

  /** Toggle bulk selection for an item */
  function handleBulkToggle(item: TranscriptionRecord) {
    const next = new Set(bulkSelectedIds);
    if (next.has(item.id)) {
      next.delete(item.id);
    } else {
      next.add(item.id);
    }
    bulkSelectedIds = next;
  }

  /** Select all visible/filtered items */
  function selectAll() {
    bulkSelectedIds = new Set(filteredRecords.map((r) => r.id));
  }

  /** Deselect all */
  function deselectAll() {
    bulkSelectedIds = new Set();
  }

  /** Request bulk delete confirmation */
  function handleBulkDeleteRequest() {
    if (bulkSelectedIds.size > 0) {
      bulkDeleteConfirm = true;
    }
  }

  /** Confirm bulk deletion */
  async function confirmBulkDelete() {
    const ids = [...bulkSelectedIds];
    const success = await historyStore.deleteRecords(ids);
    if (success) {
      bulkSelectedIds = new Set();
    }
    bulkDeleteConfirm = false;
  }

  /** Cancel bulk deletion */
  function cancelBulkDelete() {
    bulkDeleteConfirm = false;
  }

  /** Open export dialog with bulk selection context */
  function handleBulkExport() {
    bulkExportIds = [...bulkSelectedIds];
    showExportDialog = true;
  }
</script>

<svelte:window onkeydown={deleteConfirm ? handleModalKeydown : bulkDeleteConfirm ? handleBulkDeleteKeydown : handleGlobalKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="history-pane">
  <div class="toolbar">
    <div class="toolbar-left">
      <span class="count">
        {#if hasActiveFilters}
          {filteredRecords.length} of {historyStore.records.length}
        {:else}
          {filteredRecords.length} items
        {/if}
      </span>
    </div>

    <div class="toolbar-right">
      {#if hasActiveFilters && !showFilterPanel}
        <button class="clear-filters-btn" onclick={clearFilters} type="button">
          Clear filters
        </button>
      {/if}
      <button
        class="filter-toggle-btn"
        class:active={showFilterPanel}
        onclick={toggleFilterPanel}
        aria-expanded={showFilterPanel}
        aria-label="Toggle filter panel"
        type="button"
      >
        <svg
          class="filter-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"></polygon>
        </svg>
        <span>Filters</span>
        {#if hasActiveFilters}
          <span class="filter-badge"></span>
        {/if}
      </button>
      <button
        class="btn stats-btn"
        onclick={() => (showPerformanceDialog = true)}
        title="Performance analysis"
        type="button"
      >
        <svg
          class="btn-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <line x1="18" y1="20" x2="18" y2="10"></line>
          <line x1="12" y1="20" x2="12" y2="4"></line>
          <line x1="6" y1="20" x2="6" y2="14"></line>
        </svg>
        Stats
      </button>
      <button
        class="btn export-btn"
        onclick={() => (showExportDialog = true)}
        title="Export transcriptions"
        type="button"
      >
        <svg
          class="btn-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
          <polyline points="7 10 12 15 17 10"></polyline>
          <line x1="12" y1="15" x2="12" y2="3"></line>
        </svg>
        Export
      </button>
    </div>
  </div>

  <div class="content">
    <aside class="list-panel">
      <HistoryList
        items={filteredRecords}
        selectedId={historyStore.selectedId}
        {bulkSelectedIds}
        onSelect={handleSelect}
        onBulkToggle={handleBulkToggle}
        onCopy={handleCopy}
        onDelete={handleDeleteRequest}
        onLoadMore={handleLoadMore}
        isLoading={historyStore.pagination.isLoading}
        hasMore={historyStore.pagination.hasMore && !hasActiveFilters}
      />
    </aside>

    <main class="detail-panel">
      {#if historyStore.selectedRecord}
        {@const selected = historyStore.selectedRecord}
        <div class="detail-header">
          <div class="detail-meta">
            <span class="detail-date">{historyStore.formatDate(selected.timestamp)}</span>
            {#if selected.duration > 0}
              <span class="detail-duration">{historyStore.formatDuration(selected.duration)}</span>
            {/if}
            {#if selected.enhanced}
              <span class="detail-badge">Enhanced</span>
            {/if}
          </div>
          <div class="detail-timestamp">
            {selected.timestamp.toLocaleString('en-AU', {
              weekday: 'long',
              year: 'numeric',
              month: 'long',
              day: 'numeric',
              hour: '2-digit',
              minute: '2-digit',
            })}
          </div>
        </div>
        <div class="detail-content">
          {#if selected.enhanced && selected.rawText}
            <div class="bubble-container">
              <div class="bubble original">
                <div class="bubble-header">
                  <span class="bubble-label">Original</span>
                  <button
                    class="bubble-copy"
                    onclick={() => handleCopy({ ...selected, text: selected.rawText! })}
                    type="button"
                    title="Copy original text"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
                    </svg>
                  </button>
                </div>
                <p class="bubble-text">{selected.rawText}</p>
              </div>
              <div class="bubble enhanced">
                <div class="bubble-header">
                  <span class="bubble-label">Enhanced</span>
                  <button
                    class="bubble-copy"
                    onclick={() => handleCopy(selected)}
                    type="button"
                    title="Copy enhanced text"
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                      <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
                      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
                    </svg>
                  </button>
                </div>
                <p class="bubble-text">{selected.text}</p>
              </div>
            </div>
          {:else}
            <p>{selected.text}</p>
          {/if}
        </div>

        {#if selected.audioPath}
          <div class="audio-section">
            <AudioPlayer audioPath={selected.audioPath} />
          </div>
        {/if}

        {#if showMetadata && hasMetadata(selected)}
          <div class="metadata-panel">
            <div class="metadata-grid">
              {#if selected.transcriptionModelName}
                <div class="metadata-row">
                  <span class="metadata-label">Model</span>
                  <span class="metadata-value mono">{selected.transcriptionModelName}</span>
                </div>
              {/if}
              {#if selected.transcriptionDurationSeconds}
                <div class="metadata-row">
                  <span class="metadata-label">Transcription time</span>
                  <span class="metadata-value">{formatProcessingTime(selected.transcriptionDurationSeconds)}</span>
                </div>
              {/if}
              {#if selected.duration > 0 && selected.transcriptionDurationSeconds}
                <div class="metadata-row">
                  <span class="metadata-label">Speed (RTFX)</span>
                  <span class="metadata-value">{(selected.duration / selected.transcriptionDurationSeconds).toFixed(1)}x</span>
                </div>
              {/if}
              {#if selected.enhancementModelName}
                <div class="metadata-row">
                  <span class="metadata-label">Enhancement model</span>
                  <span class="metadata-value mono">{selected.enhancementModelName}</span>
                </div>
              {/if}
              {#if selected.enhancementDurationSeconds}
                <div class="metadata-row">
                  <span class="metadata-label">Enhancement time</span>
                  <span class="metadata-value">{formatProcessingTime(selected.enhancementDurationSeconds)}</span>
                </div>
              {/if}
              {#if selected.audioPath}
                <div class="metadata-row">
                  <span class="metadata-label">Audio file</span>
                  <span class="metadata-value mono truncate" title={selected.audioPath}>{selected.audioPath.split('/').pop()}</span>
                </div>
              {/if}
              {#if selected.enhancementPrompt}
                <div class="metadata-row">
                  <span class="metadata-label">Prompt</span>
                  <span class="metadata-value truncate" title={selected.enhancementPrompt}>{selected.enhancementPrompt.slice(0, 60)}{selected.enhancementPrompt.length > 60 ? '...' : ''}</span>
                </div>
              {/if}
            </div>
          </div>
        {/if}

        <div class="detail-actions">
          <button class="btn primary" onclick={handleCopySelected} type="button">
            <svg
              class="btn-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
            Copy
          </button>
          {#if hasMetadata(selected)}
            <button
              class="btn"
              class:active={showMetadata}
              onclick={() => (showMetadata = !showMetadata)}
              title="Toggle metadata"
              type="button"
            >
              <svg
                class="btn-icon"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
              >
                <circle cx="12" cy="12" r="10"></circle>
                <line x1="12" y1="16" x2="12" y2="12"></line>
                <line x1="12" y1="8" x2="12.01" y2="8"></line>
              </svg>
              Info
            </button>
          {/if}
          <button class="btn danger" onclick={handleDeleteSelected} type="button">
            <svg
              class="btn-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <polyline points="3 6 5 6 21 6"></polyline>
              <path
                d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"
              ></path>
            </svg>
            Delete
          </button>
        </div>
      {:else}
        <div class="empty-detail">
          <svg
            class="empty-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
          >
            <path
              d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"
            />
          </svg>
          <p>Select a transcription to view details</p>
        </div>
      {/if}
    </main>

    {#if showFilterPanel}
      <HistoryFilterPanel
        bind:filters
        onchange={handleFilterChange}
        onclose={() => (showFilterPanel = false)}
        matchCount={filteredRecords.length}
        totalCount={historyStore.records.length}
      />
    {/if}
  </div>

  {#if historyStore.error}
    <div class="error-toast">
      <span>{historyStore.error}</span>
      <button onclick={() => historyStore.clearError()} aria-label="Dismiss error" type="button">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6L6 18M6 6l12 12"></path>
        </svg>
      </button>
    </div>
  {/if}

  {#if copyFeedback}
    <div class="toast success">
      <svg
        class="toast-icon"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
      >
        <path d="M20 6L9 17l-5-5"></path>
      </svg>
      <span>{copyFeedback}</span>
    </div>
  {/if}

  {#if deleteConfirm}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="modal-overlay" onclick={cancelDelete} role="presentation">
      <dialog class="modal" open aria-labelledby="modal-title">
        <h3 id="modal-title" class="modal-title">Delete Transcription</h3>
        <p class="modal-text">
          Are you sure you want to delete this transcription? This action cannot be undone.
        </p>
        <div class="modal-preview">
          {deleteConfirm.text.slice(0, 100)}{deleteConfirm.text.length > 100 ? '...' : ''}
        </div>
        <div class="modal-actions">
          <button class="btn" onclick={cancelDelete} type="button">Cancel</button>
          <button class="btn danger" onclick={confirmDelete} type="button">Delete</button>
        </div>
      </dialog>
    </div>
  {/if}

  {#if bulkDeleteConfirm}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="modal-overlay" onclick={cancelBulkDelete} role="presentation">
      <dialog class="modal" open aria-labelledby="bulk-delete-title">
        <h3 id="bulk-delete-title" class="modal-title">Delete {bulkSelectedIds.size} Transcriptions</h3>
        <p class="modal-text">
          Are you sure you want to delete {bulkSelectedIds.size} transcription{bulkSelectedIds.size === 1 ? '' : 's'}? This action cannot be undone.
        </p>
        <div class="modal-actions">
          <button class="btn" onclick={cancelBulkDelete} type="button">Cancel</button>
          <button class="btn danger" onclick={confirmBulkDelete} type="button">Delete {bulkSelectedIds.size}</button>
        </div>
      </dialog>
    </div>
  {/if}

  {#if bulkSelectedIds.size > 0}
    <div class="bulk-toolbar">
      <span class="bulk-count">{bulkSelectedIds.size} selected</span>
      <div class="bulk-actions">
        <button class="bulk-btn" onclick={selectAll} type="button" title="Select all">
          Select all
        </button>
        <button class="bulk-btn" onclick={deselectAll} type="button" title="Deselect all">
          Deselect
        </button>
        <button class="bulk-btn" onclick={handleBulkExport} type="button" title="Export selected">
          <svg class="bulk-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
            <polyline points="7 10 12 15 17 10"></polyline>
            <line x1="12" y1="15" x2="12" y2="3"></line>
          </svg>
          Export
        </button>
        <button class="bulk-btn danger" onclick={handleBulkDeleteRequest} type="button" title="Delete selected">
          <svg class="bulk-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="3 6 5 6 21 6"></polyline>
            <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
          </svg>
          Delete
        </button>
      </div>
    </div>
  {/if}

  <ExportDialog bind:open={showExportDialog} selectedIds={bulkExportIds} />
  <PerformanceDialog bind:open={showPerformanceDialog} />
</div>

<style>
  .history-pane {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: var(--color-bg-primary);
    position: relative;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-md) var(--spacing-lg);
    background: var(--color-bg-secondary);
    border-bottom: 1px solid var(--color-border);
  }

  .toolbar-left {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
  }

  .toolbar-right {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
  }

  .count {
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
    white-space: nowrap;
  }

  .clear-filters-btn {
    padding: var(--spacing-xs) var(--spacing-sm);
    font-size: var(--text-xs);
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .clear-filters-btn:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }

  .filter-toggle-btn {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: var(--spacing-sm) var(--spacing-md);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      border-color var(--transition-fast);
    position: relative;
  }

  .filter-toggle-btn:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }

  .filter-toggle-btn.active {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: white;
  }

  .filter-icon {
    width: 14px;
    height: 14px;
  }

  .filter-badge {
    position: absolute;
    top: -2px;
    right: -2px;
    width: 8px;
    height: 8px;
    background: var(--color-accent);
    border-radius: var(--radius-full);
    border: 2px solid var(--color-bg-secondary);
  }

  .filter-toggle-btn.active .filter-badge {
    background: white;
    border-color: var(--color-accent);
  }

  .export-btn {
    flex-shrink: 0;
  }

  .content {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .list-panel {
    width: 320px;
    min-width: 280px;
    border-right: 1px solid var(--color-border);
    overflow: hidden;
  }

  .detail-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  .detail-header {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--color-border-subtle);
    background: var(--color-bg-secondary);
  }

  .detail-meta {
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
  }

  .detail-date {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
  }

  .detail-duration {
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
  }

  .detail-badge {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: var(--color-accent);
    color: white;
    font-weight: 500;
  }

  .detail-timestamp {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .detail-content {
    flex: 1;
    padding: var(--spacing-lg);
    overflow-y: auto;
  }

  .detail-content p {
    color: var(--color-text-primary);
    line-height: 1.7;
    white-space: pre-wrap;
    margin: 0;
  }

  .bubble-container {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-md);
  }

  .bubble {
    position: relative;
    padding: var(--spacing-md);
    border-radius: var(--radius-lg);
    max-width: 90%;
  }

  .bubble.original {
    align-self: flex-start;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border-subtle);
  }

  .bubble.enhanced {
    align-self: flex-end;
    background: color-mix(in srgb, var(--color-accent) 10%, var(--color-bg-primary));
    border: 1px solid color-mix(in srgb, var(--color-accent) 25%, transparent);
  }

  .bubble-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--spacing-xs);
  }

  .bubble-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-tertiary);
  }

  .bubble.enhanced .bubble-label {
    color: var(--color-accent);
  }

  .bubble-copy {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-tertiary);
    cursor: pointer;
    opacity: 0;
    transition:
      opacity var(--transition-fast),
      background var(--transition-fast);
  }

  .bubble:hover .bubble-copy {
    opacity: 1;
  }

  .bubble-copy:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }

  .bubble-copy svg {
    width: 14px;
    height: 14px;
  }

  .bubble-text {
    color: var(--color-text-primary);
    line-height: 1.7;
    white-space: pre-wrap;
    margin: 0;
    font-size: var(--text-sm);
  }

  .audio-section {
    padding: var(--spacing-sm) var(--spacing-lg);
    border-top: 1px solid var(--color-border-subtle);
  }

  .metadata-panel {
    padding: var(--spacing-sm) var(--spacing-lg);
    border-top: 1px solid var(--color-border-subtle);
    background: var(--color-bg-secondary);
  }

  .metadata-grid {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
  }

  .metadata-row {
    display: flex;
    align-items: baseline;
    gap: var(--spacing-md);
  }

  .metadata-label {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    white-space: nowrap;
    flex-shrink: 0;
    min-width: 100px;
  }

  .metadata-value {
    font-size: var(--text-xs);
    color: var(--color-text-primary);
    min-width: 0;
  }

  .metadata-value.mono {
    font-family: var(--font-mono, monospace);
  }

  .metadata-value.truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail-actions {
    display: flex;
    gap: var(--spacing-sm);
    padding: var(--spacing-md) var(--spacing-lg);
    border-top: 1px solid var(--color-border);
    background: var(--color-bg-secondary);
  }

  .btn {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: var(--spacing-sm) var(--spacing-md);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .btn:hover {
    background: var(--color-bg-hover);
  }

  .btn.primary {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: white;
  }

  .btn.primary:hover {
    background: var(--color-accent-hover);
  }

  .btn.danger {
    color: var(--color-error);
    border-color: var(--color-error);
  }

  .btn.danger:hover {
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
  }

  .btn.active {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: white;
  }

  .btn.active:hover {
    background: var(--color-accent-hover);
  }

  .btn-icon {
    width: 16px;
    height: 16px;
  }

  .empty-detail {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--color-text-tertiary);
    text-align: center;
    padding: var(--spacing-xl);
  }

  .empty-icon {
    width: 48px;
    height: 48px;
    margin-bottom: var(--spacing-md);
  }

  .empty-detail p {
    font-size: var(--text-sm);
    margin: 0;
  }

  .toast,
  .error-toast {
    position: absolute;
    bottom: var(--spacing-lg);
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-sm) var(--spacing-md);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    box-shadow: var(--shadow-lg);
    animation: slideUp 0.2s ease;
    z-index: 100;
  }

  .toast.success {
    background: var(--color-success);
    color: white;
  }

  .toast-icon {
    width: 16px;
    height: 16px;
  }

  .error-toast {
    background: var(--color-error);
    color: white;
  }

  .error-toast button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: white;
    cursor: pointer;
    opacity: 0.8;
    transition: opacity var(--transition-fast);
  }

  .error-toast button:hover {
    opacity: 1;
  }

  .error-toast button svg {
    width: 14px;
    height: 14px;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translate(-50%, 10px);
    }
    to {
      opacity: 1;
      transform: translate(-50%, 0);
    }
  }

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
    max-width: 400px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--spacing-lg);
    box-shadow: var(--shadow-lg);
    animation: scaleIn 0.15s ease;
  }

  .modal-title {
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text-primary);
    margin: 0 0 var(--spacing-sm) 0;
  }

  .modal-text {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0 0 var(--spacing-md) 0;
    line-height: 1.5;
  }

  .modal-preview {
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
    background: var(--color-bg-primary);
    padding: var(--spacing-sm) var(--spacing-md);
    border-radius: var(--radius-sm);
    margin-bottom: var(--spacing-lg);
    font-style: italic;
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-sm);
  }

  .bulk-toolbar {
    position: absolute;
    bottom: var(--spacing-lg);
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: var(--spacing-md);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    z-index: 50;
    animation: slideUp 0.2s ease;
  }

  .bulk-count {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
    white-space: nowrap;
  }

  .bulk-actions {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
  }

  .bulk-btn {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: var(--spacing-xs) var(--spacing-sm);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast);
    white-space: nowrap;
  }

  .bulk-btn:hover {
    background: var(--color-bg-hover);
  }

  .bulk-btn.danger {
    color: var(--color-error);
    border-color: var(--color-error);
  }

  .bulk-btn.danger:hover {
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
  }

  .bulk-icon {
    width: 14px;
    height: 14px;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes scaleIn {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }
</style>
