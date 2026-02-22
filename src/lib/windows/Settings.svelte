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
    HardDrive,
    Info,
  } from 'lucide-svelte';
  import AIEnhancementSettings from '../components/AIEnhancementSettings.svelte';
  import AudioDeviceSelector from '../components/AudioDeviceSelector.svelte';
  import DictionaryEditor from '../components/DictionaryEditor.svelte';
  import FilterSettings from '../components/FilterSettings.svelte';
  import StoragePane from '../components/StoragePane.svelte';
  import HistoryPane from '../components/HistoryPane.svelte';
  import ModelManager from '../components/ModelManager.svelte';
  import TranscribePane from '../components/TranscribePane.svelte';
	import OverviewPane from '../components/OverviewPane.svelte';
	import AboutDialog from '../components/AboutDialog.svelte';
	import ShortcutInput from '../components/ShortcutInput.svelte';
	import UpdateNotificationBanner from '../components/UpdateNotificationBanner.svelte';
	import { configStore, type RecordingMode, type IndicatorStyle } from '../stores/config.svelte';
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
    { id: 'storage', title: 'Storage', icon: HardDrive },
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
    // Update in-memory config, then save directly via set_shortcut_config
    // which bypasses the preservation logic in set_config. This ensures
    // the shortcut value is saved even when it matches the default.
    // exit_capture_mode (called by ShortcutInput.stopCapture) will then
    // re-register all shortcuts from the saved config.
    updateShortcutConfig(shortcut.id, newAccelerator);
    await saveShortcutConfig();

    // Reload registered shortcuts after the capture cycle completes.
    // onchange fires before stopCapture/exit_capture_mode, so we defer
    // the reload to run after the full async chain finishes.
    setTimeout(() => shortcutsStore.loadRegistered(), 100);
  }

  async function handleShortcutClear(shortcut: ShortcutInfo) {
    // Clear from config using bypass, then re-register all shortcuts
    updateShortcutConfig(shortcut.id, null);
    await saveShortcutConfig();
    await reRegisterShortcuts();
  }

  async function handleShortcutReset(shortcut: ShortcutInfo) {
    // Reset to default value using bypass (important: preservation logic in
    // set_config would block this since the incoming value matches the default)
    const defaultAcc = getDefaultAccelerator(shortcut.id);
    if (defaultAcc) {
      updateShortcutConfig(shortcut.id, defaultAcc);
      await saveShortcutConfig();
      await reRegisterShortcuts();
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

  /**
   * Save shortcut config directly via the bypass IPC command.
   * This avoids the preservation logic in set_config that would prevent
   * resetting shortcuts back to their default values.
   */
  async function saveShortcutConfig(): Promise<void> {
    try {
      await invoke('set_shortcut_config', {
        shortcuts: {
          toggle_recording: configStore.shortcuts.toggleRecording,
          toggle_recording_alt: configStore.shortcuts.toggleRecordingAlt,
          copy_last: configStore.shortcuts.copyLast,
          recording_mode: configStore.shortcuts.recordingMode,
        },
      });
    } catch (e) {
      console.error('Failed to save shortcut config:', e);
    }
  }

  /**
   * Unregister all shortcuts and re-register from saved config.
   * Used after clear/reset operations that change config outside of capture mode.
   */
  async function reRegisterShortcuts(): Promise<void> {
    try {
      await invoke('reregister_shortcuts');
      await shortcutsStore.loadRegistered();
    } catch (e) {
      console.error('Failed to re-register shortcuts:', e);
    }
  }

  function handleFilterChange(_options: FilterOptions) {
    // Filter change handler - options are passed to HistoryList component
  }

  async function handleRecordingModeChange(mode: RecordingMode) {
    configStore.updateShortcuts('recordingMode', mode);
    await saveShortcutConfig();

    const isPttEnabled = mode === 'push_to_talk';
    try {
      await invoke('set_ptt_mode_enabled', { enabled: isPttEnabled });
    } catch (error) {
      console.error('Failed to set PTT mode:', error);
    }
  }

  async function handleIndicatorStyleChange(style: IndicatorStyle) {
    configStore.updateGeneral('indicatorStyle', style);
    await configStore.save();

    // If currently recording, re-show the indicator to apply the new style
    try {
      if (pipelineStore.isRecording && configStore.general.showRecordingIndicator) {
        await invoke('hide_recording_indicator');
        await invoke('show_recording_indicator');
      }
    } catch (e) {
      console.error('Failed to update indicator style:', e);
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
              {#if configStore.general.showRecordingIndicator}
                <div class="row-separator"></div>
                <div class="setting-row card">
                  <div class="setting-info">
                    <span class="setting-label">Indicator Style</span>
                    <span class="setting-description">Visual appearance of the recording indicator</span>
                  </div>
                </div>
                <div class="indicator-style-selector">
                  <button
                    class="mode-option"
                    class:active={configStore.general.indicatorStyle === 'cursor-dot'}
                    onclick={() => handleIndicatorStyleChange('cursor-dot')}
                  >
                    <div class="mode-preview">
                      <svg viewBox="0 0 80 52" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <!-- Cursor arrow -->
                        <path d="M20 8 L20 36 L26 30 L32 40 L36 38 L30 28 L38 28 Z" fill="var(--color-text-secondary)" stroke="var(--color-bg-primary)" stroke-width="1.5"/>
                        <!-- Glowing dot -->
                        <circle cx="52" cy="22" r="10" fill="var(--color-accent)" opacity="0.2"/>
                        <circle cx="52" cy="22" r="7" fill="var(--color-accent)" opacity="0.4"/>
                        <circle cx="52" cy="22" r="4.5" fill="var(--color-accent)"/>
                        <!-- Mic icon inside dot -->
                        <rect x="50.5" y="18" width="3" height="5" rx="1.5" fill="white"/>
                        <path d="M49.5 22.5 C49.5 24 50.5 25 52 25 C53.5 25 54.5 24 54.5 22.5" stroke="white" stroke-width="0.8" fill="none"/>
                        <line x1="52" y1="25" x2="52" y2="26.5" stroke="white" stroke-width="0.8"/>
                      </svg>
                    </div>
                    <span class="mode-title">Cursor Dot</span>
                    <span class="mode-description">Follows your mouse cursor</span>
                  </button>
                  <button
                    class="mode-option"
                    class:active={configStore.general.indicatorStyle === 'fixed-float'}
                    onclick={() => handleIndicatorStyleChange('fixed-float')}
                  >
                    <div class="mode-preview">
                      <svg viewBox="0 0 80 52" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <!-- Screen outline -->
                        <rect x="10" y="6" width="60" height="36" rx="3" stroke="var(--color-text-tertiary)" stroke-width="1.5" fill="none"/>
                        <!-- Screen stand -->
                        <line x1="32" y1="42" x2="48" y2="42" stroke="var(--color-text-tertiary)" stroke-width="1.5" stroke-linecap="round"/>
                        <line x1="40" y1="42" x2="40" y2="46" stroke="var(--color-text-tertiary)" stroke-width="1.5"/>
                        <line x1="34" y1="46" x2="46" y2="46" stroke="var(--color-text-tertiary)" stroke-width="1.5" stroke-linecap="round"/>
                        <!-- Fixed dot in top-right corner of screen -->
                        <circle cx="59" cy="16" r="6" fill="var(--color-accent)" opacity="0.3"/>
                        <circle cx="59" cy="16" r="4" fill="var(--color-accent)"/>
                        <!-- Mic icon inside dot -->
                        <rect x="58" y="13.5" width="2" height="3.5" rx="1" fill="white"/>
                        <path d="M57.2 16.5 C57.2 17.5 58 18.2 59 18.2 C60 18.2 60.8 17.5 60.8 16.5" stroke="white" stroke-width="0.6" fill="none"/>
                        <line x1="59" y1="18.2" x2="59" y2="19" stroke="white" stroke-width="0.6"/>
                      </svg>
                    </div>
                    <span class="mode-title">Fixed Float</span>
                    <span class="mode-description">Stays at a fixed screen position</span>
                  </button>
                  <button
                    class="mode-option"
                    class:active={configStore.general.indicatorStyle === 'pill'}
                    onclick={() => handleIndicatorStyleChange('pill')}
                  >
                    <div class="mode-preview">
                      <svg viewBox="0 0 80 52" fill="none" xmlns="http://www.w3.org/2000/svg">
                        <!-- Pill shape -->
                        <rect x="8" y="16" width="64" height="20" rx="10" fill="var(--color-accent)"/>
                        <!-- Mic icon on left side -->
                        <rect x="17" y="22" width="4" height="6" rx="2" fill="white"/>
                        <path d="M15.5 27 C15.5 29 17 30.5 19 30.5 C21 30.5 22.5 29 22.5 27" stroke="white" stroke-width="1" fill="none"/>
                        <line x1="19" y1="30.5" x2="19" y2="32" stroke="white" stroke-width="1"/>
                        <!-- Waveform bars -->
                        <rect x="28" y="24" width="2.5" height="4" rx="1" fill="white" opacity="0.45"/>
                        <rect x="33" y="22" width="2.5" height="8" rx="1" fill="white" opacity="0.5"/>
                        <rect x="38" y="20" width="2.5" height="12" rx="1" fill="white" opacity="0.55"/>
                        <rect x="43" y="23" width="2.5" height="6" rx="1" fill="white" opacity="0.65"/>
                        <rect x="48" y="19" width="2.5" height="14" rx="1" fill="white" opacity="0.75"/>
                        <rect x="53" y="21" width="2.5" height="10" rx="1" fill="white" opacity="0.85"/>
                        <rect x="58" y="23" width="2.5" height="6" rx="1" fill="white" opacity="0.95"/>
                        <rect x="63" y="24" width="2.5" height="4" rx="1" fill="white"/>
                      </svg>
                    </div>
                    <span class="mode-title">Pill Bar</span>
                    <span class="mode-description">Waveform bar at top of screen</span>
                  </button>
                </div>
              {/if}
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
      {:else if activePane === 'storage'}
        <div class="pane">
          <StoragePane />
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
  .mode-selector,
  .indicator-style-selector {
    display: flex;
    gap: 12px;
    margin-bottom: 16px;
  }

  .indicator-style-selector {
    margin-top: 8px;
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

  .indicator-style-selector .mode-option {
    align-items: center;
    text-align: center;
    gap: 6px;
    padding: 12px 12px 14px;
  }

  .mode-preview {
    width: 100%;
    max-width: 80px;
    aspect-ratio: 80 / 52;
  }

  .mode-preview svg {
    width: 100%;
    height: 100%;
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
