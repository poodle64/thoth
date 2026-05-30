<script lang="ts">
  /**
   * History Filter Panel - Right pane filter/search controls for the History window.
   *
   * Provides advanced filtering options including date range, duration, and
   * enhanced status filtering for transcription records.
   */

  import { Button } from '$components/ui/button';
  import { Input } from '$components/ui/input';
  import { Label } from '$components/ui/label';
  import { Separator } from '$components/ui/separator';
  import Search from '@lucide/svelte/icons/search';
  import X from '@lucide/svelte/icons/x';
  import Star from '@lucide/svelte/icons/star';
  import Circle from '@lucide/svelte/icons/circle';
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

  let localFilters = $state<FilterState>({ ...filters });

  $effect(() => {
    localFilters = { ...filters };
  });

  function updateFilter<K extends keyof FilterState>(key: K, value: FilterState[K]) {
    localFilters = { ...localFilters, [key]: value };
    onchange?.(localFilters);
  }

  function handleDateChange(from: string, to: string) {
    localFilters = { ...localFilters, fromDate: from, toDate: to };
    onchange?.(localFilters);
  }

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

  const hasActiveFilters = $derived(
    localFilters.searchQuery !== '' ||
      localFilters.fromDate !== '' ||
      localFilters.toDate !== '' ||
      localFilters.minDuration !== null ||
      localFilters.maxDuration !== null ||
      localFilters.showEnhancedOnly ||
      localFilters.showUnenhancedOnly
  );

  function toggleEnhancedOnly() {
    if (localFilters.showEnhancedOnly) {
      updateFilter('showEnhancedOnly', false);
    } else {
      localFilters = { ...localFilters, showEnhancedOnly: true, showUnenhancedOnly: false };
      onchange?.(localFilters);
    }
  }

  function toggleUnenhancedOnly() {
    if (localFilters.showUnenhancedOnly) {
      updateFilter('showUnenhancedOnly', false);
    } else {
      localFilters = { ...localFilters, showUnenhancedOnly: true, showEnhancedOnly: false };
      onchange?.(localFilters);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      onclose?.();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<aside class="flex h-full w-[280px] min-w-[240px] flex-col border-l bg-muted/30">
  <header class="flex items-center justify-between border-b bg-muted/50 px-4 py-3">
    <h3 class="text-sm font-semibold">Filters</h3>
    <Button
      variant="ghost"
      size="icon"
      onclick={onclose}
      aria-label="Close filter panel"
      class="h-6 w-6"
    >
      <X class="size-4" />
    </Button>
  </header>

  <div class="flex-1 overflow-y-auto space-y-5 p-4">
    <!-- Search -->
    <div class="space-y-2">
      <Label for="filter-search" class="text-xs font-medium text-muted-foreground">Search</Label>
      <div class="relative">
        <Search
          class="absolute left-2.5 top-2.5 size-3.5 text-muted-foreground pointer-events-none"
        />
        <Input
          type="search"
          id="filter-search"
          placeholder="Search transcriptions..."
          value={localFilters.searchQuery}
          oninput={handleSearchInput}
          class="pl-8 h-8 text-sm"
        />
      </div>
    </div>

    <Separator />

    <!-- Date Range -->
    <div class="space-y-2">
      <span class="text-xs font-medium text-muted-foreground">Date Range</span>
      <DateRangePicker
        fromDate={localFilters.fromDate}
        toDate={localFilters.toDate}
        onchange={handleDateChange}
      />
    </div>

    <Separator />

    <!-- Duration -->
    <div class="space-y-2">
      <span class="text-xs font-medium text-muted-foreground">Duration</span>
      <div class="flex items-end gap-2">
        <div class="flex flex-1 flex-col gap-1">
          <Label for="min-duration" class="text-xs text-muted-foreground">Min (s)</Label>
          <Input
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
            class="h-8 text-sm"
          />
        </div>
        <span class="pb-1.5 text-muted-foreground text-sm">-</span>
        <div class="flex flex-1 flex-col gap-1">
          <Label for="max-duration" class="text-xs text-muted-foreground">Max (s)</Label>
          <Input
            type="number"
            id="max-duration"
            min="0"
            step="1"
            placeholder="∞"
            value={localFilters.maxDuration ?? ''}
            oninput={(e) => {
              const val = e.currentTarget.value;
              updateFilter('maxDuration', val ? parseInt(val, 10) : null);
            }}
            class="h-8 text-sm"
          />
        </div>
      </div>
    </div>

    <Separator />

    <!-- Enhancement Status -->
    <div class="space-y-2">
      <span class="text-xs font-medium text-muted-foreground">Enhancement Status</span>
      <div class="flex gap-2">
        <Button
          variant={localFilters.showEnhancedOnly ? 'default' : 'outline'}
          size="sm"
          onclick={toggleEnhancedOnly}
          type="button"
          class="flex-1 gap-1.5 h-8"
        >
          <Star class="size-3.5" />
          Enhanced
        </Button>
        <Button
          variant={localFilters.showUnenhancedOnly ? 'default' : 'outline'}
          size="sm"
          onclick={toggleUnenhancedOnly}
          type="button"
          class="flex-1 gap-1.5 h-8"
        >
          <Circle class="size-3.5" />
          Original
        </Button>
      </div>
    </div>
  </div>

  <footer class="flex items-center justify-between border-t bg-muted/50 px-4 py-3">
    <div class="text-sm text-muted-foreground">
      {#if hasActiveFilters}
        <span class="font-medium text-primary">{matchCount}</span> of {totalCount}
      {:else}
        {totalCount} total
      {/if}
    </div>
    {#if hasActiveFilters}
      <Button variant="ghost" size="sm" onclick={clearAllFilters} type="button" class="h-7 text-xs">
        Clear all
      </Button>
    {/if}
  </footer>
</aside>
