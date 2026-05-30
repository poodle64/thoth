<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { configStore } from '../stores/config.svelte';
  import { toast } from 'svelte-sonner';
  import { Button } from '$components/ui/button';
  import * as Select from '$components/ui/select';
  import * as Dialog from '$components/ui/dialog';
  import * as Alert from '$components/ui/alert';
  import { Select as SelectPrimitive } from 'bits-ui';
  import { Input } from '$components/ui/input';
  import { Textarea } from '$components/ui/textarea';
  import { Switch } from '$components/ui/switch';
  import { Badge } from '$components/ui/badge';
  import { Label } from '$components/ui/label';
  import AlertCircle from '@lucide/svelte/icons/alert-circle';

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

  let isEditing = $state(false);
  let editingPrompt = $state<PromptTemplate | null>(null);
  let newPromptName = $state('');
  let newPromptTemplate = $state('');
  let promptError = $state<string | null>(null);

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

  async function saveSettings(): Promise<void> {
    try {
      await configStore.save();
      toast.success('Settings saved');
    } catch (e) {
      console.error('Failed to save settings:', e);
      toast.error('Failed to save settings');
    }
  }

  async function handleEnabledChange(checked: boolean): Promise<void> {
    configStore.updateEnhancement('enabled', checked);
    await saveSettings();
    invoke('refresh_tray_menu').catch(() => {});
  }

  async function handleModelChange(value: string | undefined): Promise<void> {
    if (value === undefined) return;
    configStore.updateEnhancement('model', value);
    await saveSettings();
  }

  async function handlePromptChange(value: string | undefined): Promise<void> {
    if (value === undefined) return;
    configStore.updateEnhancement('promptId', value);
    await saveSettings();
    invoke('refresh_tray_menu').catch(() => {});
  }

  function handleUrlInput(event: Event): void {
    const input = event.target as HTMLInputElement;
    configStore.updateEnhancement('ollamaUrl', input.value);
  }

  async function handleUrlBlur(): Promise<void> {
    await saveSettings();
    await checkOllama();
  }

  function startNewPrompt(): void {
    isEditing = true;
    editingPrompt = null;
    newPromptName = '';
    newPromptTemplate = 'Enhance the following text:\n\n{text}';
    promptError = null;
  }

  function startEditPrompt(prompt: PromptTemplate): void {
    if (prompt.isBuiltin) return;
    isEditing = true;
    editingPrompt = prompt;
    newPromptName = prompt.name;
    newPromptTemplate = prompt.template;
    promptError = null;
  }

  function cancelEdit(): void {
    isEditing = false;
    editingPrompt = null;
    promptError = null;
  }

  function generateId(name: string): string {
    return name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '');
  }

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
      isEditing = false;
      invoke('refresh_tray_menu').catch(() => {});
    } catch (e) {
      promptError = e instanceof Error ? e.message : 'Failed to save prompt';
    }
  }

  async function deletePrompt(promptId: string): Promise<void> {
    try {
      await invoke('delete_custom_prompt_cmd', { promptId });
      await loadPrompts();
      if (configStore.enhancement.promptId === promptId) {
        configStore.updateEnhancement('promptId', 'fix-grammar');
        await saveSettings();
      }
      invoke('refresh_tray_menu').catch(() => {});
    } catch (e) {
      console.error('Failed to delete prompt:', e);
    }
  }

  function getSelectedPrompt(): PromptTemplate | undefined {
    return prompts.find((p) => p.id === configStore.enhancement.promptId);
  }

  async function openPromptGuide(): Promise<void> {
    try {
      await invoke('show_window', { label: 'prompt-guide' });
    } catch (e) {
      console.error('Failed to open prompt guide:', e);
    }
  }

  /** Derive model select value — falls back to empty string so Select shows placeholder */
  let modelSelectValue = $derived(configStore.config.enhancement.model ?? '');
  let promptSelectValue = $derived(configStore.config.enhancement.promptId ?? '');

  onMount(async () => {
    await configStore.load();
    await loadPrompts();
    await checkOllama();
  });
</script>

<div class="flex flex-col gap-6">
  {#if configStore.isLoading}
    <p class="text-sm text-muted-foreground text-center py-6">Loading settings...</p>
  {:else}
    <!-- Enable toggle -->
    <div class="flex items-center justify-between gap-4 rounded-lg border bg-card px-4 py-3">
      <div class="flex flex-col gap-0.5">
        <Label class="text-sm font-medium">Enable AI enhancement</Label>
        <p class="text-xs text-muted-foreground">
          Use Ollama to enhance transcriptions with grammar correction, formatting, and more
        </p>
      </div>
      <Switch
        checked={configStore.config.enhancement.enabled}
        onCheckedChange={handleEnabledChange}
      />
    </div>

    <!-- Ollama connection -->
    <div class="flex flex-col gap-3">
      <h3 class="text-sm font-semibold text-foreground">Ollama Connection</h3>

      <div class="flex flex-col gap-3 rounded-lg border bg-card px-4 py-3">
        <div class="flex flex-col gap-1">
          <Label class="text-sm font-medium">Server URL</Label>
          <p class="text-xs text-muted-foreground">The URL of your local Ollama server</p>
        </div>
        <div class="flex gap-2">
          <Input
            type="url"
            class="flex-1 font-mono text-sm"
            value={configStore.config.enhancement.ollamaUrl}
            oninput={handleUrlInput}
            onblur={handleUrlBlur}
            placeholder="http://localhost:11434"
          />
          <Button variant="outline" onclick={checkOllama} disabled={isCheckingOllama}>
            {isCheckingOllama ? 'Testing...' : 'Test Connection'}
          </Button>
        </div>
        <div class="flex items-center gap-2">
          {#if isCheckingOllama}
            <Badge variant="secondary">Checking…</Badge>
          {:else if ollamaAvailable}
            <Badge
              class="bg-green-500/10 text-green-600 dark:text-green-400 border-green-500/20"
              variant="outline"
            >
              Connected
            </Badge>
          {:else}
            <Badge variant="destructive">Not connected — is Ollama running?</Badge>
          {/if}
        </div>
      </div>

      {#if error}
        <Alert.Root variant="destructive">
          <AlertCircle class="size-4" />
          <Alert.Description>{error}</Alert.Description>
        </Alert.Root>
      {/if}

      <!-- Model selector -->
      <div class="flex items-center justify-between gap-4 rounded-lg border bg-card px-4 py-3">
        <div class="flex flex-col gap-0.5">
          <Label class="text-sm font-medium">Model</Label>
          <p class="text-xs text-muted-foreground">The Ollama model used for text enhancement</p>
        </div>
        <Select.Root
          type="single"
          value={modelSelectValue}
          onValueChange={handleModelChange}
          disabled={!ollamaAvailable || isLoadingModels}
        >
          <Select.Trigger class="w-48">
            <SelectPrimitive.Value
              placeholder={isLoadingModels
                ? 'Loading models…'
                : ollamaModels.length === 0
                  ? configStore.config.enhancement.model
                  : 'Select model…'}
            />
          </Select.Trigger>
          <Select.Content>
            {#if !isLoadingModels && ollamaModels.length === 0}
              <Select.Item value={configStore.config.enhancement.model}>
                {configStore.config.enhancement.model} (not available)
              </Select.Item>
            {:else}
              {#each ollamaModels as model}
                <Select.Item value={model}>{model}</Select.Item>
              {/each}
            {/if}
          </Select.Content>
        </Select.Root>
      </div>

      {#if !ollamaAvailable}
        <p class="text-xs text-muted-foreground">
          Connect to Ollama to see available models. You can download models using
          <code class="font-mono bg-muted px-1 py-0.5 rounded text-xs"
            >ollama pull &lt;model&gt;</code
          >
          in your terminal.
        </p>
      {/if}
    </div>

    <!-- Prompt templates -->
    <div class="flex flex-col gap-3">
      <h3 class="text-sm font-semibold text-foreground">Prompt Templates</h3>

      <!-- Active prompt -->
      <div class="flex flex-col gap-3 rounded-lg border bg-card px-4 py-3">
        <div class="flex items-center justify-between gap-4">
          <div class="flex flex-col gap-0.5">
            <Label class="text-sm font-medium">Active Prompt</Label>
            <p class="text-xs text-muted-foreground">
              The prompt template used to enhance transcriptions
            </p>
          </div>
          <Select.Root
            type="single"
            value={promptSelectValue}
            onValueChange={handlePromptChange}
            disabled={isLoadingPrompts}
          >
            <Select.Trigger class="w-48">
              <SelectPrimitive.Value
                placeholder={isLoadingPrompts ? 'Loading…' : 'Select prompt…'}
              />
            </Select.Trigger>
            <Select.Content>
              {#each prompts as prompt}
                <Select.Item value={prompt.id}>
                  {prompt.name}{prompt.isBuiltin ? '' : ' (Custom)'}
                </Select.Item>
              {/each}
            </Select.Content>
          </Select.Root>
        </div>
        {#if getSelectedPrompt()}
          <pre
            class="text-xs font-mono text-muted-foreground whitespace-pre-wrap break-words leading-relaxed bg-muted/50 rounded px-3 py-2">{getSelectedPrompt()
              ?.template}</pre>
        {/if}
      </div>

      <!-- Custom prompts -->
      <div class="flex flex-col gap-3">
        <div class="flex items-center justify-between">
          <span class="text-sm font-medium text-foreground">Custom Prompts</span>
          <Button variant="outline" size="sm" onclick={startNewPrompt}>+ Add Prompt</Button>
        </div>

        <p class="text-xs text-muted-foreground">
          Create your own prompt templates for specific use cases.
          <button
            class="text-primary underline-offset-2 hover:underline bg-transparent border-none p-0 text-xs font-inherit cursor-pointer"
            onclick={openPromptGuide}
          >
            View Prompt Writing Guide
          </button>
          for tips on writing effective prompts.
        </p>

        <!-- Prompt editor dialog -->
        <Dialog.Root
          bind:open={isEditing}
          onOpenChange={(open) => {
            if (!open) cancelEdit();
          }}
        >
          <Dialog.Content class="max-w-lg">
            <Dialog.Header>
              <Dialog.Title>{editingPrompt ? 'Edit Prompt' : 'New Prompt'}</Dialog.Title>
              <Dialog.Description>
                Define a custom prompt template. Use <code
                  class="font-mono text-xs bg-muted px-1 py-0.5 rounded">{'{text}'}</code
                > as the transcription placeholder.
              </Dialog.Description>
            </Dialog.Header>

            <div class="flex flex-col gap-4 py-2">
              <div class="flex flex-col gap-1.5">
                <Label for="prompt-name">Name</Label>
                <Input
                  id="prompt-name"
                  type="text"
                  bind:value={newPromptName}
                  placeholder="My Custom Prompt"
                />
              </div>

              <div class="flex flex-col gap-1.5">
                <Label for="prompt-template">
                  Template
                  <span class="text-muted-foreground font-normal ml-1 text-xs">
                    (use {'{text}'} as placeholder)
                  </span>
                </Label>
                <Textarea
                  id="prompt-template"
                  bind:value={newPromptTemplate}
                  rows={5}
                  class="font-mono text-sm resize-y"
                  placeholder="Enhance this text: {'{text}'}"
                />
              </div>

              {#if promptError}
                <Alert.Root variant="destructive">
                  <AlertCircle class="size-4" />
                  <Alert.Description>{promptError}</Alert.Description>
                </Alert.Root>
              {/if}
            </div>

            <Dialog.Footer>
              <Button variant="outline" onclick={cancelEdit}>Cancel</Button>
              <Button onclick={savePrompt}>
                {editingPrompt ? 'Update' : 'Create'} Prompt
              </Button>
            </Dialog.Footer>
          </Dialog.Content>
        </Dialog.Root>

        <!-- Custom prompts list -->
        <div class="flex flex-col gap-2">
          {#each prompts.filter((p) => !p.isBuiltin) as prompt}
            <div class="flex items-center justify-between rounded-lg bg-muted/40 px-3 py-2.5">
              <span class="text-sm font-medium">{prompt.name}</span>
              <div class="flex gap-1.5">
                <Button variant="ghost" size="sm" onclick={() => startEditPrompt(prompt)}>
                  Edit
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  class="text-destructive hover:text-destructive hover:bg-destructive/10"
                  onclick={() => deletePrompt(prompt.id)}
                >
                  Delete
                </Button>
              </div>
            </div>
          {:else}
            <p class="text-sm text-muted-foreground text-center py-4">
              No custom prompts yet. Click "Add Prompt" to create one.
            </p>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>
