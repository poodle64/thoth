<script lang="ts">
  /**
   * History Filter Panel - Right pane filter/search controls for the History window.
   *
   * Provides advanced filtering options including date range, duration, and
   * enhanced status filtering for transcription records.
   */

  import DateRangePicker from './DateRangePicker.svelte';

  /** Filter state structure */
  export interface FilterState {
    searchQuery: string;
    fromDate: string;
    toDate: string;
    minDuration: number | null;
    maxDuration: number | null;
    showEnhancedOnly: boolean;
    showUnenhancedOnly: boolean;
  }

  interface Props {
    /** Current filter state */
    filters: FilterState;
    /** Callback when filters change */
    onchange?: (filters: FilterState) => void;
    /** Callback to close the panel */
    onclose?: () => void;
    /** Total count of records matching filters */
    matchCount?: number;
    /** Total count of all records */
    totalCount?: number;
  }

  let {
    filters = $bindable(),
    onchange,
    onclose,
    matchCount = 0,
    totalCount = 0,
  }: Props = $props();

  /** Local filter state for immediate UI updates */
  let localFilters = $state<FilterState>({ ...filters });

  /** Sync local state when props change */
  $effect(() => {
    localFilters = { ...filters };
  });

  /** Update a single filter value */
  function updateFilter<K extends keyof FilterState>(key: K, value: FilterState[K]) {
    localFilters = { ...localFilters, [key]: value };
    onchange?.(localFilters);
  }

  /** Handle date range change */
  function handleDateChange(from: string, to: string) {
    localFilters = { ...localFilters, fromDate: from, toDate: to };
    onchange?.(localFilters);
  }

  /** Handle search input with debouncing */
  let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;
  function handleSearchInput(event: Event) {
    const target = event.target as HTMLInputElement;
    localFilters.searchQuery = target.value;

    if (searchDebounceTimer) {
      clearTimeout(searchDebounceTimer);
    }

    searchDebounceTimer = setTimeout(() => {
      onchange?.(localFilters);
      searchDebounceTimer = null;
    }, 300);
  }

  /** Clear all filters */
  function clearAllFilters() {
    const clearedFilters: FilterState = {
      searchQuery: '',
      fromDate: '',
      toDate: '',
      minDuration: null,
      maxDuration: null,
      showEnhancedOnly: false,
      showUnenhancedOnly: false,
    };
    localFilters = clearedFilters;
    onchange?.(clearedFilters);
  }

  /** Check if any filters are active */
  const hasActiveFilters = $derived(
    localFilters.searchQuery !== '' ||
      localFilters.fromDate !== '' ||
      localFilters.toDate !== '' ||
      localFilters.minDuration !== null ||
      localFilters.maxDuration !== null ||
      localFilters.showEnhancedOnly ||
      localFilters.showUnenhancedOnly
  );

  /** Handle enhanced filter toggle */
  function toggleEnhancedOnly() {
    if (localFilters.showEnhancedOnly) {
      updateFilter('showEnhancedOnly', false);
    } else {
      localFilters = { ...localFilters, showEnhancedOnly: true, showUnenhancedOnly: false };
      onchange?.(localFilters);
    }
  }

  /** Handle unenhanced filter toggle */
  function toggleUnenhancedOnly() {
    if (localFilters.showUnenhancedOnly) {
      updateFilter('showUnenhancedOnly', false);
    } else {
      localFilters = { ...localFilters, showUnenhancedOnly: true, showEnhancedOnly: false };
      onchange?.(localFilters);
    }
  }

  /** Handle keyboard events */
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      onclose?.();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="filter-panel">
  <header class="panel-header">
    <h3 class="panel-title">Filters</h3>
    <button class="close-btn" onclick={onclose} aria-label="Close filter panel">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M18 6L6 18M6 6l12 12"></path>
      </svg>
    </button>
  </header>

  <div class="panel-content">
    <!-- Search -->
    <div class="filter-section">
      <label class="section-label" for="filter-search">Search</label>
      <div class="search-wrapper">
        <svg
          class="search-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <circle cx="11" cy="11" r="8"></circle>
          <path d="m21 21-4.35-4.35"></path>
        </svg>
        <input
          type="search"
          id="filter-search"
          class="search-input"
          placeholder="Search transcriptions..."
          value={localFilters.searchQuery}
          oninput={handleSearchInput}
        />
      </div>
    </div>

    <!-- Date Range -->
    <div class="filter-section">
      <span class="section-label">Date Range</span>
      <DateRangePicker
        fromDate={localFilters.fromDate}
        toDate={localFilters.toDate}
        onchange={handleDateChange}
      />
    </div>

    <!-- Duration -->
    <div class="filter-section">
      <span class="section-label">Duration</span>
      <div class="duration-inputs">
        <div class="duration-field">
          <label for="min-duration">Min (seconds)</label>
          <input
            type="number"
            id="min-duration"
            min="0"
            step="1"
            placeholder="0"
            value={localFilters.minDuration ?? ''}
            oninput={(e) => {
              const val = e.currentTarget.value;
              updateFilter('minDuration', val ? parseInt(val, 10) : null);
            }}
          />
        </div>
        <span class="duration-separator">-</span>
        <div class="duration-field">
          <label for="max-duration">Max (seconds)</label>
          <input
            type="number"
            id="max-duration"
            min="0"
            step="1"
            placeholder="No limit"
            value={localFilters.maxDuration ?? ''}
            oninput={(e) => {
              const val = e.currentTarget.value;
              updateFilter('maxDuration', val ? parseInt(val, 10) : null);
            }}
          />
        </div>
      </div>
    </div>

    <!-- Enhancement Status -->
    <div class="filter-section">
      <span class="section-label">Enhancement Status</span>
      <div class="status-filters">
        <button
          class="status-btn"
          class:active={localFilters.showEnhancedOnly}
          onclick={toggleEnhancedOnly}
          type="button"
        >
          <svg
            class="status-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <path
              d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"
            ></path>
          </svg>
          Enhanced
        </button>
        <button
          class="status-btn"
          class:active={localFilters.showUnenhancedOnly}
          onclick={toggleUnenhancedOnly}
          type="button"
        >
          <svg
            class="status-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <circle cx="12" cy="12" r="10"></circle>
          </svg>
          Original
        </button>
      </div>
    </div>
  </div>

  <footer class="panel-footer">
    <div class="filter-stats">
      {#if hasActiveFilters}
        <span class="match-count">{matchCount} of {totalCount}</span>
      {:else}
        <span class="total-count">{totalCount} total</span>
      {/if}
    </div>
    {#if hasActiveFilters}
      <button class="clear-btn" onclick={clearAllFilters} type="button"> Clear all filters </button>
    {/if}
  </footer>
</aside>

<style>
  .filter-panel {
    display: flex;
    flex-direction: column;
    width: 280px;
    min-width: 240px;
    height: 100%;
    background: var(--color-bg-secondary);
    border-left: 1px solid var(--color-border);
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-md) var(--spacing-lg);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-bg-tertiary);
  }

  .panel-title {
    margin: 0;
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .close-btn {
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
    transition:
      color var(--transition-fast),
      background var(--transition-fast);
  }

  .close-btn:hover {
    color: var(--color-text-primary);
    background: var(--color-bg-hover);
  }

  .close-btn svg {
    width: 16px;
    height: 16px;
  }

  .panel-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-md) var(--spacing-lg);
  }

  .filter-section {
    margin-bottom: var(--spacing-lg);
  }

  .filter-section:last-child {
    margin-bottom: 0;
  }

  .section-label {
    display: block;
    margin-bottom: var(--spacing-sm);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  /* Search */
  .search-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: var(--spacing-sm);
    width: 14px;
    height: 14px;
    color: var(--color-text-tertiary);
    pointer-events: none;
  }

  .search-input {
    flex: 1;
    padding: var(--spacing-sm);
    padding-left: calc(var(--spacing-sm) + 20px);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-primary);
    color: var(--color-text-primary);
    font-size: var(--text-sm);
  }

  .search-input::placeholder {
    color: var(--color-text-tertiary);
  }

  .search-input:focus {
    border-color: var(--color-accent);
    outline: none;
  }

  /* Duration */
  .duration-inputs {
    display: flex;
    align-items: flex-end;
    gap: var(--spacing-sm);
  }

  .duration-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  .duration-field label {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .duration-field input {
    padding: var(--spacing-sm);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-primary);
    color: var(--color-text-primary);
    font-size: var(--text-sm);
    width: 100%;
  }

  .duration-field input:focus {
    border-color: var(--color-accent);
    outline: none;
  }

  .duration-separator {
    color: var(--color-text-tertiary);
    padding-bottom: var(--spacing-sm);
  }

  /* Status filters */
  .status-filters {
    display: flex;
    gap: var(--spacing-sm);
  }

  .status-btn {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    flex: 1;
    padding: var(--spacing-sm) var(--spacing-md);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-primary);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition:
      background var(--transition-fast),
      border-color var(--transition-fast),
      color var(--transition-fast);
  }

  .status-btn:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }

  .status-btn.active {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: white;
  }

  .status-icon {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }

  /* Footer */
  .panel-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-md) var(--spacing-lg);
    border-top: 1px solid var(--color-border);
    background: var(--color-bg-tertiary);
  }

  .filter-stats {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .match-count {
    color: var(--color-accent);
    font-weight: 500;
  }

  .total-count {
    color: var(--color-text-tertiary);
  }

  .clear-btn {
    padding: var(--spacing-xs) var(--spacing-sm);
    font-size: var(--text-xs);
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-secondary);
  }

  .clear-btn:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }
</style>
