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

  // Form state for adding/editing entries
  let editingIndex = $state<number | null>(null);
  let fromValue = $state('');
  let toValue = $state('');
  let caseSensitive = $state(false);
  let formError = $state<string | null>(null);
  let successMessage = $state<string | null>(null);

  // Load entries on mount
  $effect(() => {
    dictionaryStore.load();
  });

  // Clear messages after delay
  $effect(() => {
    if (successMessage) {
      const timeout = setTimeout(() => {
        successMessage = null;
      }, 3000);
      return () => clearTimeout(timeout);
    }
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
        successMessage = 'Entry updated successfully';
      } else {
        await dictionaryStore.add(entry);
        successMessage = 'Entry added successfully';
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
      successMessage = 'Entry removed successfully';
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
        successMessage = `Imported ${count} entries`;
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
        successMessage = 'Dictionary exported successfully';
      }
    } catch (e) {
      formError = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="dictionary-editor">
  <div class="header">
    <h3>Dictionary</h3>
    <p class="description">
      Add custom word replacements for domain-specific vocabulary and corrections.
    </p>
  </div>

  {#if successMessage}
    <div class="message success">{successMessage}</div>
  {/if}

  {#if formError || dictionaryStore.error}
    <div class="message error">{formError || dictionaryStore.error}</div>
  {/if}

  <div class="form-section">
    <div class="form-row">
      <div class="form-field">
        <label for="from-input">Replace</label>
        <input id="from-input" type="text" bind:value={fromValue} placeholder="Text to find..." />
      </div>
      <div class="form-field">
        <label for="to-input">With</label>
        <input id="to-input" type="text" bind:value={toValue} placeholder="Replacement text..." />
      </div>
    </div>
    <div class="form-row options">
      <label class="checkbox-label">
        <input type="checkbox" bind:checked={caseSensitive} />
        Case sensitive
      </label>
      <div class="form-actions">
        {#if editingIndex !== null}
          <button class="secondary" onclick={resetForm}>Cancel</button>
        {/if}
        <button class="primary" onclick={saveEntry}>
          {editingIndex !== null ? 'Update' : 'Add'} Entry
        </button>
      </div>
    </div>
  </div>

  <div class="entries-section">
    <div class="entries-header">
      <span class="entries-count">
        {dictionaryStore.entries.length}
        {dictionaryStore.entries.length === 1 ? 'entry' : 'entries'}
      </span>
      <div class="import-export">
        <button class="icon-btn" onclick={importFromFile} title="Import dictionary">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
            <polyline points="7 10 12 15 17 10"></polyline>
            <line x1="12" y1="15" x2="12" y2="3"></line>
          </svg>
          Import
        </button>
        <button class="icon-btn" onclick={exportToFile} title="Export dictionary">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
            <polyline points="17 8 12 3 7 8"></polyline>
            <line x1="12" y1="3" x2="12" y2="15"></line>
          </svg>
          Export
        </button>
      </div>
    </div>

    {#if dictionaryStore.loading}
      <div class="loading">Loading dictionary...</div>
    {:else if dictionaryStore.entries.length === 0}
      <div class="empty-state">
        <p>No dictionary entries yet.</p>
        <p class="hint">Add entries above to automatically replace text in your transcriptions.</p>
      </div>
    {:else}
      <div class="entries-list">
        {#each dictionaryStore.entries as entry, index}
          <div class="entry" class:editing={editingIndex === index}>
            <div class="entry-content">
              <span class="from">{entry.from}</span>
              <span class="arrow">&rarr;</span>
              <span class="to">{entry.to}</span>
              {#if entry.caseSensitive}
                <span class="badge">Case sensitive</span>
              {/if}
            </div>
            <div class="entry-actions">
              <button class="edit-btn" onclick={() => startEdit(index)} title="Edit entry">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"></path>
                  <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"></path>
                </svg>
              </button>
              <button class="delete-btn" onclick={() => deleteEntry(index)} title="Delete entry">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="14"
                  height="14"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                >
                  <polyline points="3 6 5 6 21 6"></polyline>
                  <path
                    d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"
                  ></path>
                </svg>
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .dictionary-editor {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .header h3 {
    margin: 0 0 4px 0;
    font-size: 16px;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .description {
    margin: 0;
    font-size: 13px;
    color: var(--color-text-secondary);
  }

  .message {
    padding: 10px 14px;
    border-radius: var(--radius-md);
    font-size: 13px;
  }

  .message.success {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
    border: 1px solid color-mix(in srgb, var(--color-success) 30%, transparent);
  }

  .message.error {
    background: color-mix(in srgb, var(--color-error) 15%, transparent);
    color: var(--color-error);
    border: 1px solid color-mix(in srgb, var(--color-error) 30%, transparent);
  }

  .form-section {
    background: var(--color-bg-secondary);
    padding: 16px;
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
  }

  .form-row {
    display: flex;
    gap: 12px;
  }

  .form-row.options {
    margin-top: 12px;
    align-items: center;
    justify-content: space-between;
  }

  .form-field {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .form-field label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .form-field input {
    width: 100%;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--color-text-secondary);
    cursor: pointer;
  }

  .checkbox-label input[type='checkbox'] {
    width: 16px;
    height: 16px;
    cursor: pointer;
  }

  .form-actions {
    display: flex;
    gap: 8px;
  }

  .form-actions button {
    padding: 8px 16px;
    font-size: 13px;
  }

  .entries-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .entries-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .entries-count {
    font-size: 13px;
    color: var(--color-text-secondary);
  }

  .import-export {
    display: flex;
    gap: 8px;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    font-size: 12px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
  }

  .icon-btn:hover {
    background: var(--color-bg-tertiary);
  }

  .loading {
    padding: 24px;
    text-align: center;
    color: var(--color-text-secondary);
    font-size: 13px;
  }

  .empty-state {
    padding: 32px 24px;
    text-align: center;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px dashed var(--color-border);
  }

  .empty-state p {
    margin: 0;
    color: var(--color-text-secondary);
    font-size: 13px;
  }

  .empty-state .hint {
    margin-top: 8px;
    color: var(--color-text-tertiary);
  }

  .entries-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .entry {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
    transition: border-color var(--transition-fast);
  }

  .entry.editing {
    border-color: var(--color-accent);
  }

  .entry:hover {
    border-color: var(--color-bg-hover);
  }

  .entry-content {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
  }

  .from {
    font-weight: 500;
    color: var(--color-text-primary);
  }

  .arrow {
    color: var(--color-text-tertiary);
    flex-shrink: 0;
  }

  .to {
    color: var(--color-accent);
  }

  .badge {
    font-size: 10px;
    padding: 2px 6px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-full);
    color: var(--color-text-secondary);
    flex-shrink: 0;
  }

  .entry-actions {
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }

  .entry:hover .entry-actions {
    opacity: 1;
  }

  .edit-btn,
  .delete-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: transparent;
    border-radius: var(--radius-sm);
  }

  .edit-btn:hover {
    background: var(--color-bg-tertiary);
    color: var(--color-accent);
  }

  .delete-btn:hover {
    background: color-mix(in srgb, var(--color-error) 20%, transparent);
    color: var(--color-error);
  }

  .secondary {
    background: var(--color-bg-tertiary);
  }

  .secondary:hover {
    background: var(--color-bg-hover);
  }
</style>
