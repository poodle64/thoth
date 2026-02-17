<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { TranscriptionRecord } from '../stores/history.svelte';
  import HistoryItem from './HistoryItem.svelte';

  interface Props {
    items: TranscriptionRecord[];
    selectedId?: string | null;
    bulkSelectedIds?: Set<string>;
    onSelect?: (item: TranscriptionRecord) => void;
    onBulkToggle?: (item: TranscriptionRecord) => void;
    onCopy?: (item: TranscriptionRecord) => void;
    onDelete?: (item: TranscriptionRecord) => void;
    onLoadMore?: () => void;
    isLoading?: boolean;
    hasMore?: boolean;
    emptyState?: Snippet;
  }

  let {
    items,
    selectedId = null,
    bulkSelectedIds = new Set(),
    onSelect,
    onBulkToggle,
    onCopy,
    onDelete,
    onLoadMore,
    isLoading = false,
    hasMore = false,
    emptyState,
  }: Props = $props();

  const bulkMode = $derived(bulkSelectedIds.size > 0);

  let listContainer: HTMLDivElement;
  let visibleItems = $state<TranscriptionRecord[]>([]);
  let startIndex = $state(0);
  let endIndex = $state(0);

  // Virtual scrolling configuration
  const ITEM_HEIGHT = 72; // Approximate height of each item in pixels
  const BUFFER_SIZE = 5; // Number of items to render above/below viewport
  const SCROLL_THRESHOLD = 200; // Pixels from bottom to trigger load more

  /** Calculate visible items based on scroll position */
  function calculateVisibleItems() {
    if (!listContainer) return;

    const scrollTop = listContainer.scrollTop;
    const containerHeight = listContainer.clientHeight;

    // Calculate which items should be visible
    const newStartIndex = Math.max(0, Math.floor(scrollTop / ITEM_HEIGHT) - BUFFER_SIZE);
    const visibleCount = Math.ceil(containerHeight / ITEM_HEIGHT) + BUFFER_SIZE * 2;
    const newEndIndex = Math.min(items.length, newStartIndex + visibleCount);

    startIndex = newStartIndex;
    endIndex = newEndIndex;
    visibleItems = items.slice(startIndex, endIndex);

    // Check if we need to load more
    const scrollBottom = listContainer.scrollHeight - scrollTop - containerHeight;
    if (scrollBottom < SCROLL_THRESHOLD && hasMore && !isLoading) {
      onLoadMore?.();
    }
  }

  /** Handle scroll events with throttling */
  let scrollRAF: number | null = null;
  function handleScroll() {
    if (scrollRAF) return;
    scrollRAF = requestAnimationFrame(() => {
      calculateVisibleItems();
      scrollRAF = null;
    });
  }

  /** Recalculate on items change */
  $effect(() => {
    if (items.length > 0) {
      calculateVisibleItems();
    }
  });

  /** Total height of all items for proper scrollbar */
  const totalHeight = $derived(items.length * ITEM_HEIGHT);

  /** Offset to position visible items correctly */
  const offsetY = $derived(startIndex * ITEM_HEIGHT);

  /** Handle keyboard navigation */
  function handleKeydown(event: KeyboardEvent) {
    if (!items.length) return;

    const currentIndex = selectedId ? items.findIndex((i) => i.id === selectedId) : -1;

    if (event.key === 'ArrowDown') {
      event.preventDefault();
      const nextIndex = Math.min(currentIndex + 1, items.length - 1);
      onSelect?.(items[nextIndex]);
      scrollToIndex(nextIndex);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      const prevIndex = Math.max(currentIndex - 1, 0);
      onSelect?.(items[prevIndex]);
      scrollToIndex(prevIndex);
    } else if (event.key === 'Home') {
      event.preventDefault();
      onSelect?.(items[0]);
      scrollToIndex(0);
    } else if (event.key === 'End') {
      event.preventDefault();
      onSelect?.(items[items.length - 1]);
      scrollToIndex(items.length - 1);
    }
  }

  /** Scroll to ensure an item is visible */
  function scrollToIndex(index: number) {
    if (!listContainer) return;

    const itemTop = index * ITEM_HEIGHT;
    const itemBottom = itemTop + ITEM_HEIGHT;
    const containerTop = listContainer.scrollTop;
    const containerBottom = containerTop + listContainer.clientHeight;

    if (itemTop < containerTop) {
      listContainer.scrollTop = itemTop;
    } else if (itemBottom > containerBottom) {
      listContainer.scrollTop = itemBottom - listContainer.clientHeight;
    }
  }
</script>

<div
  class="history-list"
  bind:this={listContainer}
  onscroll={handleScroll}
  onkeydown={handleKeydown}
  role="listbox"
  aria-label="Transcription history"
  tabindex="0"
>
  {#if items.length === 0}
    {#if emptyState}
      {@render emptyState()}
    {:else}
      <div class="empty-state">
        <svg
          class="empty-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.5"
        >
          <path d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
        </svg>
        <p class="empty-title">No transcriptions yet</p>
        <p class="empty-hint">Start recording to create your first transcription.</p>
      </div>
    {/if}
  {:else}
    <div class="virtual-scroller" style:height="{totalHeight}px">
      <div class="visible-items" style:transform="translateY({offsetY}px)">
        {#each visibleItems as item (item.id)}
          <HistoryItem
            {item}
            selected={selectedId === item.id}
            bulkSelected={bulkSelectedIds.has(item.id)}
            {bulkMode}
            {onSelect}
            {onBulkToggle}
            {onCopy}
            {onDelete}
          />
        {/each}
      </div>
    </div>

    {#if isLoading}
      <div class="loading-indicator">
        <div class="spinner"></div>
        <span>Loading more...</span>
      </div>
    {/if}
  {/if}
</div>

<style>
  .history-list {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
    background: var(--color-bg-secondary);
  }

  .history-list:focus-visible {
    outline: none;
  }

  .virtual-scroller {
    position: relative;
  }

  .visible-items {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: var(--spacing-xl);
    text-align: center;
  }

  .empty-icon {
    width: 48px;
    height: 48px;
    color: var(--color-text-tertiary);
    margin-bottom: var(--spacing-md);
  }

  .empty-title {
    font-size: var(--text-base);
    color: var(--color-text-secondary);
    margin: 0 0 var(--spacing-xs) 0;
  }

  .empty-hint {
    font-size: var(--text-sm);
    color: var(--color-text-tertiary);
    margin: 0;
  }

  .loading-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--spacing-sm);
    padding: var(--spacing-md);
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
