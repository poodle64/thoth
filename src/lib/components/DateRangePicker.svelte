<script lang="ts">
  /**
   * Date range picker component for filtering by date range.
   * Supports preset ranges and custom date selection.
   */

  interface Props {
    /** Start date in YYYY-MM-DD format */
    fromDate?: string;
    /** End date in YYYY-MM-DD format */
    toDate?: string;
    /** Callback when date range changes */
    onchange?: (from: string, to: string) => void;
    /** Whether the picker is disabled */
    disabled?: boolean;
  }

  let {
    fromDate = $bindable(''),
    toDate = $bindable(''),
    onchange,
    disabled = false,
  }: Props = $props();

  type PresetRange =
    | 'today'
    | 'yesterday'
    | 'last7days'
    | 'last30days'
    | 'thisMonth'
    | 'lastMonth'
    | 'custom';

  let selectedPreset = $state<PresetRange>('custom');

  // Preset date ranges
  const presets: { value: PresetRange; label: string }[] = [
    { value: 'today', label: 'Today' },
    { value: 'yesterday', label: 'Yesterday' },
    { value: 'last7days', label: 'Last 7 days' },
    { value: 'last30days', label: 'Last 30 days' },
    { value: 'thisMonth', label: 'This month' },
    { value: 'lastMonth', label: 'Last month' },
    { value: 'custom', label: 'Custom range' },
  ];

  function formatDate(date: Date): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
  }

  function getPresetDates(preset: PresetRange): { from: string; to: string } {
    const now = new Date();
    const today = formatDate(now);

    switch (preset) {
      case 'today':
        return { from: today, to: today };

      case 'yesterday': {
        const yesterday = new Date(now);
        yesterday.setDate(yesterday.getDate() - 1);
        const date = formatDate(yesterday);
        return { from: date, to: date };
      }

      case 'last7days': {
        const weekAgo = new Date(now);
        weekAgo.setDate(weekAgo.getDate() - 6);
        return { from: formatDate(weekAgo), to: today };
      }

      case 'last30days': {
        const monthAgo = new Date(now);
        monthAgo.setDate(monthAgo.getDate() - 29);
        return { from: formatDate(monthAgo), to: today };
      }

      case 'thisMonth': {
        const firstDay = new Date(now.getFullYear(), now.getMonth(), 1);
        return { from: formatDate(firstDay), to: today };
      }

      case 'lastMonth': {
        const firstDay = new Date(now.getFullYear(), now.getMonth() - 1, 1);
        const lastDay = new Date(now.getFullYear(), now.getMonth(), 0);
        return { from: formatDate(firstDay), to: formatDate(lastDay) };
      }

      case 'custom':
        return { from: fromDate, to: toDate };
    }
  }

  function handlePresetChange(preset: PresetRange) {
    selectedPreset = preset;

    if (preset !== 'custom') {
      const dates = getPresetDates(preset);
      fromDate = dates.from;
      toDate = dates.to;
      onchange?.(fromDate, toDate);
    }
  }

  function handleDateChange() {
    selectedPreset = 'custom';
    onchange?.(fromDate, toDate);
  }

  function handleClear() {
    fromDate = '';
    toDate = '';
    selectedPreset = 'custom';
    onchange?.('', '');
  }

  // Check if dates match any preset
  $effect(() => {
    if (fromDate && toDate) {
      for (const preset of presets) {
        if (preset.value !== 'custom') {
          const dates = getPresetDates(preset.value);
          if (dates.from === fromDate && dates.to === toDate) {
            selectedPreset = preset.value;
            return;
          }
        }
      }
      selectedPreset = 'custom';
    }
  });
</script>

<div class="date-range-picker" class:disabled>
  <div class="presets">
    {#each presets as preset}
      <button
        class="preset-btn"
        class:active={selectedPreset === preset.value}
        onclick={() => handlePresetChange(preset.value)}
        {disabled}
        type="button"
      >
        {preset.label}
      </button>
    {/each}
  </div>

  <div class="date-inputs">
    <div class="date-field">
      <label for="from-date">From</label>
      <input
        type="date"
        id="from-date"
        bind:value={fromDate}
        onchange={handleDateChange}
        {disabled}
      />
    </div>
    <span class="date-separator">-</span>
    <div class="date-field">
      <label for="to-date">To</label>
      <input type="date" id="to-date" bind:value={toDate} onchange={handleDateChange} {disabled} />
    </div>
    {#if fromDate || toDate}
      <button
        class="clear-btn"
        onclick={handleClear}
        {disabled}
        type="button"
        aria-label="Clear date range"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
          <path
            d="M4.646 4.646a.5.5 0 0 1 .708 0L8 7.293l2.646-2.647a.5.5 0 0 1 .708.708L8.707 8l2.647 2.646a.5.5 0 0 1-.708.708L8 8.707l-2.646 2.647a.5.5 0 0 1-.708-.708L7.293 8 4.646 5.354a.5.5 0 0 1 0-.708z"
          />
        </svg>
      </button>
    {/if}
  </div>
</div>

<style>
  .date-range-picker {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .date-range-picker.disabled {
    opacity: 0.6;
    pointer-events: none;
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .preset-btn {
    padding: 6px 12px;
    border: 1px solid var(--color-border);
    border-radius: 16px;
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    font-size: 12px;
    cursor: pointer;
    transition:
      background 0.15s ease,
      border-color 0.15s ease,
      color 0.15s ease;
  }

  .preset-btn:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
  }

  .preset-btn.active {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: white;
  }

  .preset-btn:disabled {
    cursor: not-allowed;
  }

  .date-inputs {
    display: flex;
    align-items: flex-end;
    gap: 8px;
  }

  .date-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
  }

  .date-field label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text-tertiary);
  }

  .date-field input[type='date'] {
    padding: 8px 10px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: var(--color-bg-secondary);
    color: var(--color-text-primary);
    font-size: 13px;
    width: 100%;
  }

  .date-field input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: 0 0 0 2px rgba(var(--color-accent-rgb), 0.2);
  }

  .date-field input:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .date-separator {
    color: var(--color-text-tertiary);
    padding-bottom: 8px;
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    border-radius: 6px;
    background: var(--color-bg-secondary);
    color: var(--color-text-tertiary);
    cursor: pointer;
    transition:
      background 0.15s ease,
      color 0.15s ease;
    margin-bottom: 1px;
  }

  .clear-btn:hover:not(:disabled) {
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
  }

  .clear-btn:disabled {
    cursor: not-allowed;
  }
</style>
