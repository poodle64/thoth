<script lang="ts">
  /**
   * Settings window - configuration interface for Thoth
   *
   * Settings window with 7 panes:
   * Overview, Recording, Models, AI, History, Dictionary, Transcribe
   */

  import type { ComponentType } from 'svelte';
  import { onMount, onDestroy } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import {
    LayoutDashboard,
    Mic,
    Cpu,
    Sparkles,
    History,
    BookOpen,
    FileText,
    Info,
  } from 'lucide-svelte';
  import AIEnhancementSettings from '../components/AIEnhancementSettings.svelte';
  import AudioDeviceSelector from '../components/AudioDeviceSelector.svelte';
  import DictionaryEditor from '../components/DictionaryEditor.svelte';
  import FilterSettings from '../components/FilterSettings.svelte';
  import HistoryPane from '../components/HistoryPane.svelte';
  import ModelManager from '../components/ModelManager.svelte';
  import TranscribePane from '../components/TranscribePane.svelte';
	import OverviewPane from '../components/OverviewPane.svelte';
	import AboutDialog from '../components/AboutDialog.svelte';
	import ShortcutInput from '../components/ShortcutInput.svelte';
	import UpdateNotificationBanner from '../components/UpdateNotificationBanner.svelte';
	import { configStore, type RecordingMode } from '../stores/config.svelte';
	import { pipelineStore } from '../stores/pipeline.svelte';
	import { shortcutsStore, type ShortcutInfo } from '../stores/shortcuts.svelte';
	import { soundStore } from '../stores/sound.svelte';

  /** Settings pane definition */
  interface SettingsPane {
    id: string;
    title: string;
    icon: ComponentType;
  }

  /** Filter options type matching Rust FilterOptions */
  interface FilterOptions {
    remove_fillers: boolean;
    normalise_whitespace: boolean;
    cleanup_punctuation: boolean;
    sentence_case: boolean;
  }

  /** Available settings panes matching Swift app */
  const panes: SettingsPane[] = [
    { id: 'overview', title: 'Overview', icon: LayoutDashboard },
    { id: 'recording', title: 'Recording', icon: Mic },
    { id: 'models', title: 'Models', icon: Cpu },
    { id: 'ai', title: 'AI Enhancement', icon: Sparkles },
    { id: 'history', title: 'History', icon: History },
    { id: 'dictionary', title: 'Dictionary', icon: BookOpen },
    { id: 'transcribe', title: 'Transcribe', icon: FileText },
  ];

  let activePane = $state('overview');

  /** About dialog visibility */
  let showAbout = $state(false);

  /** Map of shortcut IDs to their pending (unsaved) accelerators */
  const pendingChanges = $state<Map<string, string>>(new Map());

  /** Combined list of all shortcuts (registered + defaults not yet registered) */
  const allShortcuts = $derived.by(() => {
    const registered = new Map(shortcutsStore.shortcuts.map((s) => [s.id, s]));
    const combined: ShortcutInfo[] = [];

    for (const def of shortcutsStore.defaults) {
      const reg = registered.get(def.id);
      if (reg) {
        combined.push(reg);
      } else {
        combined.push({ ...def, isEnabled: false });
      }
    }

    for (const reg of shortcutsStore.shortcuts) {
      if (!shortcutsStore.defaults.some((d) => d.id === reg.id)) {
        combined.push(reg);
      }
    }

    return combined;
  });

  function getCurrentAccelerator(shortcut: ShortcutInfo): string {
    return pendingChanges.get(shortcut.id) ?? shortcut.accelerator;
  }

  function getDefaultAccelerator(id: string): string | undefined {
    return shortcutsStore.defaults.find((d) => d.id === id)?.accelerator;
  }

  async function handleShortcutChange(shortcut: ShortcutInfo, newAccelerator: string) {
    const result = await shortcutsStore.update(shortcut.id, newAccelerator);
    if (!result.success) {
      console.error('Failed to update shortcut:', result.error);
      return;
    }
    // Persist to config
    updateShortcutConfig(shortcut.id, newAccelerator);
    await configStore.save();
  }

  async function handleShortcutClear(shortcut: ShortcutInfo) {
    if (shortcut.isEnabled) {
      const result = await shortcutsStore.unregister(shortcut.id);
      if (!result.success) {
        console.error('Failed to unregister shortcut:', result.error);
        return;
      }
    }
    // Clear from config
    updateShortcutConfig(shortcut.id, null);
    await configStore.save();
  }

  async function handleShortcutReset(shortcut: ShortcutInfo) {
    const result = await shortcutsStore.resetToDefault(shortcut.id);
    if (!result.success) {
      console.error('Failed to reset shortcut:', result.error);
      return;
    }
    // Persist default to config
    const defaultAcc = getDefaultAccelerator(shortcut.id);
    if (defaultAcc) {
      updateShortcutConfig(shortcut.id, defaultAcc);
      await configStore.save();
    }
  }

  /** Update shortcut in config based on shortcut ID */
  function updateShortcutConfig(id: string, accelerator: string | null): void {
    switch (id) {
      case 'toggle_recording':
        configStore.updateShortcuts('toggleRecording', accelerator ?? 'F13');
        break;
      case 'toggle_recording_alt':
        configStore.updateShortcuts('toggleRecordingAlt', accelerator);
        break;
      case 'copy_last_transcription':
        configStore.updateShortcuts('copyLast', accelerator);
        break;
    }
  }

  function handleFilterChange(_options: FilterOptions) {
    // Filter change handler - options are passed to HistoryList component
  }

  async function handleRecordingModeChange(mode: RecordingMode) {
    configStore.updateShortcuts('recordingMode', mode);
    await configStore.save();

    const isPttEnabled = mode === 'push_to_talk';
    try {
      await invoke('set_ptt_mode_enabled', { enabled: isPttEnabled });
    } catch (error) {
      console.error('Failed to set PTT mode:', error);
    }
  }

  /** Handle window dragging from title bar */
  function handleDragRegionMouseDown(event: MouseEvent): void {
    if (event.buttons !== 1) return;
    const target = event.target as HTMLElement;
    const interactive = target.closest('button, input, a, [role="button"]');
    if (!interactive) {
      getCurrentWindow()
        .startDragging()
        .catch(() => {});
    }
  }

  let navigateUnlisten: UnlistenFn | null = null;

  onMount(async () => {
    // Listen for tray menu navigation events (e.g. "History...", "Settings...")
    navigateUnlisten = await listen<string>('navigate', (event) => {
      if (panes.some((p) => p.id === event.payload)) {
        activePane = event.payload;
      }
    });
  });

  onDestroy(() => {
    navigateUnlisten?.();
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="settings-window">
  <!-- Title bar with drag region -->
  <header class="title-bar" onmousedown={handleDragRegionMouseDown}>
    <span class="title-text">Thoth</span>
    {#if pipelineStore.isRecording}
      <span class="recording-indicator">
        <span class="recording-dot"></span>
        Recording {pipelineStore.formattedDuration}
      </span>
    {:else if pipelineStore.isProcessing}
      <span class="processing-indicator">Processing...</span>
    {/if}
  </header>

  <div class="main-area">
    <!-- Sidebar -->
    <nav class="sidebar">
      <div class="sidebar-items">
        {#each panes as pane}
          {@const Icon = pane.icon}
          <button
            class="sidebar-item"
            class:active={activePane === pane.id}
            onclick={() => (activePane = pane.id)}
          >
            <span class="sidebar-icon">
              <Icon size={16} />
            </span>
            <span class="sidebar-label">{pane.title}</span>
          </button>
        {/each}
      </div>
      <div class="sidebar-footer">
        <button class="sidebar-item" onclick={() => (showAbout = true)}>
          <span class="sidebar-icon">
            <Info size={16} />
          </span>
          <span class="sidebar-label">About Thoth</span>
        </button>
      </div>
    </nav>

		<!-- Content -->
		<main class="content">
			<!-- Update notification banner (shown at top when update available) -->
			<UpdateNotificationBanner />

			{#if activePane === 'overview'}
        <div class="pane">
          <OverviewPane onNavigate={(paneId) => (activePane = paneId)} />
        </div>
      {:else if activePane === 'recording'}
        <div class="pane">
          <!-- Audio Input Section -->
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Audio Input</h2>
              <p class="section-description">Choose how Thoth selects your audio input device</p>
            </div>
            <div class="section-content">
              <AudioDeviceSelector />
            </div>
          </section>

          <!-- Shortcuts Section -->
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Shortcuts</h2>
              <p class="section-description">
                Tap to start recording, tap again to stop. Hold for push-to-talk.
              </p>
            </div>
            <div class="section-content">
              <div class="mode-selector">
                <button
                  class="mode-option"
                  class:active={configStore.shortcuts.recordingMode === 'toggle'}
                  onclick={() => handleRecordingModeChange('toggle')}
                >
                  <span class="mode-title">Toggle Mode</span>
                  <span class="mode-description">Press to start, press again to stop</span>
                </button>
                <button
                  class="mode-option"
                  class:active={configStore.shortcuts.recordingMode === 'push_to_talk'}
                  onclick={() => handleRecordingModeChange('push_to_talk')}
                >
                  <span class="mode-title">Push-to-Talk</span>
                  <span class="mode-description">Hold to record, release to stop</span>
                </button>
              </div>

              {#if !shortcutsStore.isLoading}
                <div class="shortcuts-list">
                  {#each allShortcuts as shortcut, i (shortcut.id)}
                    <div class="setting-row card">
                      <div class="setting-info">
                        <span class="setting-label">{shortcut.description}</span>
                        <span
                          class="setting-description shortcut-status"
                          class:enabled={shortcut.isEnabled}
                        >
                          {shortcut.isEnabled ? 'Active' : 'Inactive'}
                        </span>
                      </div>
                      <ShortcutInput
                        value={getCurrentAccelerator(shortcut)}
                        shortcutId={shortcut.id}
                        defaultValue={getDefaultAccelerator(shortcut.id)}
                        onchange={(acc) => handleShortcutChange(shortcut, acc)}
                        onclear={() => handleShortcutClear(shortcut)}
                        onreset={() => handleShortcutReset(shortcut)}
                        placeholder="Click to set"
                      />
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          </section>

          <!-- Recording Behaviour Section -->
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Recording Behaviour</h2>
              <p class="section-description">Audio and clipboard handling during transcription</p>
            </div>
            <div class="section-content">
              <div class="setting-row card">
                <div class="setting-info">
                  <span class="setting-label">Sound Feedback</span>
                  <span class="setting-description"
                    >Play sounds when recording starts and stops</span
                  >
                </div>
                <label class="toggle-switch">
                  <input
                    type="checkbox"
                    checked={soundStore.isEnabled}
                    disabled={soundStore.isLoading}
                    onchange={() => soundStore.toggle()}
                  />
                  <span class="toggle-slider"></span>
                </label>
              </div>
              <div class="row-separator"></div>
              <div class="setting-row card">
                <div class="setting-info">
                  <span class="setting-label">Recording Indicator</span>
                  <span class="setting-description"
                    >Show floating indicator during recording</span
                  >
                </div>
                <label class="toggle-switch">
                  <input
                    type="checkbox"
                    checked={configStore.general.showRecordingIndicator}
                    onchange={async () => {
                      const newValue = !configStore.general.showRecordingIndicator;
                      configStore.updateGeneral('showRecordingIndicator', newValue);
                      await configStore.save();

                      try {
                        if (newValue) {
                          if (pipelineStore.isRecording) {
                            await invoke('show_recording_indicator');
                          }
                        } else {
                          await invoke('hide_recording_indicator');
                        }
                      } catch (e) {
                        console.error('Failed to update indicator visibility:', e);
                      }
                    }}
                  />
                  <span class="toggle-slider"></span>
                </label>
              </div>
            </div>
          </section>
        </div>
      {:else if activePane === 'models'}
        <div class="pane">
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Speech Recognition</h2>
              <p class="section-description">Local models for transcribing speech to text</p>
            </div>
            <div class="section-content">
              <ModelManager />
            </div>
          </section>
        </div>
      {:else if activePane === 'ai'}
        <div class="pane">
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">AI Enhancement</h2>
              <p class="section-description">
                Configure AI-powered text enhancement using Ollama for grammar correction,
                formatting, and more.
              </p>
            </div>
            <div class="section-content">
              <AIEnhancementSettings />
            </div>
          </section>
        </div>
      {:else if activePane === 'history'}
        <div class="pane history-pane">
          <HistoryPane />
        </div>
      {:else if activePane === 'dictionary'}
        <div class="pane">
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Dictionary</h2>
              <p class="section-description">
                Add words to help recognition and set up automatic replacements
              </p>
            </div>
            <div class="section-content">
              <DictionaryEditor />
            </div>
          </section>
        </div>
      {:else if activePane === 'transcribe'}
        <div class="pane">
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Import Audio Files</h2>
              <p class="section-description">Transcribe existing audio files</p>
            </div>
            <div class="section-content">
              <TranscribePane />
            </div>
          </section>
          <section class="settings-section">
            <div class="section-header">
              <h2 class="section-title">Output Filtering</h2>
              <p class="section-description">Configure how transcription output is processed</p>
            </div>
            <div class="section-content">
              <FilterSettings
                onchange={handleFilterChange}
                onOpenDictionary={() => (activePane = 'dictionary')}
              />
            </div>
          </section>
        </div>
      {/if}
    </main>
  </div>
</div>

<AboutDialog open={showAbout} onclose={() => (showAbout = false)} />

<style>
  /* Settings window layout - component-specific styles only */
  .settings-window {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100vh;
    background: var(--color-bg-primary);
  }

  /* Title bar */
  .title-bar {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    height: var(--header-height);
    background-color: var(--color-bg-primary);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    -webkit-app-region: drag;
    app-region: drag;
    -webkit-user-select: none;
    user-select: none;
  }

  .title-text {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text-primary);
    user-select: none;
  }

  .main-area {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* Sidebar */
  .sidebar {
    width: var(--sidebar-width);
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    padding-top: 12px;
    border-right: 1px solid var(--color-border);
  }

  .sidebar-items {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 12px;
  }

  .sidebar-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    font-weight: 400;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition-fast), color var(--transition-fast);
  }

  .sidebar-item:hover {
    background-color: rgba(255, 255, 255, 0.07);
    color: var(--color-text-primary);
  }

  .sidebar-item.active {
    background-color: rgba(255, 255, 255, 0.1);
    color: var(--color-text-primary);
    font-weight: 500;
  }

  .sidebar-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    flex-shrink: 0;
    color: inherit;
  }

  .sidebar-label {
    color: inherit;
  }

  .sidebar-footer {
    margin-top: auto;
    padding: 12px;
    border-top: 1px solid var(--color-border);
  }

  /* Content area */
  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 24px 32px 48px 32px;
  }

  .pane {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  /* Mode selector */
  .mode-selector {
    display: flex;
    gap: 12px;
    margin-bottom: 16px;
  }

  .mode-option {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 16px;
    background: var(--color-bg-secondary);
    border: 2px solid var(--color-border-subtle);
    border-radius: var(--radius-md);
    cursor: pointer;
    text-align: left;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }

  .mode-option:hover {
    border-color: var(--color-text-tertiary);
  }

  .mode-option.active {
    border-color: var(--color-accent);
    background: color-mix(in srgb, var(--color-accent) 5%, transparent);
  }

  .mode-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .mode-option.active .mode-title {
    color: var(--color-accent);
  }

  .mode-description {
    font-size: var(--text-xs);
    color: var(--color-text-tertiary);
    line-height: 1.4;
  }

  /* Shortcuts */
  .shortcuts-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .shortcut-status {
    font-size: var(--text-xs);
  }

  .shortcut-status.enabled {
    color: var(--color-success);
  }

  /* History pane: remove content padding so the 3-panel layout fills the space */
  .content:has(.history-pane) {
    padding: 0;
    overflow: hidden;
  }

  .history-pane {
    height: 100%;
  }

  /* Recording indicator */
  .recording-indicator {
    display: flex;
    align-items: center;
    gap: 6px;
    position: absolute;
    right: 16px;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-error);
  }

  .recording-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-error);
    animation: pulse 1s ease-in-out infinite;
  }

  .processing-indicator {
    position: absolute;
    right: 16px;
    font-size: var(--text-xs);
    font-weight: 500;
    color: var(--color-accent);
  }
</style>
