<script lang="ts">
  /**
   * Dictionary Editor component for managing vocabulary replacements
   *
   * Provides UI for CRUD operations on dictionary entries with
   * import/export functionality.
   */
  import { dictionaryStore, type DictionaryEntry } from '../stores/dictionary.svelte';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
  import { toast } from 'svelte-sonner';
  import { Button } from '$components/ui/button';
  import { Input } from '$components/ui/input';
  import { Label } from '$components/ui/label';
  import { Checkbox } from '$components/ui/checkbox';
  import { Badge } from '$components/ui/badge';
  import * as Alert from '$components/ui/alert';
  import Download from '@lucide/svelte/icons/download';
  import Upload from '@lucide/svelte/icons/upload';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';

  // Form state for adding/editing entries
  let editingIndex = $state<number | null>(null);
  let fromValue = $state('');
  let toValue = $state('');
  let caseSensitive = $state(false);
  let formError = $state<string | null>(null);

  // Load entries on mount
  $effect(() => {
    dictionaryStore.load();
  });

  /** Reset form to default state */
  function resetForm(): void {
    editingIndex = null;
    fromValue = '';
    toValue = '';
    caseSensitive = false;
    formError = null;
  }

  /** Start editing an existing entry */
  function startEdit(index: number): void {
    const entry = dictionaryStore.entries[index];
    if (entry) {
      editingIndex = index;
      fromValue = entry.from;
      toValue = entry.to;
      caseSensitive = entry.caseSensitive;
      formError = null;
    }
  }

  /** Save the current entry (add or update) */
  async function saveEntry(): Promise<void> {
    formError = null;

    if (!fromValue.trim()) {
      formError = 'Please enter the text to replace';
      return;
    }
    if (!toValue.trim()) {
      formError = 'Please enter the replacement text';
      return;
    }

    const entry: DictionaryEntry = {
      from: fromValue.trim(),
      to: toValue.trim(),
      caseSensitive,
    };

    try {
      if (editingIndex !== null) {
        await dictionaryStore.update(editingIndex, entry);
        toast.success('Entry updated successfully');
      } else {
        await dictionaryStore.add(entry);
        toast.success('Entry added successfully');
      }
      resetForm();
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }

  /** Delete an entry */
  async function deleteEntry(index: number): Promise<void> {
    try {
      await dictionaryStore.remove(index);
      if (editingIndex === index) {
        resetForm();
      }
      toast.success('Entry removed successfully');
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }

  /** Import dictionary from file */
  async function importFromFile(): Promise<void> {
    formError = null;
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });

      if (selected) {
        const content = await readTextFile(selected);
        const count = await dictionaryStore.importEntries(content, true);
        toast.success(`Imported ${count} entries`);
      }
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }

  /** Export dictionary to file */
  async function exportToFile(): Promise<void> {
    formError = null;
    try {
      const content = await dictionaryStore.exportEntries();
      const path = await save({
        filters: [{ name: 'JSON', extensions: ['json'] }],
        defaultPath: 'thoth-dictionary.json',
      });

      if (path) {
        await writeTextFile(path, content);
        toast.success('Dictionary exported successfully');
      }
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="flex flex-col gap-6">
  {#if formError || dictionaryStore.error}
    <Alert.Root variant="destructive">
      <Alert.Description>{formError || dictionaryStore.error}</Alert.Description>
    </Alert.Root>
  {/if}

  <div class="rounded-lg border p-4">
    <div class="flex gap-3">
      <div class="flex flex-1 flex-col gap-1.5">
        <Label for="from-input">Replace</Label>
        <Input id="from-input" type="text" bind:value={fromValue} placeholder="Text to find..." />
      </div>
      <div class="flex flex-1 flex-col gap-1.5">
        <Label for="to-input">With</Label>
        <Input id="to-input" type="text" bind:value={toValue} placeholder="Replacement text..." />
      </div>
    </div>
    <div class="mt-3 flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Checkbox id="case-sensitive" bind:checked={caseSensitive} />
        <Label for="case-sensitive" class="cursor-pointer text-sm font-normal">
          Case sensitive
        </Label>
      </div>
      <div class="flex gap-2">
        {#if editingIndex !== null}
          <Button variant="secondary" size="sm" onclick={resetForm}>Cancel</Button>
        {/if}
        <Button size="sm" onclick={saveEntry}>
          {editingIndex !== null ? 'Update' : 'Add'} Entry
        </Button>
      </div>
    </div>
  </div>

  <div class="flex flex-col gap-3">
    <div class="flex items-center justify-between">
      <span class="text-muted-foreground text-sm">
        {dictionaryStore.entries.length}
        {dictionaryStore.entries.length === 1 ? 'entry' : 'entries'}
      </span>
      <div class="flex gap-2">
        <Button variant="outline" size="sm" onclick={importFromFile}>
          <Download class="mr-1.5 h-3.5 w-3.5" />
          Import
        </Button>
        <Button variant="outline" size="sm" onclick={exportToFile}>
          <Upload class="mr-1.5 h-3.5 w-3.5" />
          Export
        </Button>
      </div>
    </div>

    {#if dictionaryStore.loading}
      <div class="text-muted-foreground p-6 text-center text-sm">Loading dictionary...</div>
    {:else if dictionaryStore.entries.length === 0}
      <div class="rounded-lg border border-dashed p-8 text-center">
        <p class="text-muted-foreground text-sm">No dictionary entries yet.</p>
        <p class="text-muted-foreground/70 mt-2 text-xs">
          Add entries above to automatically replace text in your transcriptions.
        </p>
      </div>
    {:else}
      <div class="flex flex-col gap-1">
        {#each dictionaryStore.entries as entry, index}
          <div
            class="group flex items-center justify-between rounded-md border px-3.5 py-2.5 transition-colors {editingIndex ===
            index
              ? 'border-primary'
              : 'hover:border-border'}"
          >
            <div class="flex min-w-0 flex-1 items-center gap-2">
              <span class="text-sm font-medium">{entry.from}</span>
              <span class="text-muted-foreground">&rarr;</span>
              <span class="text-primary text-sm">{entry.to}</span>
              {#if entry.caseSensitive}
                <Badge variant="secondary" class="text-xs">Case sensitive</Badge>
              {/if}
            </div>
            <div class="flex gap-1 opacity-0 transition-opacity group-hover:opacity-100">
              <Button
                variant="ghost"
                size="icon"
                class="h-7 w-7"
                onclick={() => startEdit(index)}
                title="Edit entry"
              >
                <Pencil class="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                class="text-destructive hover:text-destructive h-7 w-7"
                onclick={() => deleteEntry(index)}
                title="Delete entry"
              >
                <Trash2 class="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
