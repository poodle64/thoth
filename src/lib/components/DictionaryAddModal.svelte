<script lang="ts">
  /**
   * Quick-add to dictionary modal.
   *
   * Shown when the user presses the dictionary shortcut key.
   * `word` pre-fills the "from" field (clipboard content).
   * User types the replacement and confirms.
   */
  import { invoke } from '@tauri-apps/api/core';
  import { emit } from '@tauri-apps/api/event';

  interface Props {
    open: boolean;
    word: string;
    onclose: () => void;
  }

  let { open, word, onclose }: Props = $props();

  let fromWord = $state('');
  let toWord = $state('');
  let saving = $state(false);
  let inputEl: HTMLInputElement | null = null;

  // When the modal opens, pre-fill fields
  $effect(() => {
    if (open) {
      fromWord = word;
      toWord = word;
      saving = false;
      // Focus the replacement input on next tick
      setTimeout(() => inputEl?.focus(), 50);
    }
  });

  function handleBackdropClick(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains('backdrop')) onclose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      save();
    }
  }

  async function save() {
    const from = fromWord.trim();
    const to = toWord.trim();
    if (!from || !to) return;

    saving = true;
    try {
      await invoke('add_dictionary_entry', {
        entry: { from, to, caseSensitive: false },
      });
      emit('dictionary-updated', { from, to }).catch(() => {});
      emit('toast', { kind: 'success', message: `Added "${from}" → "${to}" to dictionary` }).catch(() => {});
      onclose();
    } catch (e) {
      emit('toast', {
        kind: 'error',
        message: `Failed to add entry: ${e instanceof Error ? e.message : String(e)}`,
      }).catch(() => {});
      saving = false;
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={handleBackdropClick} onkeydown={handleKeydown} role="dialog" aria-modal="true" tabindex="-1">
    <div class="dialog">
      <h2 class="title">Add to Dictionary</h2>

      <div class="field">
        <label for="dict-from">Replace</label>
        <input
          id="dict-from"
          class="text-input"
          type="text"
          bind:value={fromWord}
          placeholder="Word or phrase to replace"
        />
      </div>

      <div class="field">
        <label for="dict-to">With</label>
        <input
          id="dict-to"
          class="text-input"
          type="text"
          bind:value={toWord}
          bind:this={inputEl}
          placeholder="Replacement text"
        />
      </div>

      <div class="actions">
        <button class="btn-secondary" onclick={onclose} disabled={saving}>Cancel</button>
        <button
          class="btn-primary"
          onclick={save}
          disabled={saving || !fromWord.trim() || !toWord.trim()}
        >
          {saving ? 'Adding…' : 'Add'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(4px);
    animation: fade-in 0.15s ease;
  }

  .dialog {
    width: 340px;
    padding: 28px 24px 20px;
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    display: flex;
    flex-direction: column;
    gap: 16px;
    animation: scale-in 0.15s ease;
  }

  .title {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  label {
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .text-input {
    width: 100%;
    padding: 8px 10px;
    font-size: 0.875rem;
    background: var(--color-bg-primary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    color: var(--color-text-primary);
    outline: none;
    box-sizing: border-box;
    transition: border-color 0.15s;
  }

  .text-input:focus {
    border-color: var(--color-accent);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }

  .btn-primary,
  .btn-secondary {
    padding: 7px 16px;
    font-size: 0.8125rem;
    font-weight: 500;
    border-radius: var(--radius-sm);
    border: none;
    cursor: pointer;
    transition: opacity 0.15s;
  }

  .btn-primary {
    background: var(--color-accent);
    color: #fff;
  }

  .btn-secondary {
    background: var(--color-bg-tertiary);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border);
  }

  .btn-primary:disabled,
  .btn-secondary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  @keyframes fade-in {
    from { opacity: 0; }
    to   { opacity: 1; }
  }

  @keyframes scale-in {
    from { transform: scale(0.95); opacity: 0; }
    to   { transform: scale(1);    opacity: 1; }
  }
</style>
