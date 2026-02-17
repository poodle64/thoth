<script lang="ts">
  import type { TranscriptionRecord } from '../stores/history.svelte';
  import { historyStore } from '../stores/history.svelte';

  interface Props {
    item: TranscriptionRecord;
    selected?: boolean;
    bulkSelected?: boolean;
    bulkMode?: boolean;
    onSelect?: (item: TranscriptionRecord) => void;
    onBulkToggle?: (item: TranscriptionRecord) => void;
    onCopy?: (item: TranscriptionRecord) => void;
    onDelete?: (item: TranscriptionRecord) => void;
  }

  let {
    item,
    selected = false,
    bulkSelected = false,
    bulkMode = false,
    onSelect,
    onBulkToggle,
    onCopy,
    onDelete,
  }: Props = $props();

  let showContextMenu = $state(false);
  let contextMenuPosition = $state({ x: 0, y: 0 });

  /** Preview text with truncation */
  const previewText = $derived.by(() => {
    const text = item.text.trim();
    const maxLength = 80;
    if (text.length <= maxLength) {
      return text;
    }
    return text.slice(0, maxLength).trim() + '...';
  });

  /** Handle item click */
  function handleClick(event: MouseEvent) {
    if (event.metaKey || event.ctrlKey) {
      // Cmd/Ctrl+click toggles bulk selection
      event.preventDefault();
      onBulkToggle?.(item);
      return;
    }
    onSelect?.(item);
  }

  /** Handle checkbox click */
  function handleCheckboxClick(event: MouseEvent) {
    event.stopPropagation();
    onBulkToggle?.(item);
  }

  /** Handle keyboard selection */
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onSelect?.(item);
    }
  }

  /** Handle context menu */
  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    contextMenuPosition = { x: event.clientX, y: event.clientY };
    showContextMenu = true;
  }

  /** Close context menu when clicking outside */
  function handleWindowClick() {
    showContextMenu = false;
  }

  /** Handle copy action */
  function handleCopy() {
    showContextMenu = false;
    onCopy?.(item);
  }

  /** Handle delete action */
  function handleDelete() {
    showContextMenu = false;
    onDelete?.(item);
  }
</script>

<svelte:window onclick={handleWindowClick} />

<button
  class="history-item"
  class:selected
  class:bulk-selected={bulkSelected}
  class:bulk-mode={bulkMode}
  onclick={handleClick}
  onkeydown={handleKeydown}
  oncontextmenu={handleContextMenu}
  aria-selected={selected}
  role="option"
>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="checkbox-area"
    onclick={handleCheckboxClick}
  >
    <div class="checkbox" class:checked={bulkSelected}>
      {#if bulkSelected}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3">
          <polyline points="20 6 9 17 4 12"></polyline>
        </svg>
      {/if}
    </div>
  </div>
  <div class="item-content">
    <span class="item-text">{previewText}</span>
    <div class="item-meta">
      <span class="item-date">{historyStore.formatDate(item.timestamp)}</span>
      {#if item.duration > 0}
        <span class="item-duration">{historyStore.formatDuration(item.duration)}</span>
      {/if}
      {#if item.enhanced}
        <span class="item-badge">Enhanced</span>
      {/if}
    </div>
  </div>
</button>

{#if showContextMenu}
  <div
    class="context-menu"
    style:left="{contextMenuPosition.x}px"
    style:top="{contextMenuPosition.y}px"
    role="menu"
  >
    <button class="context-menu-item" onclick={handleCopy} role="menuitem">
      <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
      </svg>
      Copy
    </button>
    <button class="context-menu-item danger" onclick={handleDelete} role="menuitem">
      <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <polyline points="3 6 5 6 21 6"></polyline>
        <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"
        ></path>
      </svg>
      Delete
    </button>
  </div>
{/if}

<style>
  .history-item {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    width: 100%;
    padding: var(--spacing-md);
    border: none;
    border-bottom: 1px solid var(--color-border-subtle);
    background: transparent;
    cursor: pointer;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .history-item:hover {
    background: var(--color-bg-hover);
  }

  .history-item:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: -2px;
  }

  .history-item.selected {
    background: var(--color-accent);
  }

  .history-item.selected .item-text,
  .history-item.selected .item-date,
  .history-item.selected .item-duration {
    color: white;
  }

  .history-item.selected .item-badge {
    background: rgba(255, 255, 255, 0.2);
    color: white;
  }

  /* Checkbox area */
  .checkbox-area {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 0;
    overflow: hidden;
    flex-shrink: 0;
    opacity: 0;
    transition:
      width 0.15s ease,
      opacity 0.15s ease,
      margin 0.15s ease;
  }

  .history-item:hover .checkbox-area,
  .history-item.bulk-mode .checkbox-area {
    width: 24px;
    opacity: 1;
    margin-right: var(--spacing-sm);
  }

  .checkbox {
    width: 16px;
    height: 16px;
    border: 2px solid var(--color-border);
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition:
      background var(--transition-fast),
      border-color var(--transition-fast);
  }

  .checkbox.checked {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }

  .checkbox svg {
    width: 12px;
    height: 12px;
    color: white;
  }

  .history-item.selected .checkbox {
    border-color: rgba(255, 255, 255, 0.5);
  }

  .history-item.selected .checkbox.checked {
    background: white;
    border-color: white;
  }

  .history-item.selected .checkbox.checked svg {
    color: var(--color-accent);
  }

  .history-item.bulk-selected {
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
  }

  .item-content {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-xs);
    width: 100%;
    min-width: 0;
  }

  .item-text {
    font-size: var(--text-sm);
    color: var(--color-text-primary);
    line-height: 1.4;
    word-break: break-word;
  }

  .item-meta {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    flex-wrap: wrap;
  }

  .item-date {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .item-duration {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
  }

  .item-badge {
    font-size: 10px;
    padding: 2px 6px;
    border-radius: var(--radius-sm);
    background: var(--color-accent);
    color: white;
    font-weight: 500;
  }

  /* Context menu */
  .context-menu {
    position: fixed;
    z-index: 1000;
    min-width: 160px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg);
    padding: var(--spacing-xs);
  }

  .context-menu-item {
    display: flex;
    align-items: center;
    gap: var(--spacing-sm);
    width: 100%;
    padding: var(--spacing-sm) var(--spacing-md);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-primary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .context-menu-item:hover {
    background: var(--color-bg-hover);
  }

  .context-menu-item.danger {
    color: var(--color-error);
  }

  .context-menu-item.danger:hover {
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
  }

  .icon {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }
</style>
