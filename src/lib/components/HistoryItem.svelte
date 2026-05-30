<script lang="ts">
  import type { TranscriptionRecord } from '../stores/history.svelte';
  import { historyStore } from '../stores/history.svelte';
  import { Checkbox } from '$components/ui/checkbox';
  import { Badge } from '$components/ui/badge';
  import * as DropdownMenu from '$components/ui/dropdown-menu';
  import Copy from '@lucide/svelte/icons/copy';
  import Trash2 from '@lucide/svelte/icons/trash-2';

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

  let contextMenuOpen = $state(false);

  const previewText = $derived.by(() => {
    const text = item.text.trim();
    const maxLength = 80;
    if (text.length <= maxLength) {
      return text;
    }
    return text.slice(0, maxLength).trim() + '...';
  });

  function handleClick(event: MouseEvent) {
    if (event.metaKey || event.ctrlKey) {
      event.preventDefault();
      onBulkToggle?.(item);
      return;
    }
    onSelect?.(item);
  }

  function handleCheckboxChange() {
    onBulkToggle?.(item);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      onSelect?.(item);
    }
  }

  function handleCopy() {
    contextMenuOpen = false;
    onCopy?.(item);
  }

  function handleDelete() {
    contextMenuOpen = false;
    onDelete?.(item);
  }
</script>

<DropdownMenu.Root bind:open={contextMenuOpen}>
  <DropdownMenu.Trigger
    class="w-full text-left"
    oncontextmenu={(e) => {
      e.preventDefault();
      contextMenuOpen = true;
    }}
  >
    <button
      class={[
        'flex w-full flex-row items-start border-b px-3 py-3 text-left transition-colors hover:bg-accent/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset',
        selected && 'bg-primary text-primary-foreground',
        bulkSelected && !selected && 'bg-primary/10',
      ]
        .filter(Boolean)
        .join(' ')}
      onclick={handleClick}
      onkeydown={handleKeydown}
      aria-selected={selected}
      role="option"
    >
      <!-- Checkbox area -->
      <div
        class="mr-3 flex shrink-0 items-center justify-center transition-opacity"
        class:opacity-0={!bulkMode && !bulkSelected}
        class:opacity-100={bulkMode || bulkSelected}
      >
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          onclick={(e) => {
            e.stopPropagation();
            handleCheckboxChange();
          }}
        >
          <Checkbox
            checked={bulkSelected}
            class={selected
              ? 'border-primary-foreground/50 data-checked:bg-primary-foreground data-checked:text-primary'
              : ''}
          />
        </div>
      </div>

      <!-- Content -->
      <div class="flex min-w-0 flex-1 flex-col gap-1">
        <span
          class="line-clamp-2 break-words text-sm leading-snug"
          class:text-foreground={!selected}
          class:text-primary-foreground={selected}>{previewText}</span
        >
        <div class="flex flex-wrap items-center gap-2">
          <span
            class={[
              'text-xs',
              selected ? 'text-primary-foreground/70' : 'text-muted-foreground',
            ].join(' ')}>{historyStore.formatDate(item.timestamp)}</span
          >
          {#if item.duration > 0}
            <span
              class={[
                'text-xs',
                selected ? 'text-primary-foreground/70' : 'text-muted-foreground',
              ].join(' ')}>{historyStore.formatDuration(item.duration)}</span
            >
          {/if}
          {#if item.enhanced}
            <Badge variant={selected ? 'secondary' : 'default'} class="h-4 px-1.5 text-[10px]"
              >Enhanced</Badge
            >
          {/if}
        </div>
      </div>
    </button>
  </DropdownMenu.Trigger>

  <DropdownMenu.Content align="start">
    <DropdownMenu.Item onclick={handleCopy}>
      <Copy class="size-4" />
      Copy
    </DropdownMenu.Item>
    <DropdownMenu.Item variant="destructive" onclick={handleDelete}>
      <Trash2 class="size-4" />
      Delete
    </DropdownMenu.Item>
  </DropdownMenu.Content>
</DropdownMenu.Root>
