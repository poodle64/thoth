<script lang="ts">
  /**
   * Date range picker component for filtering by date range.
   * Supports preset ranges and custom date selection.
   */

  import { Button } from '$components/ui/button';
  import { Input } from '$components/ui/input';
  import { Label } from '$components/ui/label';
  import X from '@lucide/svelte/icons/x';

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

<div class="flex flex-col gap-3" class:opacity-60={disabled} class:pointer-events-none={disabled}>
  <div class="flex flex-wrap gap-1.5">
    {#each presets as preset}
      <Button
        variant={selectedPreset === preset.value ? 'default' : 'outline'}
        size="sm"
        onclick={() => handlePresetChange(preset.value)}
        {disabled}
        type="button"
        class="h-7 rounded-full text-xs"
      >
        {preset.label}
      </Button>
    {/each}
  </div>

  <div class="flex items-end gap-2">
    <div class="flex flex-1 flex-col gap-1">
      <Label for="from-date" class="text-xs">From</Label>
      <Input
        type="date"
        id="from-date"
        bind:value={fromDate}
        onchange={handleDateChange}
        {disabled}
        class="h-8 text-sm"
      />
    </div>
    <span class="pb-1.5 text-muted-foreground">-</span>
    <div class="flex flex-1 flex-col gap-1">
      <Label for="to-date" class="text-xs">To</Label>
      <Input
        type="date"
        id="to-date"
        bind:value={toDate}
        onchange={handleDateChange}
        {disabled}
        class="h-8 text-sm"
      />
    </div>
    {#if fromDate || toDate}
      <Button
        variant="ghost"
        size="icon"
        onclick={handleClear}
        {disabled}
        type="button"
        aria-label="Clear date range"
        class="h-8 w-8 shrink-0"
      >
        <X class="size-3.5" />
      </Button>
    {/if}
  </div>
</div>
