<script lang="ts">
  /**
   * AI Enhancement Settings component
   *
   * Provides UI for configuring AI enhancement including Ollama connection,
   * model selection, and prompt template management.
   */

  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { configStore } from '../stores/config.svelte';
  import { toastStore } from '../stores/toast.svelte';

  /** Prompt template matching Rust PromptTemplate struct */
  interface PromptTemplate {
    id: string;
    name: string;
    template: string;
    isBuiltin: boolean;
  }

  let ollamaAvailable = $state(false);
  let ollamaModels = $state<string[]>([]);
  let prompts = $state<PromptTemplate[]>([]);
  let isCheckingOllama = $state(false);
  let isLoadingModels = $state(false);
  let isLoadingPrompts = $state(false);
  let error = $state<string | null>(null);

  // Custom prompt editor state
  let isEditing = $state(false);
  let editingPrompt = $state<PromptTemplate | null>(null);
  let newPromptName = $state('');
  let newPromptTemplate = $state('');
  let promptError = $state<string | null>(null);

  /** Check if Ollama server is available */
  async function checkOllama(): Promise<void> {
    isCheckingOllama = true;
    error = null;

    try {
      ollamaAvailable = await invoke<boolean>('check_ollama_available');
      if (ollamaAvailable) {
        await loadModels();
      }
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to check Ollama connection';
      ollamaAvailable = false;
    } finally {
      isCheckingOllama = false;
    }
  }

  /** Load available Ollama models */
  async function loadModels(): Promise<void> {
    isLoadingModels = true;

    try {
      ollamaModels = await invoke<string[]>('list_ollama_models');
    } catch (e) {
      console.error('Failed to load Ollama models:', e);
      ollamaModels = [];
    } finally {
      isLoadingModels = false;
    }
  }

  /** Load all prompt templates */
  async function loadPrompts(): Promise<void> {
    isLoadingPrompts = true;

    try {
      prompts = await invoke<PromptTemplate[]>('get_all_prompts');
    } catch (e) {
      console.error('Failed to load prompts:', e);
      prompts = [];
    } finally {
      isLoadingPrompts = false;
    }
  }

  /** Save settings to backend with auto-save */
  async function saveSettings(): Promise<void> {
    try {
      await configStore.save();
      toastStore.success('Settings saved');
    } catch (e) {
      console.error('Failed to save settings:', e);
      toastStore.error('Failed to save settings');
    }
  }

  /** Handle enhancement enabled toggle */
  async function handleEnabledChange(): Promise<void> {
    await saveSettings();
    // Rebuild tray so the AI Enhancement submenu reflects the new state
    invoke('refresh_tray_menu').catch(() => {});
  }

  /** Handle model selection change */
  function handleModelChange(event: Event): void {
    const select = event.target as HTMLSelectElement;
    configStore.updateEnhancement('model', select.value);
    saveSettings();
  }

  /** Handle prompt selection change */
  async function handlePromptChange(event: Event): Promise<void> {
    const select = event.target as HTMLSelectElement;
    configStore.updateEnhancement('promptId', select.value);
    await saveSettings();
    // Rebuild tray so the prompt submenu checkmark is updated
    invoke('refresh_tray_menu').catch(() => {});
  }

  /** Handle Ollama URL change */
  function handleUrlChange(event: Event): void {
    const input = event.target as HTMLInputElement;
    configStore.updateEnhancement('ollamaUrl', input.value);
  }

  /** Handle Ollama URL blur (save and re-check) */
  async function handleUrlBlur(): Promise<void> {
    await saveSettings();
    await checkOllama();
  }

  /** Start creating a new custom prompt */
  function startNewPrompt(): void {
    isEditing = true;
    editingPrompt = null;
    newPromptName = '';
    newPromptTemplate = 'Enhance the following text:\n\n{text}';
    promptError = null;
  }

  /** Start editing an existing custom prompt */
  function startEditPrompt(prompt: PromptTemplate): void {
    if (prompt.isBuiltin) return;

    isEditing = true;
    editingPrompt = prompt;
    newPromptName = prompt.name;
    newPromptTemplate = prompt.template;
    promptError = null;
  }

  /** Cancel prompt editing */
  function cancelEdit(): void {
    isEditing = false;
    editingPrompt = null;
    promptError = null;
  }

  /** Generate a slug ID from name */
  function generateId(name: string): string {
    return name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');
  }

  /** Save the custom prompt */
  async function savePrompt(): Promise<void> {
    promptError = null;

    if (!newPromptName.trim()) {
      promptError = 'Please enter a prompt name';
      return;
    }

    if (!newPromptTemplate.trim()) {
      promptError = 'Please enter a prompt template';
      return;
    }

    if (!newPromptTemplate.includes('{text}')) {
      promptError = 'Template must include {text} placeholder';
      return;
    }

    const prompt: PromptTemplate = {
      id: editingPrompt?.id || generateId(newPromptName),
      name: newPromptName.trim(),
      template: newPromptTemplate.trim(),
      isBuiltin: false,
    };

    try {
      await invoke('save_custom_prompt_cmd', { prompt });
      await loadPrompts();
      cancelEdit();
      // Rebuild tray so the prompt submenu reflects the new/edited prompt
      invoke('refresh_tray_menu').catch(() => {});
    } catch (e) {
      promptError = e instanceof Error ? e.message : 'Failed to save prompt';
    }
  }

  /** Delete a custom prompt */
  async function deletePrompt(promptId: string): Promise<void> {
    try {
      await invoke('delete_custom_prompt_cmd', { promptId });
      await loadPrompts();

      // If the deleted prompt was selected, reset to default
      if (configStore.enhancement.promptId === promptId) {
        configStore.updateEnhancement('promptId', 'fix-grammar');
        await saveSettings();
      }

      // Rebuild tray so the prompt submenu stays in sync
      invoke('refresh_tray_menu').catch(() => {});
    } catch (e) {
      console.error('Failed to delete prompt:', e);
    }
  }

  /** Get the currently selected prompt */
  function getSelectedPrompt(): PromptTemplate | undefined {
    return prompts.find((p) => p.id === configStore.enhancement.promptId);
  }

  /** Open the prompt writing guide window */
  async function openPromptGuide(): Promise<void> {
    try {
      await invoke('show_window', { label: 'prompt-guide' });
    } catch (e) {
      console.error('Failed to open prompt guide:', e);
    }
  }

  onMount(async () => {
    await configStore.load();
    await loadPrompts();
    await checkOllama();
  });
</script>

<div class="ai-settings">
  {#if configStore.isLoading}
    <div class="loading">Loading settings...</div>
  {:else}
    <div class="setting-group">
      <div class="setting-row card">
        <div class="setting-info">
          <span class="setting-label">Enable AI enhancement</span>
          <span class="setting-description">
            Use Ollama to enhance transcriptions with grammar correction, formatting, and more
          </span>
        </div>
        <label class="toggle-switch">
          <input
            type="checkbox"
            bind:checked={configStore.config.enhancement.enabled}
            onchange={handleEnabledChange}
          />
          <span class="toggle-slider"></span>
        </label>
      </div>
    </div>

    <div class="setting-group">
      <h3>Ollama Connection</h3>

      <div class="setting-row card vertical">
        <div class="setting-info">
          <span class="setting-label">Server URL</span>
          <span class="setting-description">The URL of your local Ollama server</span>
        </div>
        <div class="url-input-row">
          <input
            type="url"
            class="url-input"
            value={configStore.config.enhancement.ollamaUrl}
            oninput={handleUrlChange}
            onblur={handleUrlBlur}
            placeholder="http://localhost:11434"
          />
          <button class="test-btn" onclick={checkOllama} disabled={isCheckingOllama}>
            {isCheckingOllama ? 'Testing...' : 'Test Connection'}
          </button>
        </div>
        <div
          class="connection-inline"
          class:connected={ollamaAvailable}
          class:disconnected={!ollamaAvailable && !isCheckingOllama}
        >
          {#if isCheckingOllama}
            <span class="status-indicator checking"></span>
            <span>Checking connection...</span>
          {:else if ollamaAvailable}
            <span class="status-indicator connected"></span>
            <span>Connected to Ollama</span>
          {:else}
            <span class="status-indicator disconnected"></span>
            <span>Not connected. Make sure Ollama is running.</span>
          {/if}
        </div>
      </div>

      {#if error}
        <div class="error-message">{error}</div>
      {/if}

      <div class="setting-row card">
        <div class="setting-info">
          <span class="setting-label">Model</span>
          <span class="setting-description">The Ollama model used for text enhancement</span>
        </div>
        <select
          class="select-control"
          value={configStore.config.enhancement.model}
          onchange={handleModelChange}
          disabled={!ollamaAvailable || isLoadingModels}
        >
          {#if isLoadingModels}
            <option>Loading models...</option>
          {:else if ollamaModels.length === 0}
            <option value={configStore.config.enhancement.model}>
              {configStore.config.enhancement.model} (not available)
            </option>
          {:else}
            {#each ollamaModels as model}
              <option value={model}>{model}</option>
            {/each}
          {/if}
        </select>
      </div>

      {#if !ollamaAvailable}
        <p class="hint">
          Connect to Ollama to see available models. You can download models using
          <code>ollama pull &lt;model&gt;</code> in your terminal.
        </p>
      {/if}
    </div>

    <div class="setting-group">
      <h3>Prompt Templates</h3>

      <div class="setting-row card vertical">
        <div class="prompt-selector-row">
          <div class="setting-info">
            <span class="setting-label">Active Prompt</span>
            <span class="setting-description">
              The prompt template used to enhance transcriptions
            </span>
          </div>
          <select
            class="select-control"
            value={configStore.config.enhancement.promptId}
            onchange={handlePromptChange}
            disabled={isLoadingPrompts}
          >
            {#if isLoadingPrompts}
              <option>Loading...</option>
            {:else}
              {#each prompts as prompt}
                <option value={prompt.id}>
                  {prompt.name}
                  {prompt.isBuiltin ? '' : ' (Custom)'}
                </option>
              {/each}
            {/if}
          </select>
        </div>
        {#if getSelectedPrompt()}
          <pre class="prompt-preview-text">{getSelectedPrompt()?.template}</pre>
        {/if}
      </div>

      <div class="custom-prompts-section">
        <div class="custom-prompts-header">
          <span class="setting-label">Custom Prompts</span>
          <button class="btn-small" onclick={startNewPrompt}>+ Add Prompt</button>
        </div>

        <p class="hint">
          Create your own prompt templates for specific use cases.
          <button class="help-link" onclick={openPromptGuide}>
            View Prompt Writing Guide
          </button>
          for tips on writing effective prompts.
        </p>

        {#if isEditing}
          <div class="prompt-editor">
            <div class="editor-field">
              <label for="prompt-name">Name</label>
              <input
                id="prompt-name"
                type="text"
                bind:value={newPromptName}
                placeholder="My Custom Prompt"
              />
            </div>

            <div class="editor-field">
              <label for="prompt-template">
                Template
                <span class="label-hint">(use {'{text}'} as placeholder)</span>
              </label>
              <textarea
                id="prompt-template"
                bind:value={newPromptTemplate}
                rows="5"
                placeholder="Enhance this text: {'{text}'}"
              ></textarea>
            </div>

            {#if promptError}
              <div class="editor-error">{promptError}</div>
            {/if}

            <div class="editor-actions">
              <button class="cancel-btn" onclick={cancelEdit}>Cancel</button>
              <button class="save-btn primary" onclick={savePrompt}>
                {editingPrompt ? 'Update' : 'Create'} Prompt
              </button>
            </div>
          </div>
        {/if}

        <div class="custom-prompts-list">
          {#each prompts.filter((p) => !p.isBuiltin) as prompt}
            <div class="custom-prompt-item">
              <div class="prompt-item-info">
                <span class="prompt-item-name">{prompt.name}</span>
              </div>
              <div class="prompt-item-actions">
                <button
                  class="edit-btn-small"
                  onclick={() => startEditPrompt(prompt)}
                  title="Edit prompt"
                >
                  Edit
                </button>
                <button
                  class="delete-btn-small"
                  onclick={() => deletePrompt(prompt.id)}
                  title="Delete prompt"
                >
                  Delete
                </button>
              </div>
            </div>
          {:else}
            <div class="no-custom-prompts">
              No custom prompts yet. Click "Add Prompt" to create one.
            </div>
          {/each}
        </div>
      </div>
    </div>

  {/if}
</div>

<style>
  /* AI Enhancement Settings - component-specific styles only
     Common styles (toggle-switch, setting-row, setting-info, etc.) are in app.css */

  .ai-settings {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .loading {
    color: var(--color-text-secondary);
    padding: 24px;
    text-align: center;
  }

  /* URL input row */
  .url-input-row {
    display: flex;
    gap: 8px;
  }

  .url-input {
    flex: 1;
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
    font-size: var(--text-sm);
    font-family: var(--font-mono);
  }

  .url-input:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .test-btn {
    padding: 8px 16px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-tertiary);
    color: var(--color-text-primary);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast);
  }

  .test-btn:hover:not(:disabled) {
    background: var(--color-bg-hover);
  }

  .test-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Connection status inline within card */
  .connection-inline {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }

  .connection-inline.connected {
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    color: var(--color-success);
  }

  .connection-inline.disconnected {
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    color: var(--color-warning);
  }

  /* Prompt selector row (horizontal layout within vertical card) */
  .prompt-selector-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  /* Prompt preview inline within card */
  .prompt-preview-text {
    margin: 0;
    padding: 10px 12px;
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.5;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-sm);
  }

  /* Custom prompts section */
  .custom-prompts-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .custom-prompts-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .help-link {
    background: none;
    border: none;
    color: var(--color-accent);
    text-decoration: none;
    cursor: pointer;
    padding: 0;
    font-size: inherit;
    font-family: inherit;
  }

  .help-link:hover {
    text-decoration: underline;
  }

  /* Prompt editor */
  .prompt-editor {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-md);
  }

  .editor-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .editor-field label {
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-text-secondary);
  }

  .label-hint {
    font-weight: 400;
    color: var(--color-text-tertiary);
  }

  .editor-field input,
  .editor-field textarea {
    padding: 8px 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-bg-secondary);
    color: var(--color-text-primary);
    font-size: var(--text-sm);
  }

  .editor-field textarea {
    font-family: var(--font-mono);
    resize: vertical;
    line-height: 1.5;
  }

  .editor-field input:focus,
  .editor-field textarea:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .editor-error {
    padding: 8px 12px;
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
    border-radius: var(--radius-md);
    color: var(--color-error);
    font-size: var(--text-xs);
  }

  .editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .cancel-btn,
  .save-btn {
    padding: 8px 16px;
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .cancel-btn {
    background: var(--color-bg-secondary);
    border: 1px solid var(--color-border);
    color: var(--color-text-primary);
  }

  .cancel-btn:hover {
    background: var(--color-bg-hover);
  }

  .save-btn.primary {
    background: var(--color-accent);
    border: none;
    color: white;
  }

  .save-btn.primary:hover {
    background: var(--color-accent-hover);
  }

  /* Custom prompts list */
  .custom-prompts-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .custom-prompt-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px;
    background: var(--color-bg-tertiary);
    border-radius: var(--radius-md);
  }

  .prompt-item-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .prompt-item-name {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
  }

  .prompt-item-actions {
    display: flex;
    gap: 6px;
  }

  .edit-btn-small,
  .delete-btn-small {
    padding: 4px 10px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: transparent;
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .edit-btn-small {
    color: var(--color-text-secondary);
  }

  .edit-btn-small:hover {
    background: var(--color-bg-hover);
    color: var(--color-text-primary);
  }

  .delete-btn-small {
    color: var(--color-text-tertiary);
  }

  .delete-btn-small:hover {
    background: color-mix(in srgb, var(--color-error) 10%, transparent);
    color: var(--color-error);
    border-color: var(--color-error);
  }

  .no-custom-prompts {
    padding: 16px;
    text-align: center;
    color: var(--color-text-tertiary);
    font-size: var(--text-sm);
  }
</style>
