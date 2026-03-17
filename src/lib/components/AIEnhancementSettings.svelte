<script lang="ts">
  /**
   * AI Enhancement Settings component
   *
   * Provides UI for configuring AI enhancement including Cloud AI (Anthropic)
   * and Local AI (Ollama / OpenAI-compatible) backends, model selection,
   * and prompt template management.
   */

  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { configStore, type EnhancementBackend } from '../stores/config.svelte';
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

  // Track last local backend for toggle memory
  let lastLocalBackend = $state<'ollama' | 'openai_compat'>('ollama');

  // Anthropic-specific state
  let anthropicKeyDetected = $state<string | null>(null);
  let anthropicKeyVisible = $state(false);

  /** Check if the current backend is cloud-based */
  function isCloudBackend(): boolean {
    return configStore.config.enhancement.backend === 'anthropic';
  }

  /** Check if the current backend is local */
  function isLocalBackend(): boolean {
    return (
      configStore.config.enhancement.backend === 'ollama' ||
      configStore.config.enhancement.backend === 'openai_compat'
    );
  }

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
      error = e instanceof Error ? e.message : 'Failed to check connection';
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
      console.error('Failed to load models:', e);
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

  /** Handle URL blur (save, update backend, and re-check) */
  async function handleUrlBlur(): Promise<void> {
    await saveSettings();
    await applyBackend();
    await checkOllama();
  }

  /** Handle backend selection change */
  async function handleBackendChange(event: Event): Promise<void> {
    const select = event.target as HTMLSelectElement;
    configStore.updateEnhancement('backend', select.value as EnhancementBackend);
    await saveSettings();
    await applyBackend();
    await checkOllama();
  }

  /** Handle API key change */
  function handleApiKeyChange(event: Event): void {
    const input = event.target as HTMLInputElement;
    configStore.updateEnhancement('apiKey', input.value);
  }

  /** Handle API key blur (save and re-check) */
  async function handleApiKeyBlur(): Promise<void> {
    await saveSettings();
    await applyBackend();
    await checkOllama();
  }

  /** Notify the backend of the current enhancement backend config */
  async function applyBackend(): Promise<void> {
    try {
      await invoke('set_enhancement_backend', {
        backend: configStore.config.enhancement.backend,
        baseUrl: configStore.config.enhancement.ollamaUrl,
        apiKey: configStore.config.enhancement.apiKey || null,
        anthropicApiKey: configStore.config.enhancement.anthropicApiKey || null,
        anthropicModel: configStore.config.enhancement.anthropicModel || null,
        anthropicBaseUrl: configStore.config.enhancement.anthropicUrl || null,
      });
      invoke('refresh_tray_menu').catch(() => {});
    } catch (e) {
      console.error('Failed to set enhancement backend:', e);
    }
  }

  /** Detect Anthropic API key from environment */
  async function detectAnthropicKey(): Promise<void> {
    try {
      const key = await invoke<string | null>('detect_anthropic_api_key');
      if (key) {
        anthropicKeyDetected = key;
        if (!configStore.config.enhancement.anthropicApiKey) {
          configStore.updateEnhancement('anthropicApiKey', key);
          await saveSettings();
        }
      }
    } catch (e) {
      console.error('Failed to detect Anthropic key:', e);
    }
  }

  /** Open Anthropic console in browser */
  async function openAnthropicConsole(): Promise<void> {
    await invoke('open_anthropic_console');
  }

  /** Handle Anthropic API key change */
  function handleAnthropicKeyChange(event: Event): void {
    const input = event.target as HTMLInputElement;
    configStore.updateEnhancement('anthropicApiKey', input.value);
  }

  /** Handle Anthropic API key blur */
  async function handleAnthropicKeyBlur(): Promise<void> {
    await saveSettings();
    if (configStore.config.enhancement.backend === 'anthropic') {
      await applyBackend();
      await checkOllama();
    }
  }

  /** Handle Anthropic model change */
  async function handleAnthropicModelChange(event: Event): Promise<void> {
    const select = event.target as HTMLSelectElement;
    configStore.updateEnhancement('anthropicModel', select.value);
    await saveSettings();
    if (configStore.config.enhancement.backend === 'anthropic') {
      await applyBackend();
    }
  }

  /** Switch to cloud backend */
  async function switchToCloud(): Promise<void> {
    // Remember what local backend was before switching
    if (isLocalBackend()) {
      lastLocalBackend = configStore.config.enhancement.backend as 'ollama' | 'openai_compat';
    }
    configStore.updateEnhancement('backend', 'anthropic');
    await saveSettings();
    await applyBackend();
    await checkOllama();
  }

  /** Switch to local backend */
  async function switchToLocal(): Promise<void> {
    configStore.updateEnhancement('backend', lastLocalBackend);
    await saveSettings();
    await applyBackend();
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
      await invoke('delete_custom_prompt_cmd', { prompt_id: promptId });
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
    // Remember the last local backend
    if (isLocalBackend()) {
      lastLocalBackend = configStore.config.enhancement.backend as 'ollama' | 'openai_compat';
    }
    await loadPrompts();
    await checkOllama();
    await detectAnthropicKey();
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
            Use AI to enhance transcriptions with grammar correction, formatting, and more
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
      <h3>AI Source</h3>

      <!-- Cloud / Local slider toggle -->
      <div class="backend-toggle-row">
        <span class="toggle-label-left" class:active={isLocalBackend()}>
          &#128187; Local AI
        </span>
        <button
          class="slide-toggle"
          class:cloud-active={isCloudBackend()}
          onclick={isCloudBackend() ? switchToLocal : switchToCloud}
          title={isCloudBackend() ? 'Switch to Local AI' : 'Switch to Cloud AI'}
          aria-label={isCloudBackend() ? 'Currently: Cloud AI. Click to switch to Local' : 'Currently: Local AI. Click to switch to Cloud'}
        >
          <span class="slide-thumb"></span>
        </button>
        <span class="toggle-label-right" class:active={isCloudBackend()}>
          &#9729; Cloud AI
        </span>
      </div>

      <!-- Active backend indicator pill -->
      <div class="active-backend-pill" class:cloud={isCloudBackend()} class:local={isLocalBackend()}>
        {#if isCloudBackend()}
          <span class="pill-icon anthropic-icon-sm">A</span>
          Anthropic &middot; {configStore.config.enhancement.anthropicModel.replace('claude-', 'Claude ').replace('-20251001','').replace('-20241022','')}
        {:else if configStore.config.enhancement.backend === 'openai_compat'}
          <span class="pill-dot"></span>
          OpenAI-compat &middot; {configStore.config.enhancement.ollamaUrl}
        {:else}
          <span class="pill-dot"></span>
          Ollama &middot; {configStore.config.enhancement.model || 'No model selected'}
        {/if}
      </div>

      <!-- Cloud AI section (Anthropic) -->
      {#if isCloudBackend()}
        <div class="backend-section cloud-section">
          <div class="section-label">
            <span class="section-icon anthropic-icon">A</span>
            <span>Anthropic Claude</span>
          </div>

          <!-- Model selector -->
          <div class="setting-row card">
            <div class="setting-info">
              <span class="setting-label">Model</span>
            </div>
            <select
              class="select-control"
              value={configStore.config.enhancement.anthropicModel}
              onchange={handleAnthropicModelChange}
            >
              <option value="claude-haiku-4-5-20251001">Claude Haiku 4.5 (fast)</option>
              <option value="claude-sonnet-4-6">Claude Sonnet 4.6 (balanced)</option>
              <option value="claude-opus-4-6">Claude Opus 4.6 (most capable)</option>
            </select>
          </div>

          <!-- API Key -->
          <div class="setting-row card vertical">
            <div class="setting-info">
              <span class="setting-label">API Key</span>
              <span class="setting-description">
                {#if anthropicKeyDetected}
                  <span class="key-detected">Detected from environment</span>
                {:else}
                  From console.anthropic.com
                {/if}
              </span>
            </div>
            <div class="key-input-group">
              <input
                type={anthropicKeyVisible ? 'text' : 'password'}
                class="url-input key-input"
                value={configStore.config.enhancement.anthropicApiKey}
                placeholder="sk-ant-..."
                oninput={handleAnthropicKeyChange}
                onblur={handleAnthropicKeyBlur}
                autocomplete="off"
              />
              <button
                class="btn-icon toggle-visibility-btn"
                title={anthropicKeyVisible ? 'Hide key' : 'Show key'}
                onclick={() => (anthropicKeyVisible = !anthropicKeyVisible)}
              >
                {#if anthropicKeyVisible}
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94"/>
                    <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"/>
                    <line x1="1" y1="1" x2="23" y2="23"/>
                  </svg>
                {:else}
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
                    <circle cx="12" cy="12" r="3"/>
                  </svg>
                {/if}
              </button>
            </div>
          </div>

          <!-- Get API Key button -->
          {#if !configStore.config.enhancement.anthropicApiKey}
            <div class="get-key-row">
              <button class="btn-outline get-key-btn" onclick={openAnthropicConsole}>
                Get API Key from Anthropic Console &#8599;
              </button>
            </div>
          {/if}

          <!-- Connection status -->
          <div class="connection-status">
            {#if isCheckingOllama}
              <span class="status-checking">Checking...</span>
            {:else if ollamaAvailable}
              <span class="status-connected">Connected</span>
            {:else if configStore.config.enhancement.anthropicApiKey}
              <span class="status-error">Not connected &mdash; check your API key</span>
            {:else}
              <span class="status-warning">Add API key to connect</span>
            {/if}
            <button class="test-btn" onclick={checkOllama} disabled={isCheckingOllama}>
              {isCheckingOllama ? 'Checking...' : 'Test Connection'}
            </button>
          </div>
        </div>
      {/if}

      <!-- Local AI section (Ollama / OpenAI-compat) -->
      {#if isLocalBackend()}
        <div class="backend-section local-section">
          <div class="setting-row card">
            <div class="setting-info">
              <span class="setting-label">Backend</span>
              <span class="setting-description">Choose the AI server type</span>
            </div>
            <select
              class="select-control"
              value={configStore.config.enhancement.backend}
              onchange={handleBackendChange}
            >
              <option value="ollama">Ollama</option>
              <option value="openai_compat">OpenAI Compatible</option>
            </select>
          </div>

          <div class="setting-row card vertical">
            <div class="setting-info">
              <span class="setting-label">Server URL</span>
              <span class="setting-description">
                {configStore.config.enhancement.backend === 'openai_compat'
                  ? 'The base URL of your OpenAI-compatible server'
                  : 'The URL of your local Ollama server'}
              </span>
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
                <span>Connected to server</span>
              {:else}
                <span class="status-indicator disconnected"></span>
                <span>Not connected. Make sure your AI server is running.</span>
              {/if}
            </div>
          </div>

          {#if configStore.config.enhancement.backend === 'openai_compat'}
            <div class="setting-row card vertical">
              <div class="setting-info">
                <span class="setting-label">API Key</span>
                <span class="setting-description">Optional Bearer token for authentication</span>
              </div>
              <input
                type="password"
                class="url-input"
                value={configStore.config.enhancement.apiKey}
                oninput={handleApiKeyChange}
                onblur={handleApiKeyBlur}
                placeholder="sk-... (leave empty if not required)"
                autocomplete="off"
              />
            </div>
          {/if}

          {#if error}
            <div class="error-message">{error}</div>
          {/if}

          <div class="setting-row card">
            <div class="setting-info">
              <span class="setting-label">Model</span>
              <span class="setting-description">The model used for text enhancement</span>
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
              {#if configStore.config.enhancement.backend === 'ollama'}
                Connect to Ollama to see available models. You can download models using
                <code>ollama pull &lt;model&gt;</code> in your terminal.
              {:else}
                Connect to your OpenAI-compatible server to see available models.
              {/if}
            </p>
          {/if}
        </div>
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

  /* ── Backend Toggle ──────────────────────────────────────── */
  .backend-toggle-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 8px 0;
  }

  .toggle-label-left,
  .toggle-label-right {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-tertiary);
    transition: color 0.2s;
    white-space: nowrap;
  }

  .toggle-label-left.active,
  .toggle-label-right.active {
    color: var(--color-text-primary);
    font-weight: 600;
  }

  .slide-toggle {
    position: relative;
    width: 56px;
    height: 30px;
    background: var(--color-bg-tertiary);
    border: 1px solid var(--color-border);
    border-radius: 15px;
    cursor: pointer;
    transition: background 0.25s, border-color 0.25s;
    flex-shrink: 0;
    padding: 0;
  }

  .slide-toggle.cloud-active {
    background: var(--color-accent);
    border-color: var(--color-accent);
  }

  .slide-thumb {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 22px;
    height: 22px;
    background: white;
    border-radius: 50%;
    box-shadow: 0 1px 4px rgba(0,0,0,0.2);
    transition: transform 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .slide-toggle.cloud-active .slide-thumb {
    transform: translateX(26px);
  }

  .active-backend-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px;
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 500;
    align-self: center;
    margin: 0 auto;
  }

  .active-backend-pill.cloud {
    background: color-mix(in srgb, var(--color-accent) 12%, var(--color-bg-secondary));
    color: var(--color-accent);
    border: 1px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
  }

  .active-backend-pill.local {
    background: var(--color-bg-secondary);
    color: var(--color-text-secondary);
    border: 1px solid var(--color-border-subtle);
  }

  .pill-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    font-size: 10px;
    font-weight: 700;
    color: white;
  }

  .anthropic-icon-sm {
    background: #cc785c;
  }

  .pill-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #4caf50;
    flex-shrink: 0;
  }

  /* Backend section wrapper */
  .backend-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border-subtle);
  }

  .section-label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .section-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 700;
    color: white;
  }

  .anthropic-icon {
    background: #cc785c;
  }

  /* API key input group */
  .key-input-group {
    display: flex;
    gap: 4px;
    flex: 1;
  }

  .key-input {
    flex: 1;
    font-family: monospace;
    font-size: 12px;
  }

  .btn-icon {
    padding: 4px 8px;
    background: var(--color-bg-tertiary);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
  }

  .toggle-visibility-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-secondary);
  }

  .toggle-visibility-btn:hover {
    color: var(--color-text-primary);
  }

  .key-detected {
    color: #4caf50;
    font-size: var(--text-xs);
  }

  .get-key-row {
    display: flex;
  }

  .get-key-btn {
    font-size: var(--text-xs);
    padding: 5px 12px;
  }

  .btn-outline {
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .btn-outline:hover {
    background: var(--color-bg-tertiary);
  }

  /* Cloud connection status */
  .connection-status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-xs);
  }

  .status-checking {
    color: var(--color-text-tertiary);
  }
  .status-connected {
    color: #4caf50;
  }
  .status-error {
    color: var(--color-error);
  }
  .status-warning {
    color: var(--color-warning);
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
    transition:
      background var(--transition-fast),
      color var(--transition-fast);
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
