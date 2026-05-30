<script lang="ts">
  import { dictionaryStore, type DictionaryEntry } from '../stores/dictionary.svelte';
  import { open, save } from '@tauri-apps/plugin-dialog';
  import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs';
  import { toast } from 'svelte-sonner';
  import { superForm, defaults } from 'sveltekit-superforms';
  import { zod4 } from 'sveltekit-superforms/adapters';
  import LoadingState from '$components/common/LoadingState.svelte';
  import { dictionarySchema } from '$lib/schemas/dictionary';
  import { Button } from '$components/ui/button';
  import { Input } from '$components/ui/input';
  import { Checkbox } from '$components/ui/checkbox';
  import { Label } from '$components/ui/label';
  import { Badge } from '$components/ui/badge';
  import * as Form from '$components/ui/form';
  import * as Alert from '$components/ui/alert';
  import * as AlertDialog from '$components/ui/alert-dialog';
  import Download from '@lucide/svelte/icons/download';
  import Upload from '@lucide/svelte/icons/upload';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';

  // Index of the entry currently being edited (null = add mode)
  let editingIndex = $state<number | null>(null);

  // Entry captured at delete-click time, held until the AlertDialog confirms
  let pendingDelete = $state<{ index: number; entry: DictionaryEntry } | null>(null);

  // Load entries on mount
  $effect(() => {
    dictionaryStore.load();
  });

  const form = superForm(defaults(zod4(dictionarySchema)), {
    SPA: true,
    validators: zod4(dictionarySchema),
    async onUpdate({ form: f }) {
      if (!f.valid) return;
      const entry: DictionaryEntry = {
        from: (f.data.from as string).trim(),
        to: (f.data.to as string).trim(),
        caseSensitive: f.data.caseSensitive as boolean,
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
        toast.error(e instanceof Error ? e.message : String(e));
      }
    },
  });

  const { form: formData, enhance, reset } = form;

  function resetForm(): void {
    editingIndex = null;
    reset({ data: { from: '', to: '', caseSensitive: false } });
  }

  function startEdit(index: number): void {
    const entry = dictionaryStore.entries[index];
    if (!entry) return;
    editingIndex = index;
    reset({ data: { from: entry.from, to: entry.to, caseSensitive: entry.caseSensitive } });
  }

  function requestDelete(index: number): void {
    const entry = dictionaryStore.entries[index];
    if (!entry) return;
    pendingDelete = { index, entry };
  }

  async function confirmDelete(): Promise<void> {
    if (!pendingDelete) return;
    const { index, entry } = pendingDelete;
    pendingDelete = null;
    try {
      await dictionaryStore.remove(index);
      if (editingIndex === index) {
        resetForm();
      }
      toast.success(`Removed "${entry.from}"`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  function cancelDelete(): void {
    pendingDelete = null;
  }

  async function importFromFile(): Promise<void> {
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
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  async function exportToFile(): Promise<void> {
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
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }
</script>

<div class="flex flex-col gap-6">
  {#if dictionaryStore.error}
    <Alert.Root variant="destructive">
      <Alert.Description>{dictionaryStore.error}</Alert.Description>
    </Alert.Root>
  {/if}

  <form use:enhance class="rounded-lg border p-4">
    <div class="flex gap-3">
      <Form.Field {form} name="from" class="flex-1">
        {#snippet children({ constraints })}
          <Form.Control>
            {#snippet children({ props })}
              <Form.Label>Replace</Form.Label>
              <Input
                {...props}
                {...constraints}
                type="text"
                bind:value={$formData.from}
                placeholder="Text to find..."
              />
            {/snippet}
          </Form.Control>
          <Form.FieldErrors />
        {/snippet}
      </Form.Field>

      <Form.Field {form} name="to" class="flex-1">
        {#snippet children({ constraints })}
          <Form.Control>
            {#snippet children({ props })}
              <Form.Label>With</Form.Label>
              <Input
                {...props}
                {...constraints}
                type="text"
                bind:value={$formData.to}
                placeholder="Replacement text..."
              />
            {/snippet}
          </Form.Control>
          <Form.FieldErrors />
        {/snippet}
      </Form.Field>
    </div>

    <div class="mt-3 flex items-center justify-between">
      <Form.ElementField {form} name="caseSensitive">
        {#snippet children({ value, constraints })}
          <div class="flex items-center gap-2">
            <Checkbox
              id="case-sensitive"
              {...constraints}
              checked={value as boolean}
              onCheckedChange={(checked) => ($formData.caseSensitive = checked === true)}
            />
            <Label for="case-sensitive" class="cursor-pointer text-sm font-normal">
              Case sensitive
            </Label>
          </div>
        {/snippet}
      </Form.ElementField>

      <div class="flex gap-2">
        {#if editingIndex !== null}
          <Button type="button" variant="secondary" size="sm" onclick={resetForm}>Cancel</Button>
        {/if}
        <Button type="submit" size="sm">
          {editingIndex !== null ? 'Update' : 'Add'} Entry
        </Button>
      </div>
    </div>
  </form>

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
      <LoadingState message="Loading dictionary..." />
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
                onclick={() => requestDelete(index)}
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

<AlertDialog.Root
  open={pendingDelete !== null}
  onOpenChange={(open) => {
    if (!open) cancelDelete();
  }}
>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>Delete Dictionary Entry</AlertDialog.Title>
      <AlertDialog.Description>
        Are you sure you want to delete this entry? This action cannot be undone.
      </AlertDialog.Description>
    </AlertDialog.Header>
    {#if pendingDelete}
      <p class="rounded-md bg-muted px-3 py-2 text-sm italic text-muted-foreground">
        "{pendingDelete.entry.from}" &rarr; "{pendingDelete.entry.to}"
      </p>
    {/if}
    <AlertDialog.Footer>
      <AlertDialog.Cancel onclick={cancelDelete}>Cancel</AlertDialog.Cancel>
      <AlertDialog.Action onclick={confirmDelete} variant="destructive">Delete</AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
